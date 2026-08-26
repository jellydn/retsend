//! Settings screen renderer: a scrolling list of name/value rows, the current
//! row's action in the footer.

use super::theme;
use crate::app::{AppCommand, Direction};
use crate::config::AppConfig;
use crate::overlay::settings::Settings;
use egui_sdl2::egui;

pub fn render(
    root: &mut egui::Ui,
    state: &Settings,
    config: &AppConfig,
    actual_port: u16,
    taps: &mut Vec<AppCommand>,
) {
    // Order matches `crate::overlay::settings::ROWS`; third field is the A verb,
    // `None` on the rows ◂ ▸ walk instead.
    let rows: [(&str, String, Option<&str>); crate::overlay::settings::ROW_COUNT] = [
        ("Device name", config.device.alias.clone(), Some("Edit")),
        (
            "Save folder",
            config.transfer.save_dir.clone(),
            Some("Choose"),
        ),
        (
            "Quick save",
            if config.transfer.auto_accept {
                "on — accept without asking".into()
            } else {
                "off".into()
            },
            Some("Toggle"),
        ),
        (
            "If a file exists",
            if config.transfer.overwrite {
                "replace".into()
            } else {
                "keep both — save as name (1)".into()
            },
            Some("Toggle"),
        ),
        (
            "Received folders",
            if config.transfer.keep_folders {
                "keep the sender's structure".into()
            } else {
                "flatten into the save routes".into()
            },
            Some("Toggle"),
        ),
        (
            "Auto save routes",
            auto_routes_value(config.transfer.auto_routes, state.auto_route_count),
            Some("Toggle"),
        ),
        (
            "Save routes",
            match config.transfer.routes.len() {
                0 => "none".into(),
                1 => "1 extension".into(),
                n => format!("{n} extensions"),
            },
            Some("Edit"),
        ),
        (
            "Port",
            port_label(config.network.port, actual_port, state.port_dirty),
            Some("Edit"),
        ),
        ("UI scale", scale_label(config.display.scale), None),
        (
            "About",
            format!("retsend {}", env!("CARGO_PKG_VERSION")),
            Some("Open"),
        ),
    ];

    // No title panel: the tab bar already shows "⚙ Settings" as the active
    // tab, and an extra top panel here (absent on Send/Receive) would shift
    // egui's panel-id sequence and flag the footer as a changed id.
    egui::Panel::bottom(super::BOTTOM_PANEL_ID).show(root, |ui| {
        ui.add_space(4.0);
        // The row's action, where every other screen puts its buttons.
        match rows[state.cursor.min(rows.len() - 1)].2 {
            Some(action) => super::home::hint_bar(
                ui,
                &[
                    ("← →", "Tabs", None),
                    ("A", action, Some(AppCommand::Confirm)),
                ],
                taps,
            ),
            // A stepper row spends ◂ ▸ on its own value, so it names them —
            // and naming them is what lets a screen with no pad reach them.
            None => super::home::hint_bar(
                ui,
                &[
                    ("◂", "Smaller", Some(AppCommand::Nav(Direction::Left))),
                    ("▸", "Bigger", Some(AppCommand::Nav(Direction::Right))),
                    ("L1 R1", "Tabs", None),
                ],
                taps,
            ),
        }
        ui.add_space(4.0);
    });

    egui::CentralPanel::default().show(root, |ui| {
        // Scrolled like every other list: the rows outgrow a 640x480 screen,
        // and a shorter one (or a larger RETSEND_SCALE) cuts them sooner.
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, (name, value, _)) in rows.iter().enumerate() {
                let selected = state.cursor == i;
                let desired = egui::vec2(ui.available_width(), theme::ROW_HEIGHT);
                let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
                if response.clicked() {
                    taps.push(AppCommand::PickRow(i));
                    taps.push(AppCommand::Confirm);
                }
                if selected {
                    response.scroll_to_me(None);
                    ui.painter().rect(
                        rect,
                        6.0,
                        theme::ACCENT.linear_multiply(0.30),
                        egui::Stroke::new(1.0, theme::ACCENT),
                        egui::StrokeKind::Inside,
                    );
                }
                let painter = ui.painter();
                let padding = 10.0;
                painter.text(
                    rect.left_center() + egui::vec2(padding, 0.0),
                    egui::Align2::LEFT_CENTER,
                    *name,
                    egui::FontId::proportional(theme::ROW_FONT),
                    ui.visuals().text_color(),
                );
                painter.text(
                    rect.right_center() - egui::vec2(padding, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    value,
                    egui::FontId::proportional(theme::DETAIL_FONT),
                    theme::DIM,
                );
            }
        });
    });
}

fn port_label(configured: u16, actual: u16, dirty: bool) -> String {
    if dirty {
        format!("{configured} (applies on close)")
    } else if configured == actual {
        actual.to_string()
    } else {
        format!("{actual} ({configured} was busy)")
    }
}

/// The factor as a percentage of the fit the panel itself asked for. 100% is
/// that fit, which the row spells out — a bare number says nothing about what
/// it is a percentage of.
fn scale_label(scale: f32) -> String {
    let percent = (scale * 100.0).round() as i32;
    if percent == 100 {
        "100% — fits this screen".to_string()
    } else {
        format!("{percent}%")
    }
}

/// "on" with nothing detected has to say so: that is the desktop case, where the
/// setting is live but no console folder exists to route into.
fn auto_routes_value(on: bool, folders: usize) -> String {
    match (on, folders) {
        (false, _) => "off".to_string(),
        (true, 0) => "on — no console folders found".to_string(),
        (true, n) => format!("on — save ROMs into {n} console folders"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_routes_value_spells_out_each_state() {
        assert_eq!(auto_routes_value(false, 12), "off");
        assert_eq!(auto_routes_value(true, 0), "on — no console folders found");
        assert_eq!(
            auto_routes_value(true, 12),
            "on — save ROMs into 12 console folders"
        );
    }
}
