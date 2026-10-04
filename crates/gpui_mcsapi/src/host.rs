use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use egui::{Event, Rect, Sense};

use crate::input::InputTranslator;
use crate::{Bridge, Host, HostEvent};

/// Shows a GPUI app's windows inside mcsapi.
///
/// The first window fills the surface and later ones (settings, prompts, pickers opened as
/// windows) float above it. Pointer input goes to the window under the pointer, or to the one
/// a drag started in; keys go to the window clicked last.
pub struct GpuiSurface {
    bridge: Arc<Bridge>,
    title: String,
    windows: HashMap<u64, SurfaceWindow>,
    focused: Option<u64>,
    pointer_owner: Option<u64>,
    had_windows: bool,
}

#[derive(Default)]
struct SurfaceWindow {
    translator: InputTranslator,
    reported: Option<(egui::Vec2, f32)>,
    hovered: bool,
    open: bool,
}

impl GpuiSurface {
    pub(crate) fn new(bridge: Arc<Bridge>) -> Self {
        Self {
            bridge,
            title: String::new(),
            windows: HashMap::new(),
            focused: None,
            pointer_owner: None,
            had_windows: false,
        }
    }

    /// Whether GPUI closed every window it opened, which is when a standalone host should go.
    pub fn finished(&self) -> bool {
        self.had_windows && self.bridge.windows.lock().is_empty()
    }

    /// The size GPUI asked for its first window, once it has opened one.
    pub fn first_requested_size(&self) -> Option<egui::Vec2> {
        self.bridge
            .windows
            .lock()
            .first()
            .map(|slot| slot.requested_size)
    }

    /// Asks every window to close, as when the surface itself is going away.
    pub fn close_all(&self) {
        for slot in self.bridge.windows.lock().iter() {
            self.bridge
                .send(HostEvent::CloseRequested { window: slot.id });
        }
    }

    /// Paints GPUI's windows into the space `ui` has left and forwards this frame's input.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let slots: Vec<_> = self
            .bridge
            .windows
            .lock()
            .iter()
            .map(|slot| {
                (
                    slot.id,
                    slot.title.clone(),
                    slot.frame.clone(),
                    slot.requested_size,
                )
            })
            .collect();
        self.had_windows |= !slots.is_empty();
        self.windows
            .retain(|id, _| slots.iter().any(|(slot_id, ..)| slot_id == id));
        if let Some((_, title, ..)) = slots.first() {
            self.title = title.clone();
        }

        let mut regions = Vec::with_capacity(slots.len());
        for (index, (id, title, frame, requested_size)) in slots.into_iter().enumerate() {
            let rect = if index == 0 {
                ui.available_rect_before_wrap()
            } else {
                let window = self.windows.entry(id).or_insert_with(|| SurfaceWindow {
                    open: true,
                    ..Default::default()
                });
                let mut open = window.open;
                let response = egui::Window::new(title)
                    .id(egui::Id::new(("gpui window", id)))
                    .open(&mut open)
                    .default_size(requested_size)
                    .resizable(true)
                    .show(ui.ctx(), |ui| {
                        let rect = ui.available_rect_before_wrap();
                        ui.allocate_rect(rect, Sense::hover());
                        rect
                    });
                if !open {
                    self.bridge.send(HostEvent::CloseRequested { window: id });
                }
                match response.and_then(|response| response.inner) {
                    Some(rect) => rect,
                    None => continue,
                }
            };
            regions.push((id, rect, frame));
        }

        let pixels_per_point = ui.ctx().pixels_per_point();
        for (index, (id, rect, frame)) in regions.iter().enumerate() {
            let window = self.windows.entry(*id).or_insert_with(|| SurfaceWindow {
                open: true,
                ..Default::default()
            });
            let report = (rect.size(), pixels_per_point);
            if window.reported != Some(report) {
                window.reported = Some(report);
                self.bridge.send(HostEvent::Resize {
                    window: *id,
                    size: rect.size(),
                    scale_factor: pixels_per_point,
                });
            }
            let Some(frame) = frame else { continue };
            // The first window is the surface's own content; floating ones paint inside their
            // egui window's layer so they stay above it.
            let painter = if index == 0 {
                ui.painter().clone()
            } else {
                ui.ctx().layer_painter(egui::LayerId::new(
                    egui::Order::Middle,
                    egui::Id::new(("gpui window", *id)),
                ))
            };
            for clipped in &frame.shapes {
                let clip = clipped
                    .clip_rect
                    .translate(rect.min.to_vec2())
                    .intersect(*rect);
                if clip.is_positive() {
                    let mut shape = clipped.shape.clone();
                    shape.translate(rect.min.to_vec2());
                    painter.with_clip_rect(clip).add(shape);
                }
            }
        }

        let main = regions.first().map(|(id, rect, _)| (*id, *rect));
        if let Some((_, rect)) = main {
            let response = ui.allocate_rect(rect, Sense::click_and_drag() | Sense::FOCUSABLE);
            if response.clicked() || response.drag_started() {
                response.request_focus();
            }
            // Tab, arrows and Escape belong to GPUI's keymap, not to egui's focus navigation.
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
        }
        self.forward_input(ui, &regions);
    }

    fn forward_input(&mut self, ui: &egui::Ui, regions: &[(u64, Rect, Option<Arc<crate::Frame>>)]) {
        let (events, modifiers, pointer) = ui.input(|input| {
            (
                input.raw.events.clone(),
                input.modifiers,
                input.pointer.latest_pos(),
            )
        });
        // Floating windows are drawn last, so they win where they overlap the main one.
        let under_pointer = pointer.and_then(|pointer| {
            regions
                .iter()
                .rev()
                .find(|(_, rect, _)| rect.contains(pointer))
                .map(|(id, ..)| *id)
        });

        for (id, ..) in regions {
            if let Some(window) = self.windows.get_mut(id) {
                let hovered = under_pointer == Some(*id);
                if window.hovered != hovered {
                    window.hovered = hovered;
                    self.bridge.send(HostEvent::Hovered {
                        window: *id,
                        hovered,
                    });
                }
            }
        }
        if under_pointer.is_some() {
            ui.ctx().set_cursor_icon(*self.bridge.cursor.lock());
        }

        let mut pointer_events: Vec<Event> = Vec::new();
        let mut key_events: Vec<Event> = Vec::new();
        for event in events {
            match &event {
                Event::PointerButton { pressed: true, .. } => {
                    self.pointer_owner = under_pointer;
                    if let Some(id) = under_pointer
                        && self.focused != Some(id)
                    {
                        if let Some(previous) = self.focused {
                            self.bridge.send(HostEvent::Active {
                                window: previous,
                                active: false,
                            });
                        }
                        self.focused = Some(id);
                        self.bridge.send(HostEvent::Active {
                            window: id,
                            active: true,
                        });
                    }
                    pointer_events.push(event);
                }
                Event::PointerMoved(_)
                | Event::PointerButton { .. }
                | Event::PointerGone
                | Event::MouseWheel { .. } => pointer_events.push(event),
                _ => key_events.push(event),
            }
        }

        let pointer_target = self.pointer_owner.or(under_pointer);
        let any_pressed = ui.input(|input| input.pointer.any_down());
        if !any_pressed {
            self.pointer_owner = None;
        }
        if self.focused.is_none() {
            self.focused = regions.first().map(|(id, ..)| *id);
        }

        let bridge = self.bridge.clone();
        let origin_of = |id: u64| {
            regions
                .iter()
                .find(|(region, ..)| *region == id)
                .map(|(_, rect, _)| rect.min)
        };
        if let Some(target) = pointer_target
            && let Some(origin) = origin_of(target)
            && let Some(window) = self.windows.get_mut(&target)
            && !pointer_events.is_empty()
        {
            window
                .translator
                .translate(target, origin, &pointer_events, modifiers, |event| {
                    bridge.send(event)
                });
        }
        if let Some(target) = self.focused
            && let Some(origin) = origin_of(target)
            && let Some(window) = self.windows.get_mut(&target)
        {
            window
                .translator
                .translate(target, origin, &key_events, modifiers, |event| {
                    bridge.send(event)
                });
        }
    }
}

impl mcsapi_ui::App for GpuiSurface {
    fn title(&self) -> &str {
        &self.title
    }

    fn ui(&mut self, ui: &mut egui::Ui, _theme: &mcsapi_ui::Theme) {
        self.show(ui);
    }
}

/// The window [`spawn_host`] opens.
pub struct HostOptions {
    pub title: String,
    /// The Wayland app id and X11 class.
    pub app_id: String,
    pub size: egui::Vec2,
    pub theme: mcsapi_ui::Theme,
}

impl HostOptions {
    /// A window titled and identified as `name`, in mcsapi's default theme.
    pub fn new(name: &str) -> Self {
        Self {
            title: name.to_owned(),
            app_id: name.to_owned(),
            size: egui::vec2(1280.0, 800.0),
            theme: mcsapi_ui::Theme::default(),
        }
    }
}

/// Opens an mcsapi window for GPUI on a thread of its own and returns GPUI's end of it.
///
/// GPUI's event loop keeps the main thread, so the window's event loop is built to accept
/// another one, which X11 and Wayland allow.
pub fn spawn_host(options: HostOptions) -> Result<Host> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("mcsapi host".into())
        .spawn(move || {
            let native_options = eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_title(options.title.clone())
                    .with_app_id(options.app_id.clone())
                    .with_inner_size(options.size),
                event_loop_builder: Some(Box::new(|builder| {
                    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
                    {
                        winit::platform::x11::EventLoopBuilderExtX11::with_any_thread(
                            builder, true,
                        );
                        winit::platform::wayland::EventLoopBuilderExtWayland::with_any_thread(
                            builder, true,
                        );
                    }
                    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
                    let _ = builder;
                })),
                ..Default::default()
            };
            let theme = options.theme;
            let result = eframe::run_native(
                &options.title,
                native_options,
                Box::new(move |creation| {
                    mcsapi_components::Tokens::from_theme(&theme).install(&creation.egui_ctx);
                    let (host, surface) = Host::new(creation.egui_ctx.clone());
                    sender
                        .send(host)
                        .map_err(|_| anyhow::anyhow!("GPUI stopped waiting for its host"))?;
                    Ok(Box::new(Standalone {
                        surface,
                        theme,
                        sized: false,
                    }))
                }),
            );
            if let Err(error) = result {
                log::error!("the mcsapi host window failed: {error}");
            }
        })
        .context("starting the mcsapi host thread")?;
    receiver
        .recv()
        .context("the mcsapi host stopped before its window opened")
}

struct Standalone {
    surface: GpuiSurface,
    theme: mcsapi_ui::Theme,
    /// Whether the window has taken the size GPUI asked for its first window.
    sized: bool,
}

impl eframe::App for Standalone {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui.ctx().input(|input| input.viewport().close_requested()) {
            self.surface.close_all();
        }
        if !self.sized
            && let Some(size) = self.surface.first_requested_size()
        {
            self.sized = true;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        }
        egui::CentralPanel::no_frame()
            .frame(egui::Frame::new().fill(self.theme.background))
            .show(ui, |ui| self.surface.show(ui));
        if self.surface.finished() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
