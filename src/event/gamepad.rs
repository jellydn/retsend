//! Controller input → [`AppCommand`]s. A trimmed-down take on retsurf's
//! gesture resolver: taps dispatch on press, held directions (D-pad or left
//! stick past the dead zone) auto-repeat navigation, and a held Select turns
//! Y into the quit chord — the same one the keyboard path offers.

use crate::app::{AppCommand, Direction};
use crate::config::InputConfig;
use sdl2::controller::{Axis, Button};
use std::time::{Duration, Instant};

pub struct Gamepad {
    config: InputConfig,
    /// Currently held direction (last one pressed wins) and its repeat clock.
    held: Option<Held>,
    /// Left-stick state folded into digital directions with hysteresis.
    stick: (f32, f32),
    stick_dir: Option<Direction>,
    /// Select is held (the Select+Y chord's anchor).
    select_held: bool,
}

struct Held {
    dir: Direction,
    pressed_at: Instant,
    last_repeat: Option<Instant>,
}

impl Gamepad {
    pub fn new(config: InputConfig) -> Self {
        Self {
            config,
            held: None,
            stick: (0.0, 0.0),
            stick_dir: None,
            select_held: false,
        }
    }

    /// Time until the next auto-repeat is due (zero when overdue), `None`
    /// when nothing is held — the event loop must not block past it.
    pub fn next_repeat_in(&self) -> Option<Duration> {
        let held = self.held.as_ref()?;
        let due = match held.last_repeat {
            None => held.pressed_at + Duration::from_millis(self.config.repeat_initial_delay_ms),
            Some(last) => last + Duration::from_millis(self.config.repeat_interval_ms),
        };
        Some(due.saturating_duration_since(Instant::now()))
    }

    pub fn on_button(&mut self, button: Button, pressed: bool, commands: &mut Vec<AppCommand>) {
        if let Some(dir) = dpad_dir(button) {
            if pressed {
                commands.push(AppCommand::Nav(dir));
                self.hold(dir);
            } else if self.held.as_ref().is_some_and(|h| h.dir == dir) {
                self.held = None;
            }
            return;
        }
        // Select anchors the quit chord, so its releases matter too: held, it
        // turns Y into the app's way out; alone it stays the radar's refresh.
        if button == Button::Back {
            self.select_held = pressed;
            if pressed {
                commands.push(AppCommand::ReAnnounce);
            }
            return;
        }
        if !pressed {
            return;
        }
        match button {
            Button::A => commands.push(AppCommand::Confirm),
            Button::B => commands.push(AppCommand::Back),
            Button::X => commands.push(AppCommand::Alt),
            Button::Y if self.select_held => commands.push(AppCommand::Shutdown),
            Button::Y => commands.push(AppCommand::TogglePin),
            Button::Start => commands.push(AppCommand::Start),
            Button::LeftShoulder => commands.push(AppCommand::PageUp),
            Button::RightShoulder => commands.push(AppCommand::PageDown),
            _ => {}
        }
    }

    pub fn on_axis(&mut self, axis: Axis, value: i16, commands: &mut Vec<AppCommand>) {
        let v = value as f32 / i16::MAX as f32;
        match axis {
            Axis::LeftX => self.stick.0 = v,
            Axis::LeftY => self.stick.1 = v,
            _ => return,
        }
        // Digitalize with hysteresis: engage past the dead zone, release only
        // when well back inside it, so a wobbling stick doesn't chatter.
        let engage = self.config.deadzone;
        let release = engage * 0.6;
        let (x, y) = self.stick;
        let dir = if x.abs().max(y.abs()) >= engage {
            Some(if x.abs() > y.abs() {
                if x > 0.0 {
                    Direction::Right
                } else {
                    Direction::Left
                }
            } else if y > 0.0 {
                Direction::Down // SDL Y axis is positive downward
            } else {
                Direction::Up
            })
        } else if x.abs().max(y.abs()) <= release {
            None
        } else {
            self.stick_dir // in the hysteresis band: keep the current state
        };

        if dir != self.stick_dir {
            if let Some(old) = self.stick_dir {
                if self.held.as_ref().is_some_and(|h| h.dir == old) {
                    self.held = None;
                }
            }
            if let Some(d) = dir {
                commands.push(AppCommand::Nav(d));
                self.hold(d);
            }
            self.stick_dir = dir;
        }
    }

    /// Fire navigation repeats for the held direction. Called once per frame.
    pub fn tick(&mut self, commands: &mut Vec<AppCommand>) {
        let Some(held) = &mut self.held else { return };
        let now = Instant::now();
        let initial = Duration::from_millis(self.config.repeat_initial_delay_ms);
        let interval = Duration::from_millis(self.config.repeat_interval_ms);
        let due = match held.last_repeat {
            None => now.duration_since(held.pressed_at) >= initial,
            Some(last) => now.duration_since(last) >= interval,
        };
        if due {
            commands.push(AppCommand::Nav(held.dir));
            held.last_repeat = Some(now);
        }
    }

    fn hold(&mut self, dir: Direction) {
        self.held = Some(Held {
            dir,
            pressed_at: Instant::now(),
            last_repeat: None,
        });
    }
}

fn dpad_dir(button: Button) -> Option<Direction> {
    match button {
        Button::DPadUp => Some(Direction::Up),
        Button::DPadDown => Some(Direction::Down),
        Button::DPadLeft => Some(Direction::Left),
        Button::DPadRight => Some(Direction::Right),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_held_turns_y_into_the_quit_chord() {
        let mut pad = Gamepad::new(InputConfig::default());
        let mut commands = Vec::new();
        pad.on_button(Button::Back, true, &mut commands);
        pad.on_button(Button::Y, true, &mut commands);
        assert_eq!(commands, vec![AppCommand::ReAnnounce, AppCommand::Shutdown]);
    }

    #[test]
    fn releasing_select_hands_y_back_to_pinning() {
        let mut pad = Gamepad::new(InputConfig::default());
        let mut commands = Vec::new();
        pad.on_button(Button::Back, true, &mut commands);
        pad.on_button(Button::Back, false, &mut commands);
        pad.on_button(Button::Y, true, &mut commands);
        assert_eq!(
            commands,
            vec![AppCommand::ReAnnounce, AppCommand::TogglePin]
        );
    }

    /// Y pressed first keeps its own action; the later Select stays a refresh.
    #[test]
    fn y_first_then_select_does_not_quit() {
        let mut pad = Gamepad::new(InputConfig::default());
        let mut commands = Vec::new();
        pad.on_button(Button::Y, true, &mut commands);
        pad.on_button(Button::Back, true, &mut commands);
        assert_eq!(
            commands,
            vec![AppCommand::TogglePin, AppCommand::ReAnnounce]
        );
    }
}
