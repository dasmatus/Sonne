//! Sonne: a desktop for building software with Claude on LosOS.
//!
//! The window is an mcsapi app ([`Sonne`]), drawn with mcsapi's components so
//! it looks like the rest of a derisk desktop and can run inside derisk's
//! compositor as well as in a window of its own. The left column holds
//! projects, chats, routines and artifacts; the chat runs Claude Code with
//! Sonne's MCP server; the right side shows the app the agent is building, live,
//! next to the forge's pull or merge requests and the project's MCP servers.

pub mod agent;
mod code_view;
pub mod forge;
pub mod mcp;
pub mod routines;
pub mod setup;
pub mod store;
pub mod tools;
mod ui;

pub use ui::Sonne;

use mcsapi_ui::{Theme, egui::Color32};

/// The theme derisk publishes to `$XDG_RUNTIME_DIR/derisk/theme.json`, so a
/// Sonne window outside derisk's compositor still matches the desktop.
pub fn derisk_theme() -> Option<Theme> {
    let path = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?)
        .join("derisk")
        .join("theme.json");
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let color = |name: &str| parse_hex(json["palette"][name].as_str()?);
    Some(Theme {
        background: color("background")?,
        surface: color("surface")?,
        foreground: color("foreground")?,
        border: color("border")?,
        accent: color("accent")?,
    })
}

fn parse_hex(text: &str) -> Option<Color32> {
    let hex = text.strip_prefix('#')?;
    let byte = |index: usize| u8::from_str_radix(hex.get(index..index + 2)?, 16).ok();
    match hex.len() {
        6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(
            byte(0)?,
            byte(2)?,
            byte(4)?,
            byte(6)?,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_colors_parse_from_hex() {
        assert_eq!(parse_hex("#ff8000"), Some(Color32::from_rgb(255, 128, 0)));
        assert_eq!(
            parse_hex("#00000080"),
            Some(Color32::from_rgba_unmultiplied(0, 0, 0, 128))
        );
        assert_eq!(parse_hex("ff8000"), None);
        assert_eq!(parse_hex("#ff80"), None);
    }
}
