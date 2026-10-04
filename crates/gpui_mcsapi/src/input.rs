use std::time::{Duration, Instant};

use egui::{Event, Key, MouseWheelUnit, PointerButton};
use gpui::{
    KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, NavigationDirection,
    PlatformInput, Point, ScrollDelta, ScrollWheelEvent, TouchPhase, px,
};

use crate::HostEvent;

/// Lines per wheel notch, as GPUI's X11 and Wayland backends scroll.
const LINES_PER_NOTCH: f32 = 3.0;
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(400);
const DOUBLE_CLICK_DISTANCE: f32 = 4.0;

/// Turns one egui frame's input into GPUI events for one window.
///
/// Positions arrive in the surface's points and leave relative to the window's origin, which
/// is also in points: GPUI's logical pixels are egui's points when the scale factors match.
#[derive(Default)]
pub(crate) struct InputTranslator {
    modifiers: Modifiers,
    position: Point<gpui::Pixels>,
    pressed: Option<MouseButton>,
    last_click: Option<(MouseButton, Instant, egui::Pos2, usize)>,
}

impl InputTranslator {
    pub(crate) fn translate(
        &mut self,
        window: u64,
        origin: egui::Pos2,
        events: &[Event],
        modifiers: egui::Modifiers,
        mut send: impl FnMut(HostEvent),
    ) {
        let modifiers = convert_modifiers(modifiers);
        if modifiers != self.modifiers {
            self.modifiers = modifiers;
            send_input(
                &mut send,
                window,
                PlatformInput::ModifiersChanged(ModifiersChangedEvent {
                    modifiers,
                    capslock: Default::default(),
                }),
            );
        }

        let mut events = events.iter().peekable();
        while let Some(event) = events.next() {
            match event {
                Event::PointerMoved(position) => {
                    self.position = to_point(*position - origin.to_vec2());
                    send_input(
                        &mut send,
                        window,
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position: self.position,
                            pressed_button: self.pressed,
                            modifiers,
                        }),
                    );
                }
                Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers: event_modifiers,
                } => {
                    let Some(button) = convert_button(*button) else {
                        continue;
                    };
                    let modifiers = convert_modifiers(*event_modifiers);
                    let position = *pos - origin.to_vec2();
                    self.position = to_point(position);
                    if *pressed {
                        let click_count = self.click_count(button, position);
                        self.pressed = Some(button);
                        send_input(
                            &mut send,
                            window,
                            PlatformInput::MouseDown(MouseDownEvent {
                                button,
                                position: self.position,
                                modifiers,
                                click_count,
                                first_mouse: false,
                            }),
                        );
                    } else {
                        self.pressed = None;
                        let click_count = self.last_click.map_or(1, |(_, _, _, count)| count);
                        send_input(
                            &mut send,
                            window,
                            PlatformInput::MouseUp(MouseUpEvent {
                                button,
                                position: self.position,
                                modifiers,
                                click_count,
                            }),
                        );
                    }
                }
                Event::PointerGone => {
                    send_input(
                        &mut send,
                        window,
                        PlatformInput::MouseExited(MouseExitEvent {
                            position: self.position,
                            pressed_button: self.pressed,
                            modifiers,
                        }),
                    );
                }
                Event::MouseWheel {
                    unit,
                    delta,
                    modifiers: event_modifiers,
                    ..
                } => {
                    let delta = match unit {
                        MouseWheelUnit::Point => {
                            ScrollDelta::Pixels(Point::new(px(delta.x), px(delta.y)))
                        }
                        MouseWheelUnit::Line => ScrollDelta::Lines(Point::new(
                            delta.x * LINES_PER_NOTCH,
                            delta.y * LINES_PER_NOTCH,
                        )),
                        // GPUI has no page unit; a page is close to a screenful of lines.
                        MouseWheelUnit::Page => {
                            ScrollDelta::Lines(Point::new(delta.x * 30.0, delta.y * 30.0))
                        }
                    };
                    send_input(
                        &mut send,
                        window,
                        PlatformInput::ScrollWheel(ScrollWheelEvent {
                            position: self.position,
                            delta,
                            modifiers: convert_modifiers(*event_modifiers),
                            touch_phase: TouchPhase::Moved,
                        }),
                    );
                }
                Event::Key {
                    key,
                    pressed,
                    repeat,
                    modifiers: event_modifiers,
                    ..
                } => {
                    let modifiers = convert_modifiers(*event_modifiers);
                    // egui reports the typed character as a Text event right after its key.
                    let key_char = if *pressed && !modifiers.control && !modifiers.alt {
                        match events.peek() {
                            Some(Event::Text(text)) => {
                                let text = text.clone();
                                events.next();
                                Some(text)
                            }
                            _ => None,
                        }
                    } else {
                        None
                    };
                    let keystroke = Keystroke {
                        modifiers,
                        key: key_name(*key),
                        key_char,
                    };
                    if *pressed {
                        send_input(
                            &mut send,
                            window,
                            PlatformInput::KeyDown(KeyDownEvent {
                                keystroke,
                                is_held: *repeat,
                                prefer_character_input: false,
                            }),
                        );
                    } else {
                        send_input(
                            &mut send,
                            window,
                            PlatformInput::KeyUp(KeyUpEvent { keystroke }),
                        );
                    }
                }
                Event::Text(text) => send(HostEvent::Text {
                    window,
                    text: text.clone(),
                }),
                Event::Ime(egui::ImeEvent::Commit(text)) => send(HostEvent::Text {
                    window,
                    text: text.clone(),
                }),
                // egui-winit turns the clipboard shortcuts into these instead of key events,
                // so they go back to GPUI as the shortcuts its keymap binds.
                Event::Copy => self.shortcut(&mut send, window, "c"),
                Event::Cut => self.shortcut(&mut send, window, "x"),
                Event::Paste(text) => {
                    send(HostEvent::Clipboard(text.clone()));
                    self.shortcut(&mut send, window, "v");
                }
                Event::WindowFocused(active) => send(HostEvent::Active {
                    window,
                    active: *active,
                }),
                _ => {}
            }
        }
    }

    fn shortcut(&self, send: &mut impl FnMut(HostEvent), window: u64, key: &str) {
        let keystroke = Keystroke {
            modifiers: Modifiers {
                control: true,
                ..self.modifiers
            },
            key: key.to_owned(),
            key_char: None,
        };
        send_input(
            send,
            window,
            PlatformInput::KeyDown(KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            }),
        );
        send_input(send, window, PlatformInput::KeyUp(KeyUpEvent { keystroke }));
    }

    fn click_count(&mut self, button: MouseButton, position: egui::Pos2) -> usize {
        let now = Instant::now();
        let count = match self.last_click {
            Some((last_button, at, last_position, count))
                if last_button == button
                    && now.duration_since(at) <= DOUBLE_CLICK_INTERVAL
                    && last_position.distance(position) <= DOUBLE_CLICK_DISTANCE =>
            {
                count + 1
            }
            _ => 1,
        };
        self.last_click = Some((button, now, position, count));
        count
    }
}

fn send_input(send: &mut impl FnMut(HostEvent), window: u64, input: PlatformInput) {
    send(HostEvent::Input { window, input });
}

fn to_point(position: egui::Pos2) -> Point<gpui::Pixels> {
    Point::new(px(position.x), px(position.y))
}

fn convert_modifiers(modifiers: egui::Modifiers) -> Modifiers {
    Modifiers {
        control: modifiers.ctrl,
        alt: modifiers.alt,
        shift: modifiers.shift,
        // egui reports the platform key only on macOS, as `mac_cmd`.
        platform: modifiers.mac_cmd,
        function: false,
    }
}

fn convert_button(button: PointerButton) -> Option<MouseButton> {
    Some(match button {
        PointerButton::Primary => MouseButton::Left,
        PointerButton::Secondary => MouseButton::Right,
        PointerButton::Middle => MouseButton::Middle,
        PointerButton::Extra1 => MouseButton::Navigate(NavigationDirection::Back),
        PointerButton::Extra2 => MouseButton::Navigate(NavigationDirection::Forward),
    })
}

/// GPUI's name for a key, as its keymaps write it.
fn key_name(key: Key) -> String {
    let name = match key {
        Key::ArrowDown => "down",
        Key::ArrowLeft => "left",
        Key::ArrowRight => "right",
        Key::ArrowUp => "up",
        Key::Escape => "escape",
        Key::Tab => "tab",
        Key::Backspace => "backspace",
        Key::Enter => "enter",
        Key::Space => "space",
        Key::Insert => "insert",
        Key::Delete => "delete",
        Key::Home => "home",
        Key::End => "end",
        Key::PageUp => "pageup",
        Key::PageDown => "pagedown",
        Key::Copy => "copy",
        Key::Cut => "cut",
        Key::Paste => "paste",
        // egui's symbols for these are typographic (a minus sign, for one); keymaps write ASCII.
        Key::Colon => ":",
        Key::Comma => ",",
        Key::Backslash => "\\",
        Key::Slash => "/",
        Key::Pipe => "|",
        Key::Questionmark => "?",
        Key::Exclamationmark => "!",
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        Key::OpenCurlyBracket => "{",
        Key::CloseCurlyBracket => "}",
        Key::Backtick => "`",
        Key::Minus => "-",
        Key::Period => ".",
        Key::Plus => "+",
        Key::Equals => "=",
        Key::Semicolon => ";",
        Key::Quote => "'",
        _ => return key.symbol_or_name().to_lowercase(),
    };
    name.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_use_gpui_names() {
        assert_eq!(key_name(Key::A), "a");
        assert_eq!(key_name(Key::Num1), "1");
        assert_eq!(key_name(Key::F5), "f5");
        assert_eq!(key_name(Key::Minus), "-");
        assert_eq!(key_name(Key::Backslash), "\\");
        assert_eq!(key_name(Key::PageDown), "pagedown");
    }

    #[test]
    fn typed_character_rides_on_its_key() {
        let mut translator = InputTranslator::default();
        let mut sent = Vec::new();
        let shift = egui::Modifiers {
            shift: true,
            ..Default::default()
        };
        translator.translate(
            7,
            egui::Pos2::ZERO,
            &[
                Event::Key {
                    key: Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: shift,
                },
                Event::Text("A".into()),
            ],
            shift,
            |event| sent.push(event),
        );
        let key_downs: Vec<_> = sent
            .iter()
            .filter_map(|event| match event {
                HostEvent::Input {
                    input: PlatformInput::KeyDown(event),
                    ..
                } => Some(event.keystroke.clone()),
                HostEvent::Text { .. } => panic!("the character was sent twice"),
                _ => None,
            })
            .collect();
        assert_eq!(key_downs.len(), 1);
        assert_eq!(key_downs[0].key, "a");
        assert_eq!(key_downs[0].key_char.as_deref(), Some("A"));
        assert!(key_downs[0].modifiers.shift);
    }
}
