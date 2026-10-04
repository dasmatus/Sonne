use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures::StreamExt as _;
use gpui::{
    Action, ActivityGuard, AnyWindowHandle, BackgroundExecutor, Bounds, ClipboardItem,
    CursorStyle, DisplayId, ForegroundExecutor, Keymap, Menu, MenuItem, OwnedMenu,
    PathPromptOptions, Pixels, Platform, PlatformDisplay, PlatformKeyboardLayout,
    PlatformKeyboardMapper, PlatformTextSystem, PlatformWindow, Point, Task, ThermalState,
    WindowAppearance, WindowParams, px,
};

use crate::window::{McsapiWindow, WindowInner};
use crate::{Bridge, Host, HostEvent};

/// How often open windows are offered a frame. GPUI draws only when a window is dirty, so an
/// idle window costs one callback per tick.
const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

/// A GPUI platform whose windows are drawn by an mcsapi surface.
///
/// Everything that is not a window, cursor or clipboard is the wrapped platform's, which
/// should be headless: it runs the event loop and owns the main thread.
pub struct McsapiPlatform {
    inner: Rc<dyn Platform>,
    bridge: Arc<Bridge>,
    events: RefCell<Option<futures::channel::mpsc::UnboundedReceiver<HostEvent>>>,
    windows: Rc<RefCell<Vec<Weak<WindowInner>>>>,
    display: Rc<dyn PlatformDisplay>,
    next_window_id: RefCell<u64>,
    clipboard: Rc<RefCell<Option<ClipboardItem>>>,
    primary: RefCell<Option<ClipboardItem>>,
}

impl McsapiPlatform {
    /// Wraps `inner`, normally `gpui_platform::current_platform(true)`.
    pub fn new(inner: Rc<dyn Platform>, host: Host) -> Rc<Self> {
        Rc::new(Self {
            inner,
            display: Rc::new(SurfaceDisplay {
                bounds: Bounds::new(Point::default(), gpui::size(px(1920.), px(1080.))),
            }),
            bridge: host.bridge,
            events: RefCell::new(Some(host.events)),
            windows: Rc::new(RefCell::new(Vec::new())),
            next_window_id: RefCell::new(1),
            clipboard: Rc::new(RefCell::new(None)),
            primary: RefCell::new(None),
        })
    }

    fn start_pump(&self) {
        let Some(mut events) = self.events.borrow_mut().take() else {
            return;
        };
        let executor = self.inner.foreground_executor();
        let windows = self.windows.clone();
        let clipboard = self.clipboard.clone();
        executor
            .spawn(async move {
                while let Some(event) = events.next().await {
                    dispatch(&windows, &clipboard, event);
                }
            })
            .detach();

        let windows = self.windows.clone();
        let background = self.inner.background_executor();
        executor
            .spawn(async move {
                loop {
                    background.timer(FRAME_INTERVAL).await;
                    for window in live_windows(&windows) {
                        window.request_frame();
                    }
                }
            })
            .detach();
    }
}

fn live_windows(windows: &RefCell<Vec<Weak<WindowInner>>>) -> Vec<Rc<WindowInner>> {
    let mut windows = windows.borrow_mut();
    windows.retain(|window| window.strong_count() > 0);
    windows.iter().filter_map(Weak::upgrade).collect()
}

fn dispatch(
    windows: &RefCell<Vec<Weak<WindowInner>>>,
    clipboard: &RefCell<Option<ClipboardItem>>,
    event: HostEvent,
) {
    let find = |id: u64| live_windows(windows).into_iter().find(|window| window.id == id);
    match event {
        HostEvent::Input { window, input } => {
            if let Some(window) = find(window) {
                window.handle_input(input);
            }
        }
        HostEvent::Text { window, text } => {
            if let Some(window) = find(window) {
                window.insert_text(&text);
            }
        }
        HostEvent::Resize {
            window,
            size,
            scale_factor,
        } => {
            if let Some(window) = find(window) {
                window.resize(size, scale_factor);
                window.request_frame();
            }
        }
        HostEvent::Active { window, active } => {
            if let Some(window) = find(window) {
                window.set_active(active);
            }
        }
        HostEvent::Hovered { window, hovered } => {
            if let Some(window) = find(window) {
                window.set_hovered(hovered);
            }
        }
        HostEvent::CloseRequested { window } => {
            if let Some(window) = find(window) {
                window.request_close();
            }
        }
        HostEvent::Clipboard(text) => {
            *clipboard.borrow_mut() = Some(ClipboardItem::new_string(text));
        }
    }
}

#[derive(Debug)]
struct SurfaceDisplay {
    bounds: Bounds<Pixels>,
}

impl PlatformDisplay for SurfaceDisplay {
    fn id(&self) -> DisplayId {
        DisplayId::new(0)
    }

    fn uuid(&self) -> Result<uuid::Uuid> {
        // The surface is the only display GPUI sees.
        Ok(uuid::Uuid::nil())
    }

    fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }
}

impl Platform for McsapiPlatform {
    fn background_executor(&self) -> BackgroundExecutor {
        self.inner.background_executor()
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        self.inner.foreground_executor()
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.inner.text_system()
    }

    fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
        self.start_pump();
        self.inner.run(on_finish_launching);
    }

    fn quit(&self) {
        self.inner.quit();
    }

    fn restart(&self, binary_path: Option<PathBuf>, arguments: Vec<OsString>) {
        self.inner.restart(binary_path, arguments);
    }

    fn activate(&self, _ignoring_other_apps: bool) {
        self.bridge.context.request_repaint();
    }

    fn hide(&self) {}

    fn hide_other_apps(&self) {}

    fn unhide_other_apps(&self) {}

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        vec![self.display.clone()]
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(self.display.clone())
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        None
    }

    fn open_window(
        &self,
        _handle: AnyWindowHandle,
        options: WindowParams,
    ) -> Result<Box<dyn PlatformWindow>> {
        let id = {
            let mut next = self.next_window_id.borrow_mut();
            let id = *next;
            *next += 1;
            id
        };
        let window = McsapiWindow::new(id, options, self.bridge.clone(), self.display.clone());
        self.windows.borrow_mut().push(Rc::downgrade(&window.0));
        Ok(Box::new(window))
    }

    fn window_appearance(&self) -> WindowAppearance {
        if self.bridge.context.global_style().visuals.dark_mode {
            WindowAppearance::Dark
        } else {
            WindowAppearance::Light
        }
    }

    fn open_url(&self, url: &str) {
        self.inner.open_url(url);
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        self.inner.on_open_urls(callback);
    }

    fn register_url_scheme(&self, url: &str) -> Task<Result<()>> {
        self.inner.register_url_scheme(url)
    }

    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> futures::channel::oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        self.inner.prompt_for_paths(options)
    }

    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> futures::channel::oneshot::Receiver<Result<Option<PathBuf>>> {
        self.inner.prompt_for_new_path(directory, suggested_name)
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        self.inner.can_select_mixed_files_and_dirs()
    }

    fn reveal_path(&self, path: &Path) {
        self.inner.reveal_path(path);
    }

    fn open_with_system(&self, path: &Path) {
        self.inner.open_with_system(path);
    }

    fn on_quit(&self, callback: Box<dyn FnMut() -> bool>) {
        self.inner.on_quit(callback);
    }

    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_reopen(callback);
    }

    fn on_system_sleep(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_system_sleep(callback);
    }

    fn on_system_wake(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_system_wake(callback);
    }

    fn set_menus(&self, menus: Vec<Menu>, keymap: &Keymap) {
        self.inner.set_menus(menus, keymap);
    }

    fn get_menus(&self) -> Option<Vec<OwnedMenu>> {
        self.inner.get_menus()
    }

    fn set_dock_menu(&self, menu: Vec<MenuItem>, keymap: &Keymap) {
        self.inner.set_dock_menu(menu, keymap);
    }

    fn on_app_menu_action(&self, callback: Box<dyn FnMut(&dyn Action)>) {
        self.inner.on_app_menu_action(callback);
    }

    fn on_will_open_app_menu(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_will_open_app_menu(callback);
    }

    fn on_validate_app_menu_command(&self, callback: Box<dyn FnMut(&dyn Action) -> bool>) {
        self.inner.on_validate_app_menu_command(callback);
    }

    fn thermal_state(&self) -> ThermalState {
        self.inner.thermal_state()
    }

    fn on_thermal_state_change(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_thermal_state_change(callback);
    }

    fn prevent_idle_sleep(&self, reason: &str) -> Task<Result<ActivityGuard>> {
        self.inner.prevent_idle_sleep(reason)
    }

    fn set_app_identity(&self, identifier: &str, name: &str) {
        self.inner.set_app_identity(identifier, name);
    }

    fn show_system_notification(&self, notification: gpui::SystemNotification) {
        self.inner.show_system_notification(notification);
    }

    fn dismiss_system_notification(&self, tag: &str) {
        self.inner.dismiss_system_notification(tag);
    }

    fn on_system_notification_response(
        &self,
        callback: Box<dyn FnMut(gpui::SystemNotificationResponse)>,
    ) {
        self.inner.on_system_notification_response(callback);
    }

    fn compositor_name(&self) -> &'static str {
        "mcsapi"
    }

    fn app_path(&self) -> Result<PathBuf> {
        self.inner.app_path()
    }

    fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf> {
        self.inner.path_for_auxiliary_executable(name)
    }

    fn set_cursor_style(&self, style: CursorStyle) {
        let icon = cursor_icon(style);
        let mut cursor = self.bridge.cursor.lock();
        if *cursor != icon {
            *cursor = icon;
            self.bridge.context.request_repaint();
        }
    }

    fn hide_cursor_until_mouse_moves(&self) {}

    fn is_cursor_visible(&self) -> bool {
        true
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        false
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        self.clipboard.borrow().clone()
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        if let Some(text) = item.text() {
            self.bridge.context.copy_text(text);
            self.bridge.context.request_repaint();
        }
        *self.clipboard.borrow_mut() = Some(item);
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn read_from_primary(&self) -> Option<ClipboardItem> {
        self.primary.borrow().clone()
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn write_to_primary(&self, item: ClipboardItem) {
        // egui has no primary selection, so it stays inside the app.
        *self.primary.borrow_mut() = Some(item);
    }

    #[cfg(target_os = "macos")]
    fn read_from_find_pasteboard(&self) -> Option<ClipboardItem> {
        self.inner.read_from_find_pasteboard()
    }

    #[cfg(target_os = "macos")]
    fn write_to_find_pasteboard(&self, item: ClipboardItem) {
        self.inner.write_to_find_pasteboard(item);
    }

    fn write_credentials(&self, url: &str, username: &str, password: &[u8]) -> Task<Result<()>> {
        self.inner.write_credentials(url, username, password)
    }

    fn read_credentials(&self, url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        self.inner.read_credentials(url)
    }

    fn delete_credentials(&self, url: &str) -> Task<Result<()>> {
        self.inner.delete_credentials(url)
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        self.inner.keyboard_layout()
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        self.inner.keyboard_mapper()
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>) {
        self.inner.on_keyboard_layout_change(callback);
    }
}

fn cursor_icon(style: CursorStyle) -> egui::CursorIcon {
    use egui::CursorIcon;
    match style {
        CursorStyle::Arrow => CursorIcon::Default,
        CursorStyle::IBeam => CursorIcon::Text,
        CursorStyle::Crosshair => CursorIcon::Crosshair,
        CursorStyle::ClosedHand => CursorIcon::Grabbing,
        CursorStyle::OpenHand => CursorIcon::Grab,
        CursorStyle::PointingHand => CursorIcon::PointingHand,
        CursorStyle::ResizeLeft => CursorIcon::ResizeWest,
        CursorStyle::ResizeRight => CursorIcon::ResizeEast,
        CursorStyle::ResizeLeftRight => CursorIcon::ResizeHorizontal,
        CursorStyle::ResizeUp => CursorIcon::ResizeNorth,
        CursorStyle::ResizeDown => CursorIcon::ResizeSouth,
        CursorStyle::ResizeUpDown => CursorIcon::ResizeVertical,
        CursorStyle::ResizeUpLeftDownRight => CursorIcon::ResizeNwSe,
        CursorStyle::ResizeUpRightDownLeft => CursorIcon::ResizeNeSw,
        CursorStyle::ResizeColumn => CursorIcon::ResizeColumn,
        CursorStyle::ResizeRow => CursorIcon::ResizeRow,
        CursorStyle::IBeamCursorForVerticalLayout => CursorIcon::VerticalText,
        CursorStyle::OperationNotAllowed => CursorIcon::NotAllowed,
        CursorStyle::DragLink => CursorIcon::Alias,
        CursorStyle::DragCopy => CursorIcon::Copy,
        CursorStyle::ContextualMenu => CursorIcon::ContextMenu,
    }
}
