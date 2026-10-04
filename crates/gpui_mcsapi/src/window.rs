use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    Bounds, Capslock, DispatchEventResult, GpuSpecs, Modifiers, Pixels, PlatformAtlas,
    PlatformDisplay, PlatformInput, PlatformInputHandler, PlatformWindow, Point, PromptButton,
    PromptLevel, RequestFrameOptions, Scene, Size, WindowAppearance, WindowBackgroundAppearance,
    WindowBounds, WindowControlArea, WindowParams, WindowVisibility, px,
};

use crate::atlas::EguiAtlas;
use crate::scene::scene_to_shapes;
use crate::{Bridge, Frame, WindowSlot};

/// A GPUI window drawn by an mcsapi surface.
pub(crate) struct McsapiWindow(pub(crate) Rc<WindowInner>);

pub(crate) struct WindowInner {
    pub id: u64,
    bridge: Arc<Bridge>,
    atlas: Arc<EguiAtlas>,
    display: Rc<dyn PlatformDisplay>,
    state: RefCell<State>,
    callbacks: RefCell<Callbacks>,
}

struct State {
    bounds: Bounds<Pixels>,
    scale_factor: f32,
    mouse_position: Point<Pixels>,
    modifiers: Modifiers,
    capslock: Capslock,
    input_handler: Option<PlatformInputHandler>,
    title: String,
    active: bool,
    hovered: bool,
    fullscreen: bool,
    /// Set once the surface has reported a size, so the first frame is drawn at the size the
    /// user will see.
    laid_out: bool,
}

#[derive(Default)]
struct Callbacks {
    request_frame: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    input: Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    active_status_change: Option<Box<dyn FnMut(bool)>>,
    hover_status_change: Option<Box<dyn FnMut(bool)>>,
    resize: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    should_close: Option<Box<dyn FnMut() -> bool>>,
    close: Option<Box<dyn FnOnce()>>,
}

/// Takes a callback out while it runs: a callback may call back into the window, for example
/// to install a new input handler, and must not find the cell borrowed.
macro_rules! with_callback {
    ($self:expr, $name:ident, |$callback:ident| $body:expr) => {{
        let taken = $self.callbacks.borrow_mut().$name.take();
        taken.map(|mut $callback| {
            let result = $body;
            let mut callbacks = $self.callbacks.borrow_mut();
            if callbacks.$name.is_none() {
                callbacks.$name = Some($callback);
            }
            result
        })
    }};
}

impl McsapiWindow {
    pub(crate) fn new(
        id: u64,
        params: WindowParams,
        bridge: Arc<Bridge>,
        display: Rc<dyn PlatformDisplay>,
    ) -> Self {
        let title = params
            .titlebar
            .as_ref()
            .and_then(|titlebar| titlebar.title.as_ref())
            .map(|title| title.to_string())
            .unwrap_or_default();
        bridge.windows.lock().push(WindowSlot {
            id,
            title: title.clone(),
            frame: None,
            requested_size: egui::vec2(
                f32::from(params.bounds.size.width),
                f32::from(params.bounds.size.height),
            ),
        });
        bridge.context.request_repaint();
        let atlas = Arc::new(EguiAtlas::new(bridge.context.clone()));
        Self(Rc::new(WindowInner {
            id,
            bridge,
            atlas,
            display,
            state: RefCell::new(State {
                bounds: params.bounds,
                scale_factor: 1.0,
                mouse_position: Point::default(),
                modifiers: Modifiers::default(),
                capslock: Capslock::default(),
                input_handler: None,
                title,
                active: false,
                hovered: false,
                fullscreen: false,
                laid_out: false,
            }),
            callbacks: RefCell::new(Callbacks::default()),
        }))
    }
}

impl Drop for McsapiWindow {
    fn drop(&mut self) {
        let id = self.0.id;
        self.0.bridge.windows.lock().retain(|slot| slot.id != id);
        self.0.bridge.context.request_repaint();
    }
}

impl WindowInner {
    pub(crate) fn request_frame(&self) {
        if !self.state.borrow().laid_out {
            return;
        }
        with_callback!(self, request_frame, |callback| callback(
            RequestFrameOptions {
                require_presentation: false,
                force_render: false,
                ..Default::default()
            }
        ));
    }

    pub(crate) fn handle_input(&self, input: PlatformInput) {
        {
            let mut state = self.state.borrow_mut();
            match &input {
                PlatformInput::MouseMove(event) => {
                    state.mouse_position = event.position;
                    state.modifiers = event.modifiers;
                }
                PlatformInput::MouseDown(event) => state.modifiers = event.modifiers,
                PlatformInput::MouseUp(event) => state.modifiers = event.modifiers,
                PlatformInput::ModifiersChanged(event) => {
                    state.modifiers = event.modifiers;
                    state.capslock = event.capslock;
                }
                _ => {}
            }
        }
        let result = with_callback!(self, input, |callback| callback(input.clone()));
        if result.is_some_and(|result| !result.propagate) {
            return;
        }
        // Like the Wayland and X11 windows: a key nothing bound types its character.
        if let PlatformInput::KeyDown(event) = input
            && event.keystroke.modifiers.is_subset_of(&Modifiers::shift())
            && let Some(key_char) = &event.keystroke.key_char
        {
            self.insert_text(key_char);
        }
    }

    pub(crate) fn insert_text(&self, text: &str) {
        let handler = self.state.borrow_mut().input_handler.take();
        if let Some(mut handler) = handler {
            handler.replace_text_in_range(None, text);
            let mut state = self.state.borrow_mut();
            if state.input_handler.is_none() {
                state.input_handler = Some(handler);
            }
        }
    }

    pub(crate) fn resize(&self, size: egui::Vec2, scale_factor: f32) {
        let size = Size {
            width: px(size.x),
            height: px(size.y),
        };
        {
            let mut state = self.state.borrow_mut();
            if state.laid_out && state.bounds.size == size && state.scale_factor == scale_factor {
                return;
            }
            state.bounds.size = size;
            state.scale_factor = scale_factor;
            state.laid_out = true;
        }
        with_callback!(self, resize, |callback| callback(size, scale_factor));
    }

    pub(crate) fn set_active(&self, active: bool) {
        self.state.borrow_mut().active = active;
        with_callback!(self, active_status_change, |callback| callback(active));
    }

    pub(crate) fn set_hovered(&self, hovered: bool) {
        self.state.borrow_mut().hovered = hovered;
        with_callback!(self, hover_status_change, |callback| callback(hovered));
    }

    pub(crate) fn request_close(&self) {
        let should_close =
            with_callback!(self, should_close, |callback| callback()).unwrap_or(true);
        if should_close {
            let close = self.callbacks.borrow_mut().close.take();
            if let Some(close) = close {
                close();
            }
        }
    }
}

impl raw_window_handle::HasWindowHandle for McsapiWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        // The surface may be a compositor client or a region of another app's window, so
        // there is no native window to hand out.
        Err(raw_window_handle::HandleError::NotSupported)
    }
}

impl raw_window_handle::HasDisplayHandle for McsapiWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::NotSupported)
    }
}

impl PlatformWindow for McsapiWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.0.state.borrow().bounds
    }

    fn is_maximized(&self) -> bool {
        false
    }

    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Windowed(self.bounds())
    }

    fn content_size(&self) -> Size<Pixels> {
        self.bounds().size
    }

    fn resize(&mut self, size: Size<Pixels>) {
        // The surface owns the layout; this becomes the size it is asked for.
        let id = self.0.id;
        if let Some(slot) = self
            .0
            .bridge
            .windows
            .lock()
            .iter_mut()
            .find(|slot| slot.id == id)
        {
            slot.requested_size = egui::vec2(f32::from(size.width), f32::from(size.height));
        }
        self.0.bridge.context.request_repaint();
    }

    fn scale_factor(&self) -> f32 {
        self.0.state.borrow().scale_factor
    }

    fn appearance(&self) -> WindowAppearance {
        if self.0.bridge.context.global_style().visuals.dark_mode {
            WindowAppearance::Dark
        } else {
            WindowAppearance::Light
        }
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(self.0.display.clone())
    }

    fn mouse_position(&self) -> Point<Pixels> {
        self.0.state.borrow().mouse_position
    }

    fn modifiers(&self) -> Modifiers {
        self.0.state.borrow().modifiers
    }

    fn capslock(&self) -> Capslock {
        self.0.state.borrow().capslock
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.0.state.borrow_mut().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.state.borrow_mut().input_handler.take()
    }

    fn prompt(
        &self,
        _level: PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<futures::channel::oneshot::Receiver<usize>> {
        // GPUI draws its own prompt inside the window when the platform has none.
        None
    }

    fn activate(&self) {
        self.0.bridge.context.request_repaint();
    }

    fn is_active(&self) -> bool {
        self.0.state.borrow().active
    }

    fn visibility(&self) -> WindowVisibility {
        WindowVisibility::Visible
    }

    fn is_hovered(&self) -> bool {
        self.0.state.borrow().hovered
    }

    fn background_appearance(&self) -> WindowBackgroundAppearance {
        WindowBackgroundAppearance::Opaque
    }

    fn set_title(&mut self, title: &str) {
        self.0.state.borrow_mut().title = title.to_owned();
        let id = self.0.id;
        if let Some(slot) = self
            .0
            .bridge
            .windows
            .lock()
            .iter_mut()
            .find(|slot| slot.id == id)
        {
            slot.title = title.to_owned();
        }
    }

    fn get_title(&self) -> String {
        self.0.state.borrow().title.clone()
    }

    fn set_background_appearance(&self, _background: WindowBackgroundAppearance) {}

    fn minimize(&self) {}

    fn zoom(&self) {}

    fn toggle_fullscreen(&self) {
        let mut state = self.0.state.borrow_mut();
        state.fullscreen = !state.fullscreen;
    }

    fn is_fullscreen(&self) -> bool {
        self.0.state.borrow().fullscreen
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.callbacks.borrow_mut().request_frame = Some(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.0.callbacks.borrow_mut().input = Some(callback);
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.callbacks.borrow_mut().active_status_change = Some(callback);
    }

    fn on_visibility_change(&self, _callback: Box<dyn FnMut(WindowVisibility)>) {}

    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.callbacks.borrow_mut().hover_status_change = Some(callback);
    }

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.0.callbacks.borrow_mut().resize = Some(callback);
    }

    fn on_moved(&self, _callback: Box<dyn FnMut()>) {}

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.0.callbacks.borrow_mut().should_close = Some(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.callbacks.borrow_mut().close = Some(callback);
    }

    fn on_hit_test_window_control(&self, _callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
    }

    fn on_appearance_changed(&self, _callback: Box<dyn FnMut()>) {}

    fn draw(&self, scene: &Scene) {
        let scale_factor = self.0.state.borrow().scale_factor;
        let atlas = &self.0.atlas;
        let shapes = scene_to_shapes(scene, scale_factor, &|id| atlas.slot(id));
        let id = self.0.id;
        if let Some(slot) = self
            .0
            .bridge
            .windows
            .lock()
            .iter_mut()
            .find(|slot| slot.id == id)
        {
            slot.frame = Some(Arc::new(Frame { shapes }));
        }
        self.0.bridge.context.request_repaint();
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.0.atlas.clone()
    }

    fn is_subpixel_rendering_supported(&self) -> bool {
        false
    }

    fn update_ime_position(&self, _bounds: Bounds<Pixels>) {}

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        None
    }
}
