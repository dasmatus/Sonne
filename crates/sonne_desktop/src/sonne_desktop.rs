//! Sonne: a desktop for building software with Claude on LosOS.
//!
//! The window is an mcsapi app ([`Sonne`]), drawn with mcsapi's components in
//! Sonne's rising-sun palette ([`sonne_theme`]), and can run inside derisk's
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

/// Sonne's rising-sun palette: a night sky turning to dawn, with the sun's
/// orange as the accent. The editor's "Sonne Dawn" theme uses the same colours.
///
/// Sonne used to take the desktop theme from derisk's
/// `$XDG_RUNTIME_DIR/derisk/theme.json`; it now keeps its own palette in every
/// session, since the palette is part of what Sonne is.
pub fn sonne_theme() -> Theme {
    Theme {
        background: Color32::from_rgb(0x1a, 0x14, 0x30),
        surface: Color32::from_rgb(0x25, 0x1c, 0x40),
        foreground: Color32::from_rgb(0xfb, 0xe9, 0xd7),
        border: Color32::from_rgb(0x3d, 0x2f, 0x5c),
        accent: Color32::from_rgb(0xff, 0x8c, 0x42),
    }
}
