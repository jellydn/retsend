//! LocalSend v2.1 wire types (<https://github.com/localsend/protocol>) and the
//! random-token helper. Pure data: everything else in `net` consumes this.
//!
//! One flattened [`DeviceInfo`] with optionals serves every message that
//! carries device identity — the multicast announce, the `/register` body and
//! response, and prepare-upload's `info` are all subsets of it. Input parsing
//! is deliberately lax (real-world LocalSend forks vary); we always emit the
//! full v2.1 shape.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROTOCOL_VERSION: &str = "2.1";
pub const API_PREFIX: &str = "/api/localsend/v2";
pub const MULTICAST_GROUP: std::net::Ipv4Addr = std::net::Ipv4Addr::new(224, 0, 0, 167);
/// The protocol-fixed UDP discovery port. The TCP port is ours to choose (the
/// announce carries it); this one every implementation must share.
pub const MULTICAST_PORT: u16 = 53317;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub alias: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_type: Option<String>,
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// "http" | "https"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download: Option<bool>,
    /// Multicast only: `true` asks receivers to respond (via `/register` or a
    /// multicast reply with `announce: false`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub announce: Option<bool>,
}

/// Longest peer-supplied name we keep. A device name is a few words; past this
/// it is a peer costing us a text layout per frame (64 KB measured at 85 ms).
const MAX_PEER_NAME_CHARS: usize = 48;
/// Stands in for a name that arrives empty, so a row is never blank.
const UNNAMED: &str = "unknown";

/// A peer-supplied name fit to store and to draw: control characters — which
/// would otherwise let a sender add lines to our own dialogs — become spaces,
/// and the length is capped.
pub fn clamp_peer_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_PEER_NAME_CHARS)
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return UNNAMED.to_string();
    }
    trimmed.to_string()
}

impl DeviceInfo {
    /// The identity with the names a peer chose clamped — apply once, where a
    /// peer's info enters (the registry, prepare-upload), so no screen has to.
    pub fn clamped(mut self) -> Self {
        self.alias = clamp_peer_name(&self.alias);
        self.device_model = self.device_model.as_deref().map(clamp_peer_name);
        self
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileMeta {
    pub id: String,
    pub file_name: String,
    pub size: u64,
    pub file_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PrepareUploadRequest {
    pub info: DeviceInfo,
    /// Keyed by file id.
    pub files: BTreeMap<String, FileMeta>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PrepareUploadResponse {
    pub session_id: String,
    /// fileId -> upload token.
    pub files: BTreeMap<String, String>,
}

/// `n_bytes` of OS randomness as lowercase hex. Session ids and per-file
/// tokens use 16 bytes; the per-run fingerprint (HTTP mode: just a random
/// string for self-ignore) uses 32.
pub fn random_token(n_bytes: usize) -> String {
    let mut buf = vec![0u8; n_bytes];
    getrandom::fill(&mut buf).expect("OS randomness unavailable");
    let mut out = String::with_capacity(n_bytes * 2);
    for b in buf {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An announce as the official app (2.x, HTTPS mode) multicasts it.
    const OFFICIAL_ANNOUNCE: &str = r#"{
        "alias": "Nice Orange",
        "version": "2.1",
        "deviceModel": "Samsung",
        "deviceType": "mobile",
        "fingerprint": "e0accd3aax9",
        "port": 53317,
        "protocol": "https",
        "download": true,
        "announce": true
    }"#;

    #[test]
    fn parses_official_announce() {
        let d: DeviceInfo = serde_json::from_str(OFFICIAL_ANNOUNCE).unwrap();
        assert_eq!(d.alias, "Nice Orange");
        assert_eq!(d.version, "2.1");
        assert_eq!(d.device_type.as_deref(), Some("mobile"));
        assert_eq!(d.port, Some(53317));
        assert_eq!(d.protocol.as_deref(), Some("https"));
        assert_eq!(d.announce, Some(true));
    }

    #[test]
    fn a_peer_name_keeps_what_a_device_name_needs() {
        assert_eq!(clamp_peer_name("Nice Orange"), "Nice Orange");
        // Emoji and non-latin names are names too.
        assert_eq!(clamp_peer_name("Максим's Pixel 📱"), "Максим's Pixel 📱");
        assert_eq!(clamp_peer_name("  padded  "), "padded");
    }

    /// The flood and the injection: a name is capped, and control characters
    /// cannot add lines to a dialog that quotes it.
    #[test]
    fn a_peer_name_is_capped_and_stripped_of_control_characters() {
        let flood = clamp_peer_name(&"A".repeat(64 * 1024));
        assert_eq!(flood.chars().count(), MAX_PEER_NAME_CHARS);

        let injected = clamp_peer_name("Phone\n\nAccept to continue");
        assert!(!injected.contains('\n'), "{injected}");
        assert_eq!(injected, "Phone  Accept to continue");

        // A name that is nothing but control characters still leaves a row.
        assert_eq!(clamp_peer_name("\n\t\0"), UNNAMED);
        assert_eq!(clamp_peer_name(""), UNNAMED);
    }

    #[test]
    fn clamping_covers_every_name_a_peer_chooses() {
        let hostile = DeviceInfo {
            alias: "\n".to_string() + &"A".repeat(1000),
            device_model: Some("M".repeat(1000)),
            ..serde_json::from_str::<DeviceInfo>(OFFICIAL_ANNOUNCE).unwrap()
        }
        .clamped();
        assert_eq!(hostile.alias.chars().count(), MAX_PEER_NAME_CHARS - 1);
        assert_eq!(
            hostile.device_model.as_deref().map(str::len),
            Some(MAX_PEER_NAME_CHARS)
        );
    }

    /// Minimal message: only the required fields. Forks omit the rest.
    #[test]
    fn parses_minimal_device_info() {
        let d: DeviceInfo =
            serde_json::from_str(r#"{"alias":"A","version":"2.0","fingerprint":"f"}"#).unwrap();
        assert_eq!(d.alias, "A");
        assert_eq!(d.port, None);
        assert_eq!(d.announce, None);
    }

    /// Unknown extra fields must not fail parsing (forward compatibility).
    #[test]
    fn ignores_unknown_fields() {
        let d: DeviceInfo = serde_json::from_str(
            r#"{"alias":"A","version":"2.1","fingerprint":"f","futureField":42}"#,
        )
        .unwrap();
        assert_eq!(d.alias, "A");
    }

    #[test]
    fn device_info_round_trips() {
        let me = DeviceInfo {
            alias: "retsend".into(),
            version: PROTOCOL_VERSION.into(),
            device_model: Some("Retro Handheld".into()),
            device_type: Some("desktop".into()),
            fingerprint: random_token(32),
            port: Some(53317),
            protocol: Some("http".into()),
            download: Some(false),
            announce: Some(true),
        };
        let json = serde_json::to_string(&me).unwrap();
        let back: DeviceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(me, back);
        // Wire names are camelCase, not snake_case.
        assert!(json.contains("\"deviceModel\""));
        assert!(json.contains("\"deviceType\""));
    }

    #[test]
    fn prepare_upload_round_trips() {
        let req = PrepareUploadRequest {
            info: serde_json::from_str(OFFICIAL_ANNOUNCE).unwrap(),
            files: BTreeMap::from([(
                "abc".to_string(),
                FileMeta {
                    id: "abc".into(),
                    file_name: "game.gbc".into(),
                    size: 1024,
                    file_type: "application/octet-stream".into(),
                    sha256: None,
                    preview: None,
                    metadata: None,
                },
            )]),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"fileName\":\"game.gbc\""));
        let back: PrepareUploadRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.files["abc"].file_name, "game.gbc");

        let resp = PrepareUploadResponse {
            session_id: random_token(16),
            files: BTreeMap::from([("abc".to_string(), random_token(16))]),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"sessionId\""));
    }

    #[test]
    fn random_tokens_are_unique_hex() {
        let a = random_token(16);
        let b = random_token(16);
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
