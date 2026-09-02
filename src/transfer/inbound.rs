//! An accepted inbound transfer: per-file tokens, streaming to `.part` files
//! with an atomic rename on completion, and progress the UI reads lock-free.
//!
//! One session at a time (the server answers 409 while one is active). Files
//! within the session may upload in parallel — each `/upload` request streams
//! a different slot from its own connection thread, so per-slot atomics need
//! no coordination.

use super::route::SaveRouter;
use crate::net::protocol::{self, FileMeta};
use crate::net::{Wake, WakeReason};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Streaming chunk size; also the granularity of cancel checks.
const CHUNK: usize = 64 * 1024;
/// Progress wakes are throttled to one per this interval (retsurf's cadence)
/// so a fast sender can't flood the SDL event queue.
const NOTIFY_EVERY: Duration = Duration::from_millis(250);
/// A session with no bytes moving for this long is abandoned: the next
/// prepare-upload may evict it. Individual reads already die at the socket
/// timeout, so a stall never extends this by much.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq)]
pub enum FileState {
    Pending,
    Receiving,
    Done,
    Failed(String),
}

pub struct FileSlot {
    pub meta: FileMeta,
    pub token: String,
    /// Final destination (see [`SaveRouter::dest_for`]).
    pub dest: PathBuf,
    /// Where the body streams until it is whole (see [`super::files::part_path`]).
    part: PathBuf,
    pub state: Mutex<FileState>,
    pub received: AtomicU64,
}

pub struct InboundSession {
    pub session_id: String,
    pub peer_alias: String,
    pub files: Vec<FileSlot>,
    by_id: HashMap<String, usize>,
    pub total_bytes: u64,
    pub received_total: AtomicU64,
    pub cancelled: AtomicBool,
    pub started_at: Instant,
    last_activity: Mutex<Instant>,
    last_wake: Mutex<Instant>,
}

impl InboundSession {
    /// Build a session for `files`, assigning destinations and per-file
    /// tokens. Each file's directory (per the `router`) is created on demand.
    pub fn new(
        peer_alias: String,
        files: Vec<FileMeta>,
        router: &SaveRouter,
    ) -> std::io::Result<Self> {
        let mut slots = Vec::with_capacity(files.len());
        let mut by_id = HashMap::with_capacity(files.len());
        let mut taken = HashSet::new();
        let mut total = 0u64;
        for meta in files {
            let rel = super::files::sanitize_relative_path(&meta.file_name);
            let dir = router.dir_for(&rel);
            std::fs::create_dir_all(&dir)?;
            let dest = router.dest_for(&rel, &taken);
            // Belt and braces on top of sanitize: never outside its dir.
            assert_eq!(dest.parent(), Some(dir.as_path()));
            taken.insert(dest.clone());
            total += meta.size;
            by_id.insert(meta.id.clone(), slots.len());
            slots.push(FileSlot {
                token: protocol::random_token(16),
                // Its own tag rather than the token, which is a secret and has
                // no business in a name anyone can list.
                part: super::files::part_path(&dest, &protocol::random_token(4)),
                dest,
                meta,
                state: Mutex::new(FileState::Pending),
                received: AtomicU64::new(0),
            });
        }

        Ok(Self {
            session_id: protocol::random_token(16),
            peer_alias,
            files: slots,
            by_id,
            total_bytes: total,
            received_total: AtomicU64::new(0),
            cancelled: AtomicBool::new(false),
            started_at: Instant::now(),
            last_activity: Mutex::new(Instant::now()),
            last_wake: Mutex::new(Instant::now()),
        })
    }

    /// fileId → token map for the prepare-upload response.
    pub fn tokens(&self) -> std::collections::BTreeMap<String, String> {
        self.files
            .iter()
            .map(|s| (s.meta.id.clone(), s.token.clone()))
            .collect()
    }

    /// Stream one file's body to disk. Called on the connection thread;
    /// returns the HTTP status to answer with.
    pub fn receive_file(
        &self,
        file_id: &str,
        token: &str,
        body: &mut dyn Read,
        wake: &dyn Wake,
    ) -> u16 {
        let Some(&idx) = self.by_id.get(file_id) else {
            return 403;
        };
        let slot = &self.files[idx];
        if slot.token != token {
            return 403;
        }
        if self.cancelled.load(Ordering::SeqCst) {
            return 409;
        }
        {
            let mut state = slot.state.lock().unwrap();
            match &*state {
                FileState::Done => return 200, // idempotent retry after a lost response
                FileState::Receiving => return 409,
                FileState::Pending | FileState::Failed(_) => *state = FileState::Receiving,
            }
        }

        match self.stream_to_disk(slot, body, wake) {
            Ok(()) => {
                *slot.state.lock().unwrap() = FileState::Done;
                self.touch();
                self.maybe_wake(wake);
                200
            }
            Err((status, message)) => {
                log::warn!("receive `{}` failed: {message}", slot.meta.file_name);
                let _ = std::fs::remove_file(&slot.part);
                // Roll the totals back so the overall bar doesn't count bytes
                // of a file that will be reported failed.
                let received = slot.received.swap(0, Ordering::SeqCst);
                self.received_total.fetch_sub(received, Ordering::SeqCst);
                *slot.state.lock().unwrap() = FileState::Failed(message);
                wake.wake(WakeReason::Progress);
                status
            }
        }
    }

    fn stream_to_disk(
        &self,
        slot: &FileSlot,
        body: &mut dyn Read,
        wake: &dyn Wake,
    ) -> Result<(), (u16, String)> {
        let part = &slot.part;
        let mut file = std::fs::File::create(part)
            .map_err(|e| (500, format!("create `{}`: {e}", part.display())))?;

        let mut buf = vec![0u8; CHUNK];
        // Read no further than the declared size: a chunked body has no length
        // of its own, so an over-long one would otherwise fill the card before
        // the check below ever ran.
        let mut left = slot.meta.size;
        while left > 0 {
            if self.cancelled.load(Ordering::SeqCst) {
                return Err((409, "session cancelled".to_string()));
            }
            let want = left.min(CHUNK as u64) as usize;
            let n = match body.read(&mut buf[..want]) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err((500, format!("read body: {e}"))),
            };
            file.write_all(&buf[..n]).map_err(|e| map_write_error(&e))?;
            left -= n as u64;
            slot.received.fetch_add(n as u64, Ordering::SeqCst);
            self.received_total.fetch_add(n as u64, Ordering::SeqCst);
            self.touch();
            self.maybe_wake(wake);
        }

        let received = slot.received.load(Ordering::SeqCst);
        if received != slot.meta.size {
            return Err((
                500,
                format!("size mismatch: got {received}, expected {}", slot.meta.size),
            ));
        }
        // One byte past the declaration is a sender that lied about the size;
        // nothing of it is on disk, and taking it as complete would be worse.
        if body.read(&mut buf[..1]).unwrap_or(0) != 0 {
            return Err((
                400,
                format!("body longer than the declared {} bytes", slot.meta.size),
            ));
        }
        // fsync before the rename: an SD yank must not leave a truncated file
        // that looks complete.
        file.sync_all().map_err(|e| map_write_error(&e))?;
        drop(file);
        std::fs::rename(part, &slot.dest)
            .map_err(|e| (500, format!("rename to `{}`: {e}", slot.dest.display())))?;
        Ok(())
    }

    /// Cancel from either side: flips the flag (streaming threads abort at
    /// the next chunk and clean their `.part`s) and fails all pending slots.
    pub fn cancel(&self, reason: &str) {
        self.cancelled.store(true, Ordering::SeqCst);
        for slot in &self.files {
            let mut state = slot.state.lock().unwrap();
            if *state == FileState::Pending {
                *state = FileState::Failed(reason.to_string());
            }
        }
    }

    /// Every slot reached a terminal state (the receiving threads are done).
    pub fn is_finished(&self) -> bool {
        self.files.iter().all(|s| {
            matches!(
                &*s.state.lock().unwrap(),
                FileState::Done | FileState::Failed(_)
            )
        })
    }

    /// Abandoned: nothing terminal-bound is moving. The sender accepted our
    /// prepare response and then disappeared without uploading (or died
    /// mid-session past the socket timeouts).
    pub fn is_stale(&self) -> bool {
        !self.is_finished() && self.last_activity.lock().unwrap().elapsed() > IDLE_TIMEOUT
    }

    pub fn done_count(&self) -> usize {
        self.files
            .iter()
            .filter(|s| *s.state.lock().unwrap() == FileState::Done)
            .count()
    }

    fn touch(&self) {
        *self.last_activity.lock().unwrap() = Instant::now();
    }

    /// Throttled progress wake shared by all streaming threads.
    fn maybe_wake(&self, wake: &dyn Wake) {
        let mut last = self.last_wake.lock().unwrap();
        if last.elapsed() >= NOTIFY_EVERY {
            *last = Instant::now();
            drop(last);
            wake.wake(WakeReason::Progress);
        }
    }
}

fn map_write_error(e: &std::io::Error) -> (u16, String) {
    if e.raw_os_error() == Some(libc_enospc()) {
        (500, "disk full".to_string())
    } else {
        (500, format!("write: {e}"))
    }
}

/// ENOSPC without a libc dependency — the value is part of the Linux ABI.
const fn libc_enospc() -> i32 {
    28
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::WakeReason;

    struct NoopWake;
    impl Wake for NoopWake {
        fn wake(&self, _: WakeReason) {}
    }

    fn meta(id: &str, name: &str, size: u64) -> FileMeta {
        FileMeta {
            id: id.into(),
            file_name: name.into(),
            size,
            file_type: "application/octet-stream".into(),
            sha256: None,
            preview: None,
            metadata: None,
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lsretro-inbound-{tag}-{}",
            protocol::random_token(4)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A no-routes router that lands everything in `dir`, keeping both on a
    /// name collision.
    fn router(dir: &std::path::Path) -> SaveRouter {
        SaveRouter::into_dir(dir.to_path_buf(), false, true)
    }

    #[test]
    fn receives_a_file_end_to_end() {
        let dir = temp_dir("ok");
        let session = InboundSession::new(
            "Phone".into(),
            vec![meta("a", "game.gbc", 5)],
            &router(&dir),
        )
        .unwrap();
        let token = session.tokens()["a"].clone();

        let status = session.receive_file("a", &token, &mut &b"hello"[..], &NoopWake);
        assert_eq!(status, 200);
        assert_eq!(std::fs::read(dir.join("game.gbc")).unwrap(), b"hello");
        assert!(!dir.join("game.gbc.part").exists());
        assert!(session.is_finished());
        assert_eq!(session.done_count(), 1);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A sender may name one file like another's streaming path. Both are
    /// files it asked us to keep, so neither may be streamed over.
    #[test]
    fn a_file_named_like_a_part_path_survives_its_neighbour() {
        let dir = temp_dir("part-clash");
        let session = InboundSession::new(
            "Phone".into(),
            vec![meta("a", "x.bin.part", 4), meta("b", "x.bin", 4)],
            &router(&dir),
        )
        .unwrap();

        for (id, body) in [("a", &b"AAAA"[..]), ("b", &b"BBBB"[..])] {
            let token = session.tokens()[id].clone();
            assert_eq!(
                session.receive_file(id, &token, &mut &body[..], &NoopWake),
                200
            );
        }

        assert_eq!(std::fs::read(dir.join("x.bin.part")).unwrap(), b"AAAA");
        assert_eq!(std::fs::read(dir.join("x.bin")).unwrap(), b"BBBB");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The streaming path leaves nothing behind, whatever it was called.
    #[test]
    fn a_finished_receive_leaves_no_part_file() {
        let dir = temp_dir("no-debris");
        let session =
            InboundSession::new("Phone".into(), vec![meta("a", "rom.gbc", 2)], &router(&dir))
                .unwrap();
        let token = session.tokens()["a"].clone();
        assert_eq!(
            session.receive_file("a", &token, &mut &b"hi"[..], &NoopWake),
            200
        );

        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".part"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A body past the declared size is refused, and — the point of it — the
    /// card never takes the excess: the reader is not drained past the size.
    #[test]
    fn a_body_longer_than_declared_is_refused_before_the_card_fills() {
        /// Serves as much as it is asked for, counting the bytes handed over.
        struct Endless {
            served: u64,
        }
        impl Read for Endless {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                buf.fill(b'x');
                self.served += buf.len() as u64;
                Ok(buf.len())
            }
        }

        let dir = temp_dir("overlong");
        let declared = 4u64;
        let session = InboundSession::new(
            "Phone".into(),
            vec![meta("a", "small.bin", declared)],
            &router(&dir),
        )
        .unwrap();
        let token = session.tokens()["a"].clone();

        let mut body = Endless { served: 0 };
        let status = session.receive_file("a", &token, &mut body, &NoopWake);

        assert_eq!(status, 400);
        // The declared bytes, plus the one byte that proves there are more.
        assert_eq!(body.served, declared + 1);
        assert!(!dir.join("small.bin").exists());
        assert_eq!(session.received_total.load(Ordering::SeqCst), 0);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_bad_token_and_unknown_file() {
        let dir = temp_dir("token");
        let session =
            InboundSession::new("Phone".into(), vec![meta("a", "x.bin", 1)], &router(&dir))
                .unwrap();
        assert_eq!(
            session.receive_file("a", "wrong", &mut &b"x"[..], &NoopWake),
            403
        );
        assert_eq!(
            session.receive_file("nope", "wrong", &mut &b"x"[..], &NoopWake),
            403
        );
        assert!(!session.is_finished());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn size_mismatch_fails_and_cleans_up() {
        let dir = temp_dir("short");
        let session =
            InboundSession::new("Phone".into(), vec![meta("a", "x.bin", 10)], &router(&dir))
                .unwrap();
        let token = session.tokens()["a"].clone();
        let status = session.receive_file("a", &token, &mut &b"tiny"[..], &NoopWake);
        assert_eq!(status, 500);
        assert!(!dir.join("x.bin").exists());
        assert!(!dir.join("x.bin.part").exists());
        assert_eq!(session.received_total.load(Ordering::SeqCst), 0);
        assert!(matches!(
            &*session.files[0].state.lock().unwrap(),
            FileState::Failed(_)
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cancel_fails_pending_and_blocks_uploads() {
        let dir = temp_dir("cancel");
        let session = InboundSession::new(
            "Phone".into(),
            vec![meta("a", "one.bin", 1), meta("b", "two.bin", 1)],
            &router(&dir),
        )
        .unwrap();
        let token = session.tokens()["a"].clone();
        session.cancel("cancelled by sender");
        assert!(session.is_finished());
        assert_eq!(
            session.receive_file("a", &token, &mut &b"x"[..], &NoopWake),
            409
        );
        assert_eq!(session.done_count(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_folder_transfer_rebuilds_the_senders_tree() {
        let dir = temp_dir("folder");
        let session = InboundSession::new(
            "Phone".into(),
            vec![
                meta("a", "Zelda/rom.gbc", 4),
                meta("b", "Zelda/saves/rom.sav", 4),
                // Same basename in another folder: no collision suffix.
                meta("c", "Link/saves/rom.sav", 4),
            ],
            &router(&dir),
        )
        .unwrap();
        for id in ["a", "b", "c"] {
            let token = session.tokens()[id].clone();
            assert_eq!(
                session.receive_file(id, &token, &mut &b"data"[..], &NoopWake),
                200
            );
        }
        assert!(dir.join("Zelda/rom.gbc").exists());
        assert!(dir.join("Zelda/saves/rom.sav").exists());
        assert!(dir.join("Link/saves/rom.sav").exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hostile_names_land_inside_save_dir() {
        let dir = temp_dir("hostile");
        let session = InboundSession::new(
            "Phone".into(),
            vec![meta("a", "../../evil.sh", 4), meta("b", "../../evil.sh", 4)],
            &router(&dir),
        )
        .unwrap();
        for (id, expected) in [("a", "evil.sh"), ("b", "evil (1).sh")] {
            let token = session.tokens()[id].clone();
            assert_eq!(
                session.receive_file(id, &token, &mut &b"data"[..], &NoopWake),
                200
            );
            assert!(dir.join(expected).exists(), "{expected}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn overwrite_replaces_the_file_on_disk() {
        let dir = temp_dir("overwrite");
        std::fs::write(dir.join("game.gbc"), b"old").unwrap();
        let router = SaveRouter::into_dir(dir.clone(), true, true);
        let session =
            InboundSession::new("Phone".into(), vec![meta("a", "game.gbc", 3)], &router).unwrap();
        let token = session.tokens()["a"].clone();

        assert_eq!(
            session.receive_file("a", &token, &mut &b"new"[..], &NoopWake),
            200
        );
        assert_eq!(std::fs::read(dir.join("game.gbc")).unwrap(), b"new");
        assert!(!dir.join("game (1).gbc").exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The old file must survive a receive that dies part-way: the rename onto
    /// it only happens once the whole body is on disk.
    #[test]
    fn overwrite_keeps_the_old_file_when_the_transfer_fails() {
        let dir = temp_dir("overwrite-fail");
        std::fs::write(dir.join("game.gbc"), b"old").unwrap();
        let router = SaveRouter::into_dir(dir.clone(), true, true);
        let session =
            InboundSession::new("Phone".into(), vec![meta("a", "game.gbc", 10)], &router).unwrap();
        let token = session.tokens()["a"].clone();

        assert_eq!(
            session.receive_file("a", &token, &mut &b"short"[..], &NoopWake),
            500
        );
        assert_eq!(std::fs::read(dir.join("game.gbc")).unwrap(), b"old");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn routes_files_to_per_extension_folders() {
        let base = temp_dir("route");
        let settings = crate::net::TransferSettings {
            save_dir: base.clone(),
            auto_accept: false,
            overwrite: false,
            auto_routes: false,
            keep_folders: true,
            routes: [("png".to_string(), "shots".to_string())].into(), // relative → base/shots
        };
        let router = SaveRouter::new(base.clone(), &settings);
        let session = InboundSession::new(
            "Phone".into(),
            vec![meta("a", "grab.png", 3), meta("b", "rom.gbc", 3)],
            &router,
        )
        .unwrap();
        for id in ["a", "b"] {
            let token = session.tokens()[id].clone();
            assert_eq!(
                session.receive_file(id, &token, &mut &b"abc"[..], &NoopWake),
                200
            );
        }
        // The routed dir is created on demand; the unmatched extension falls back.
        assert!(base.join("shots").join("grab.png").exists());
        assert!(base.join("rom.gbc").exists());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
