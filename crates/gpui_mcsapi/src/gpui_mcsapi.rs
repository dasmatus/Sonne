//! Runs GPUI apps on mcsapi.
//!
//! [`McsapiPlatform`] wraps a headless GPUI platform, which keeps providing the executors, the
//! text system and everything else that is not a window. Its windows hand each finished scene
//! to an mcsapi surface ([`GpuiSurface`]), which paints it with egui and sends input back. So
//! every crate written against GPUI keeps its code and draws through mcsapi.
//!
//! The surface is an [`mcsapi_ui::App`]: it runs wherever mcsapi apps run. [`spawn_host`]
//! gives it a window of its own on a second thread, since GPUI keeps the main one.

mod atlas;
mod host;
mod input;
mod platform;
mod scene;
mod window;

use std::sync::Arc;

use egui::epaint::ClippedShape;
use parking_lot::Mutex;

pub use host::{GpuiSurface, HostOptions, spawn_host};
pub use platform::McsapiPlatform;

/// What one side of the bridge tells the other. GPUI's side runs on its own thread with `Rc`
/// state, so all of it crosses as plain data.
pub(crate) struct Bridge {
    pub context: egui::Context,
    pub windows: Mutex<Vec<WindowSlot>>,
    pub cursor: Mutex<egui::CursorIcon>,
    events: futures::channel::mpsc::UnboundedSender<HostEvent>,
}

impl Bridge {
    pub(crate) fn send(&self, event: HostEvent) {
        if self.events.unbounded_send(event).is_err() {
            log::debug!("GPUI stopped listening; dropping a host event");
        }
    }
}

/// A GPUI window as the host sees it.
pub(crate) struct WindowSlot {
    pub id: u64,
    pub title: String,
    /// The latest frame GPUI drew, in window points.
    pub frame: Option<Arc<Frame>>,
    /// The size GPUI asked for, used until the host lays the window out.
    pub requested_size: egui::Vec2,
}

pub(crate) struct Frame {
    pub shapes: Vec<ClippedShape>,
}

/// Something that happened on the host, for GPUI's thread.
pub(crate) enum HostEvent {
    Input { window: u64, input: gpui::PlatformInput },
    /// Text that no key event carried, such as a committed IME composition.
    Text { window: u64, text: String },
    Resize { window: u64, size: egui::Vec2, scale_factor: f32 },
    Active { window: u64, active: bool },
    Hovered { window: u64, hovered: bool },
    CloseRequested { window: u64 },
    /// The host's clipboard, read when the user pasted.
    Clipboard(String),
}

/// The GPUI side of a running host: hand it to [`McsapiPlatform::new`].
pub struct Host {
    bridge: Arc<Bridge>,
    events: futures::channel::mpsc::UnboundedReceiver<HostEvent>,
}

impl Host {
    /// Connects GPUI to a surface the caller drives, for example inside a compositor.
    pub fn new(context: egui::Context) -> (Self, GpuiSurface) {
        let (sender, receiver) = futures::channel::mpsc::unbounded();
        let bridge = Arc::new(Bridge {
            context,
            windows: Mutex::new(Vec::new()),
            cursor: Mutex::new(egui::CursorIcon::Default),
            events: sender,
        });
        (
            Self {
                bridge: bridge.clone(),
                events: receiver,
            },
            GpuiSurface::new(bridge),
        )
    }
}
