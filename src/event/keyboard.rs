//! Keyboard controls: the desktop-dev mirror of the gamepad, plus the layout
//! handhelds send when their pad arrives as key presses.

use crate::app::{AppCommand, Direction};
use sdl2::keyboard::Keycode;

/// Which layout the keys arriving from the device follow.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum Keymap {
    #[default]
    Desktop,
    /// Its SDL2 offers the pad as a joystick with no gamepad mapping, and sends
    /// keys instead. See [`miyoo_mini`]. `menu_quits` is the launcher saying it
    /// keeps no kill helper of its own, which leaves MENU to the app.
    MiyooMini { menu_quits: bool },
}

impl Keymap {
    /// `RETSEND_KEYMAP=miyoo|desktop` wins; else the driver name gives it away.
    pub fn detect(video_driver: &str) -> Self {
        let miyoo = Self::MiyooMini {
            menu_quits: std::env::var_os("RETSEND_MENU_QUIT").is_some_and(|v| v != "0"),
        };
        match std::env::var("RETSEND_KEYMAP").as_deref() {
            Ok("miyoo") => miyoo,
            Ok("desktop") => Self::Desktop,
            Ok(other) => {
                log::warn!("unknown RETSEND_KEYMAP `{other}`; using the desktop layout");
                Self::Desktop
            }
            Err(_) if video_driver == "mmiyoo" => miyoo,
            Err(_) => Self::Desktop,
        }
    }
}

/// Chord state for pads that arrive as key presses. Select is the anchor:
/// holding it turns the next Y press into the app's quit — the same chord the
/// gamepad path offers — and its release hands Y back to pinning.
#[derive(Default)]
pub struct Chord {
    select_held: bool,
}

/// Whether this key is the layout's Select (the chord's anchor).
fn is_select(keymap: Keymap, kc: Keycode) -> bool {
    match keymap {
        Keymap::Desktop => matches!(kc, Keycode::Tab | Keycode::F5),
        Keymap::MiyooMini { .. } => kc == Keycode::RCtrl,
    }
}

/// Whether this key is the layout's Y (the chord's trigger).
fn is_y(keymap: Keymap, kc: Keycode) -> bool {
    match keymap {
        Keymap::Desktop => kc == Keycode::Y,
        Keymap::MiyooMini { .. } => kc == Keycode::LAlt,
    }
}

pub fn on_key_down(
    keymap: Keymap,
    kc: Keycode,
    repeat: bool,
    chord: &mut Chord,
    commands: &mut Vec<AppCommand>,
) {
    if is_select(keymap, kc) {
        chord.select_held = true;
    }
    let cmd = match kc {
        Keycode::Up => AppCommand::Nav(Direction::Up),
        Keycode::Down => AppCommand::Nav(Direction::Down),
        Keycode::Left => AppCommand::Nav(Direction::Left),
        Keycode::Right => AppCommand::Nav(Direction::Right),
        Keycode::PageUp => AppCommand::PageUp,
        Keycode::PageDown => AppCommand::PageDown,
        // OS key repeat only drives navigation; a held Enter must not
        // re-confirm and a held Esc must not unwind several screens.
        _ if repeat => return,
        _ => {
            // Select+Y quits — the way out that no single button names. Y's own
            // action is suppressed for this press; key repeat cannot re-fire
            // it, and a held Y pressed into a later Select does nothing (the
            // chord fires on Y's press, like the gamepad's).
            if chord.select_held && is_y(keymap, kc) {
                commands.push(AppCommand::Shutdown);
                return;
            }
            match keymap {
                Keymap::Desktop => match desktop(kc) {
                    Some(cmd) => cmd,
                    None => return,
                },
                Keymap::MiyooMini { menu_quits } => match miyoo_mini(kc, menu_quits) {
                    Some(cmd) => cmd,
                    None => return,
                },
            }
        }
    };
    commands.push(cmd);
}

/// Key releases carry no action of their own; they only end a chord.
pub fn on_key_up(keymap: Keymap, kc: Keycode, chord: &mut Chord) {
    if is_select(keymap, kc) {
        chord.select_held = false;
    }
}

fn desktop(kc: Keycode) -> Option<AppCommand> {
    Some(match kc {
        Keycode::Return | Keycode::KpEnter => AppCommand::Confirm,
        // AcBack is Android's hardware/gesture Back, trapped into a key event by
        // the hint `run_app` sets there.
        Keycode::Escape | Keycode::AcBack => AppCommand::Back,
        // Both the pad's label and the key a desktop hand reaches for.
        Keycode::X | Keycode::Backspace => AppCommand::Alt,
        Keycode::F1 => AppCommand::Start,
        Keycode::Tab | Keycode::F5 => AppCommand::ReAnnounce,
        Keycode::Y => AppCommand::TogglePin,
        _ => return None,
    })
}

/// The pad, as keys. MENU belongs to the launcher — OnionOS gives it to the
/// system's own kill helper, as every app there does — and is the app's to
/// answer only where the launcher keeps none (Allium).
fn miyoo_mini(kc: Keycode, menu_quits: bool) -> Option<AppCommand> {
    Some(match kc {
        // MENU, where the launcher has handed it over.
        Keycode::Escape if menu_quits => AppCommand::Shutdown,
        Keycode::Space => AppCommand::Confirm,    // A
        Keycode::LCtrl => AppCommand::Back,       // B
        Keycode::LShift => AppCommand::Alt,       // X
        Keycode::LAlt => AppCommand::TogglePin,   // Y
        Keycode::Return => AppCommand::Start,     // Start
        Keycode::RCtrl => AppCommand::ReAnnounce, // Select
        Keycode::E => AppCommand::PageUp,         // L1
        Keycode::T => AppCommand::PageDown,       // R1
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x_and_backspace_erase_while_escape_backs_out() {
        let mut chord = Chord::default();
        let mut commands = Vec::new();
        on_key_down(Keymap::Desktop, Keycode::X, false, &mut chord, &mut commands);
        on_key_down(Keymap::Desktop, Keycode::Backspace, false, &mut chord, &mut commands);
        on_key_down(Keymap::Desktop, Keycode::Escape, false, &mut chord, &mut commands);
        assert_eq!(
            commands,
            vec![AppCommand::Alt, AppCommand::Alt, AppCommand::Back]
        );
    }

    #[test]
    fn y_pins_and_does_not_repeat() {
        let mut chord = Chord::default();
        let mut commands = Vec::new();
        on_key_down(Keymap::Desktop, Keycode::Y, false, &mut chord, &mut commands);
        assert_eq!(commands, vec![AppCommand::TogglePin]);

        // Holding it must not toggle over and over.
        commands.clear();
        on_key_down(Keymap::Desktop, Keycode::Y, true, &mut chord, &mut commands);
        assert!(commands.is_empty());
    }

    #[test]
    fn select_y_quits_on_either_layout() {
        // Desktop: Select is Tab (or F5), Y is Y.
        let mut chord = Chord::default();
        let mut commands = Vec::new();
        on_key_down(Keymap::Desktop, Keycode::Tab, false, &mut chord, &mut commands);
        on_key_down(Keymap::Desktop, Keycode::Y, false, &mut chord, &mut commands);
        assert_eq!(commands, vec![AppCommand::ReAnnounce, AppCommand::Shutdown]);

        // The Miyoo pad: Select is RCtrl, Y is LAlt.
        let miyoo = Keymap::MiyooMini { menu_quits: false };
        let mut chord = Chord::default();
        let mut commands = Vec::new();
        on_key_down(miyoo, Keycode::RCtrl, false, &mut chord, &mut commands);
        on_key_down(miyoo, Keycode::LAlt, false, &mut chord, &mut commands);
        assert_eq!(commands, vec![AppCommand::ReAnnounce, AppCommand::Shutdown]);
    }

    #[test]
    fn releasing_select_hands_y_back_to_pinning() {
        let mut chord = Chord::default();
        let mut commands = Vec::new();
        on_key_down(Keymap::Desktop, Keycode::F5, false, &mut chord, &mut commands);
        on_key_up(Keymap::Desktop, Keycode::F5, &mut chord);
        on_key_down(Keymap::Desktop, Keycode::Y, false, &mut chord, &mut commands);
        assert_eq!(commands, vec![AppCommand::ReAnnounce, AppCommand::TogglePin]);
    }

    /// The chord fires on Y's press: Y held first keeps pinning, and a later
    /// Select press stays a plain refresh.
    #[test]
    fn y_first_then_select_does_not_quit() {
        let mut chord = Chord::default();
        let mut commands = Vec::new();
        on_key_down(Keymap::Desktop, Keycode::Y, false, &mut chord, &mut commands);
        on_key_down(Keymap::Desktop, Keycode::Tab, false, &mut chord, &mut commands);
        on_key_up(Keymap::Desktop, Keycode::Tab, &mut chord);
        assert_eq!(commands, vec![AppCommand::TogglePin, AppCommand::ReAnnounce]);
    }

    #[test]
    fn the_miyoo_pad_maps_a_to_confirm_and_start_to_start() {
        let miyoo = Keymap::MiyooMini { menu_quits: false };
        let mut chord = Chord::default();
        let mut commands = Vec::new();
        on_key_down(miyoo, Keycode::Space, false, &mut chord, &mut commands);
        on_key_down(miyoo, Keycode::LCtrl, false, &mut chord, &mut commands);
        on_key_down(miyoo, Keycode::Return, false, &mut chord, &mut commands);
        assert_eq!(
            commands,
            vec![AppCommand::Confirm, AppCommand::Back, AppCommand::Start]
        );
    }

    /// MENU is the launcher's key until a launcher says otherwise, and it is
    /// never the desktop's Escape, which backs out of a screen.
    #[test]
    fn menu_quits_only_where_the_launcher_hands_it_over() {
        let mut chord = Chord::default();
        let mut commands = Vec::new();
        on_key_down(
            Keymap::MiyooMini { menu_quits: false },
            Keycode::Escape,
            false,
            &mut chord,
            &mut commands,
        );
        assert!(commands.is_empty());

        on_key_down(
            Keymap::MiyooMini { menu_quits: true },
            Keycode::Escape,
            false,
            &mut chord,
            &mut commands,
        );
        assert_eq!(commands, vec![AppCommand::Shutdown]);
    }

    #[test]
    fn arrows_navigate_under_either_layout() {
        for keymap in [Keymap::Desktop, Keymap::MiyooMini { menu_quits: true }] {
            let mut chord = Chord::default();
            let mut commands = Vec::new();
            on_key_down(keymap, Keycode::Up, false, &mut chord, &mut commands);
            assert_eq!(commands, vec![AppCommand::Nav(Direction::Up)]);
        }
    }
}
