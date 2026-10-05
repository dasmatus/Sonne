//! The project setup in the editor: the same five questions as Sonne's agent
//! window ([`sonne_setup`]), asked with Zed's own components, ending in a
//! thread that sends the final prompt.

use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use gpui::{
    DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, ScrollHandle, Subscription,
    WeakEntity, prelude::*,
};
use sonne_setup::{
    AppSetup, FlatpakRuntime, Language, Metadata, PwaDisplay, STEPS, SetupForm, WasmKind,
};
use ui::{
    Checkbox, ChoiceCard, KeyBinding, Modal, ModalFooter, ModalHeader, Section, SectionHeader,
    ToggleState, WithScrollbar, prelude::*,
};
use ui_input::{ErasedEditor, ErasedEditorEvent, InputField};
use util::ResultExt as _;
use workspace::{ModalView, Workspace};

use crate::{AgentInitialContent, AgentPanel, AgentThreadSource};

/// The metadata the user types, each kept in an [`InputField`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    DisplayName,
    Summary,
    Version,
    License,
    Homepage,
    Folder,
    PmDependencies,
    FlatpakId,
    FlatpakRuntimeVersion,
    PwaShortName,
    PwaStartUrl,
    PwaThemeColor,
    PwaBackgroundColor,
}

impl Field {
    const ALL: [Self; 14] = [
        Self::Name,
        Self::DisplayName,
        Self::Summary,
        Self::Version,
        Self::License,
        Self::Homepage,
        Self::Folder,
        Self::PmDependencies,
        Self::FlatpakId,
        Self::FlatpakRuntimeVersion,
        Self::PwaShortName,
        Self::PwaStartUrl,
        Self::PwaThemeColor,
        Self::PwaBackgroundColor,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Name => "Package name",
            Self::DisplayName => "Display name",
            Self::Summary => "Summary",
            Self::Version => "Version",
            Self::License => "Licence",
            Self::Homepage => "Homepage",
            Self::Folder => "Folder",
            Self::PmDependencies => "pm dependencies",
            Self::FlatpakId => "App ID",
            Self::FlatpakRuntimeVersion => "Runtime version",
            Self::PwaShortName => "Short name",
            Self::PwaStartUrl => "Start URL",
            Self::PwaThemeColor => "Theme colour",
            Self::PwaBackgroundColor => "Background colour",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Self::Name => "todo-list",
            Self::DisplayName => "From the package name",
            Self::Summary => "One line for app stores",
            Self::Version => "0.1.0",
            Self::License => "AGPL-3.0-or-later",
            Self::Homepage => "https://",
            Self::Folder => "~/src/<package name>",
            Self::PmDependencies => "Comma separated",
            Self::FlatpakId => "org.example.TodoList",
            Self::FlatpakRuntimeVersion => "",
            Self::PwaShortName => "From the display name",
            Self::PwaStartUrl => "/",
            Self::PwaThemeColor | Self::PwaBackgroundColor => "#111827",
        }
    }

    fn value(self, metadata: &mut Metadata) -> &mut String {
        match self {
            Self::Name => &mut metadata.name,
            Self::DisplayName => &mut metadata.display_name,
            Self::Summary => &mut metadata.summary,
            Self::Version => &mut metadata.version,
            Self::License => &mut metadata.license,
            Self::Homepage => &mut metadata.homepage,
            Self::Folder => &mut metadata.folder,
            Self::PmDependencies => &mut metadata.pm_dependencies,
            Self::FlatpakId => &mut metadata.flatpak_id,
            Self::FlatpakRuntimeVersion => &mut metadata.flatpak_runtime_version,
            Self::PwaShortName => &mut metadata.pwa_short_name,
            Self::PwaStartUrl => &mut metadata.pwa_start_url,
            Self::PwaThemeColor => &mut metadata.pwa_theme_color,
            Self::PwaBackgroundColor => &mut metadata.pwa_background_color,
        }
    }
}

pub struct AppSetupModal {
    form: SetupForm,
    fields: Vec<(Field, Entity<InputField>)>,
    description: Entity<InputField>,
    prompt: Entity<InputField>,
    workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl AppSetupModal {
    pub fn toggle(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
        let workspace_handle = cx.weak_entity();
        workspace.toggle_modal(window, cx, |window, cx| {
            Self::new(AppSetup::default(), workspace_handle, window, cx)
        });
    }

    fn new(
        setup: AppSetup,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut form = SetupForm::new(setup, None);
        let mut subscriptions = Vec::new();
        let mut watch = |input: &Entity<InputField>, window: &mut Window, cx: &mut Context<Self>| {
            let this = cx.weak_entity();
            subscriptions.push(editor(input, cx).subscribe(
                Box::new(move |event, _window, cx| {
                    if event == ErasedEditorEvent::BufferEdited {
                        this.update(cx, |this, cx| {
                            this.read_inputs(cx);
                            cx.notify();
                        })
                        .log_err();
                    }
                }),
                window,
                cx,
            ));
        };

        let fields: Vec<_> = Field::ALL
            .into_iter()
            .map(|field| {
                let value = field.value(&mut form.setup.metadata).clone();
                let input = cx.new(|cx| {
                    InputField::new(window, cx, field.placeholder())
                        .label(field.label())
                        .label_min_width(px(0.))
                });
                editor(&input, cx).set_text(&value, window, cx);
                watch(&input, window, cx);
                (field, input)
            })
            .collect();

        let description = cx.new(|cx| {
            InputField::new(
                window,
                cx,
                "A to-do list that syncs between my phone and my laptop…",
            )
        });
        editor(&description, cx).set_multiline(Some(12), window, cx);
        watch(&description, window, cx);

        let prompt = cx.new(|cx| InputField::new(window, cx, ""));
        editor(&prompt, cx).set_multiline(Some(18), window, cx);
        watch(&prompt, window, cx);

        Self {
            form,
            fields,
            description,
            prompt,
            workspace,
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
            error: None,
            _subscriptions: subscriptions,
        }
    }

    /// Copies what the user typed into the answers.
    fn read_inputs(&mut self, cx: &App) {
        for (field, input) in &self.fields {
            *field.value(&mut self.form.setup.metadata) = input.read(cx).text(cx);
        }
        self.form.setup.description = self.description.read(cx).text(cx);
        let prompt = self.prompt.read(cx).text(cx);
        // Writing the composed prompt into the field reports an edit too;
        // only text that differs from it is the user's.
        if prompt != self.form.prompt {
            self.form.prompt = prompt;
            self.form.prompt_edited = true;
        }
    }

    fn go_to(&mut self, step: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.read_inputs(cx);
        self.error = None;
        self.form.go_to(step);
        if step == STEPS.len() - 1 {
            editor(&self.prompt, cx).set_text(&self.form.prompt, window, cx);
        }
        self.scroll_handle.scroll_to_top_of_item(0);
        cx.notify();
    }

    fn set_text(&self, field: Field, text: &str, window: &mut Window, cx: &mut App) {
        if let Some((_, input)) = self.fields.iter().find(|(each, _)| *each == field) {
            editor(input, cx).set_text(text, window, cx);
        }
    }

    fn set_language(&mut self, language: Language, window: &mut Window, cx: &mut Context<Self>) {
        self.form.setup.set_language(language);
        // The runtime version may have followed the language.
        let version = self.form.setup.metadata.flatpak_runtime_version.clone();
        self.set_text(Field::FlatpakRuntimeVersion, &version, window, cx);
        cx.notify();
    }

    fn set_runtime(&mut self, runtime: FlatpakRuntime, window: &mut Window, cx: &mut Context<Self>) {
        let metadata = &mut self.form.setup.metadata;
        metadata.flatpak_runtime = runtime;
        metadata.flatpak_runtime_version = runtime.default_version().into();
        self.set_text(
            Field::FlatpakRuntimeVersion,
            runtime.default_version(),
            window,
            cx,
        );
        cx.notify();
    }

    fn cancel(&mut self, _: &menu::Cancel, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    fn confirm(&mut self, _: &menu::Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let last = STEPS.len() - 1;
        if self.form.step < last {
            let next = self.form.step + 1;
            if self.form.reachable() >= next {
                self.go_to(next, window, cx);
            }
            return;
        }
        self.start(window, cx);
    }

    /// Creates the app's folder, adds it to the project and sends the prompt
    /// in a new thread.
    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.read_inputs(cx);
        self.error = None;
        let setup = self.form.setup.clone();
        if let Some(problem) = setup.problem(STEPS.len() - 1) {
            self.error = Some(problem.into());
            cx.notify();
            return;
        }
        let folder = setup.folder();
        if let Err(error) = setup.write_files() {
            self.error = Some(format!("Could not set up {}: {error}", folder.display()).into());
            cx.notify();
            return;
        }
        let Some(workspace) = self.workspace.upgrade() else {
            cx.emit(DismissEvent);
            return;
        };
        let prompt = self.form.prompt.trim().to_owned();
        let add_folder = workspace.update(cx, |workspace, cx| {
            workspace.project().update(cx, |project, cx| {
                project.find_or_create_worktree(&folder, true, cx)
            })
        });
        let workspace = workspace.downgrade();
        window
            .spawn(cx, async move |cx| {
                add_folder.await?;
                workspace.update_in(cx, |workspace, window, cx| {
                    let Some(panel) = workspace.focus_panel::<AgentPanel>(window, cx) else {
                        log::warn!("the agent panel is off, so the app's prompt was not sent");
                        return;
                    };
                    panel.update(cx, |panel, cx| {
                        panel.external_thread(
                            None,
                            None,
                            None,
                            None,
                            Some(AgentInitialContent::ContentBlock {
                                blocks: vec![acp::ContentBlock::Text(acp::TextContent::new(
                                    prompt,
                                ))],
                                // The user read and accepted the prompt in
                                // the last step.
                                auto_submit: true,
                            }),
                            true,
                            AgentThreadSource::AgentPanel,
                            window,
                            cx,
                        );
                    });
                })
            })
            .detach_and_log_err(cx);
        cx.emit(DismissEvent);
    }

    fn render_steps(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let reachable = self.form.reachable();
        h_flex()
            .gap_1()
            .flex_wrap()
            .children(STEPS.iter().enumerate().map(|(index, name)| {
                Button::new(("setup-step", index), format!("{}. {name}", index + 1))
                    .style(ButtonStyle::Subtle)
                    .toggle_state(index == self.form.step)
                    .disabled(index > reachable)
                    .on_click(cx.listener(move |this, _, window, cx| this.go_to(index, window, cx)))
            }))
    }

    fn render_kind(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let setup = &self.form.setup;
        let card = |id: &'static str,
                    label: &'static str,
                    description: &'static str,
                    selected: bool,
                    toggle: fn(&mut AppSetup)| {
            ChoiceCard::checkbox(id, label, selected)
                .description(description)
                .on_click(cx.listener(move |this, _, _window, cx| {
                    toggle(&mut this.form.setup);
                    cx.notify();
                }))
        };
        v_flex()
            .gap_2()
            .child(Label::new("What kind of app is it? Pick one or more.").color(Color::Muted))
            .child(card(
                "kind-losos",
                "A LosOS app",
                "Installed with pm, from a build file the agent writes.",
                setup.losos,
                |setup| setup.losos = !setup.losos,
            ))
            .child(card(
                "kind-flatpak",
                "A Flatpak",
                "Sandboxed, for Bazaar, Flathub and other distributions.",
                setup.flatpak,
                |setup| setup.flatpak = !setup.flatpak,
            ))
            .child(card(
                "kind-wasm",
                "A WASM app",
                "A wasm32-wasip2 component Sonne can preview, and optionally a web app.",
                setup.wasm,
                |setup| setup.wasm = !setup.wasm,
            ))
    }

    fn render_language(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.form.setup.language;
        v_flex()
            .gap_2()
            .child(
                Label::new("Which language should the agent write it in?").color(Color::Muted),
            )
            .children(Language::ALL.into_iter().enumerate().map(|(index, language)| {
                ChoiceCard::radio(("language", index), language.label(), language == selected)
                    .description(language.note())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_language(language, window, cx)
                    }))
            }))
    }

    fn field(&self, field: Field) -> AnyElement {
        self.fields
            .iter()
            .find(|(each, _)| *each == field)
            .map(|(_, input)| div().flex_1().child(input.clone()).into_any_element())
            .unwrap_or_else(|| div().into_any_element())
    }

    fn row(&self, fields: &[Field]) -> impl IntoElement {
        h_flex()
            .gap_2()
            .items_start()
            .children(fields.iter().map(|field| self.field(*field)))
    }

    fn render_package(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let setup = &self.form.setup;
        let metadata = &setup.metadata;
        let permission = |id: &'static str, label: &'static str, on: bool, toggle: fn(&mut Metadata)| {
            Checkbox::new(id, ToggleState::from(on))
                .label(label)
                .on_click(cx.listener(move |this, _, _window, cx| {
                    toggle(&mut this.form.setup.metadata);
                    cx.notify();
                }))
        };

        let mut package = v_flex()
            .gap_3()
            .child(self.row(&[Field::Name, Field::DisplayName]))
            .child(self.row(&[Field::Summary]))
            .child(self.row(&[Field::Version, Field::License]))
            .child(self.row(&[Field::Homepage, Field::Folder]));

        if setup.losos {
            package = package
                .child(SectionHeader::new("LosOS (pm)"))
                .child(self.row(&[Field::PmDependencies]));
        }

        if setup.flatpak {
            let selected = metadata.flatpak_runtime;
            package = package
                .child(SectionHeader::new("Flatpak"))
                .child(self.row(&[Field::FlatpakId, Field::FlatpakRuntimeVersion]))
                .child(
                    h_flex().gap_2().children(FlatpakRuntime::ALL.into_iter().enumerate().map(
                        |(index, runtime)| {
                            Button::new(("runtime", index), runtime.id())
                                .style(ButtonStyle::Outlined)
                                .toggle_state(runtime == selected)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.set_runtime(runtime, window, cx)
                                }))
                        },
                    )),
                )
                .child(
                    h_flex()
                        .gap_4()
                        .flex_wrap()
                        .child(permission("flatpak-gpu", "GPU", metadata.flatpak_gpu, |m| {
                            m.flatpak_gpu = !m.flatpak_gpu
                        }))
                        .child(permission(
                            "flatpak-network",
                            "Network",
                            metadata.flatpak_network,
                            |m| m.flatpak_network = !m.flatpak_network,
                        ))
                        .child(permission(
                            "flatpak-home",
                            "Home folder",
                            metadata.flatpak_home,
                            |m| m.flatpak_home = !m.flatpak_home,
                        ))
                        .child(permission(
                            "flatpak-audio",
                            "Audio",
                            metadata.flatpak_audio,
                            |m| m.flatpak_audio = !m.flatpak_audio,
                        )),
                );
        }

        if setup.wasm {
            let kind = metadata.wasm_kind;
            package = package
                .child(SectionHeader::new("WASM"))
                .children(WasmKind::ALL.into_iter().enumerate().map(|(index, each)| {
                    ChoiceCard::radio(("wasm-kind", index), each.label(), each == kind).on_click(
                        cx.listener(move |this, _, _window, cx| {
                            this.form.setup.metadata.wasm_kind = each;
                            cx.notify();
                        }),
                    )
                }))
                .child(permission(
                    "pwa",
                    "Also an installable web app (PWA), with a manifest Sonne writes",
                    metadata.pwa,
                    |m| m.pwa = !m.pwa,
                ));
            if metadata.pwa {
                let display = metadata.pwa_display;
                package = package
                    .child(self.row(&[Field::PwaShortName, Field::PwaStartUrl]))
                    .child(self.row(&[Field::PwaThemeColor, Field::PwaBackgroundColor]))
                    .child(
                        h_flex().gap_2().children(PwaDisplay::ALL.into_iter().enumerate().map(
                            |(index, each)| {
                                Button::new(("pwa-display", index), each.value())
                                    .style(ButtonStyle::Outlined)
                                    .toggle_state(each == display)
                                    .on_click(cx.listener(move |this, _, _window, cx| {
                                        this.form.setup.metadata.pwa_display = each;
                                        cx.notify();
                                    }))
                            },
                        )),
                    );
            }
        }
        package
    }

    fn render_description(&self) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(
                Label::new(
                    "What should the app do? Say who it is for, what they do with it, and \
                     anything it must or must not do.",
                )
                .color(Color::Muted),
            )
            .child(self.description.clone())
    }

    fn render_prompt(&self) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(
                Label::new(
                    "This is what the agent gets. Edit it if you like; Start sends it in a new \
                     thread.",
                )
                .color(Color::Muted),
            )
            .when(self.form.prompt_edited, |this| {
                this.child(
                    Label::new("Edited: going back and changing an answer keeps your edit.")
                        .size(LabelSize::Small)
                        .color(Color::Warning),
                )
            })
            .child(self.prompt.clone())
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> ModalFooter {
        let step = self.form.step;
        let last = STEPS.len() - 1;
        let problem = self
            .error
            .clone()
            .or_else(|| self.form.setup.problem(step.min(last - 1)).map(Into::into));
        let can_continue = problem.is_none();
        let focus_handle = self.focus_handle.clone();
        ModalFooter::new()
            .start_slot::<Label>(problem.map(|problem| {
                Label::new(problem)
                    .size(LabelSize::Small)
                    .color(Color::Error)
            }))
            .end_slot(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("setup-cancel", "Cancel")
                            .key_binding(
                                KeyBinding::for_action_in(&menu::Cancel, &focus_handle, cx)
                                    .map(|binding| binding.size(rems_from_px(12_f32))),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel(&menu::Cancel, window, cx)
                            })),
                    )
                    .when(step > 0, |this| {
                        this.child(Button::new("setup-back", "Back").on_click(cx.listener(
                            move |this, _, window, cx| this.go_to(step - 1, window, cx),
                        )))
                    })
                    .child(
                        Button::new(
                            "setup-next",
                            if step == last { "Start" } else { "Next" },
                        )
                        .style(ButtonStyle::Filled)
                        .disabled(!can_continue)
                        .key_binding(
                            KeyBinding::for_action_in(&menu::Confirm, &focus_handle, cx)
                                .map(|binding| binding.size(rems_from_px(12_f32))),
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.confirm(&menu::Confirm, window, cx)
                        })),
                    ),
            )
    }
}

/// The field's editor, held apart from the field so it can be changed while
/// the field is read.
fn editor(input: &Entity<InputField>, cx: &App) -> Arc<dyn ErasedEditor> {
    input.read(cx).editor().clone()
}

impl ModalView for AppSetupModal {}

impl EventEmitter<DismissEvent> for AppSetupModal {}

impl Focusable for AppSetupModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AppSetupModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.form.step {
            0 => self.render_kind(cx).into_any_element(),
            1 => self.render_language(cx).into_any_element(),
            2 => self.render_package(cx).into_any_element(),
            3 => self.render_description().into_any_element(),
            _ => self.render_prompt().into_any_element(),
        };
        v_flex()
            .key_context("AppSetupModal")
            .track_focus(&self.focus_handle)
            .elevation_3(cx)
            .w(rems(44.))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::confirm))
            .child(
                Modal::new("app-setup", None)
                    .header(
                        ModalHeader::new()
                            .headline("Set Up a New App")
                            .description("Five questions, then the agent builds it."),
                    )
                    .section(Section::new().child(self.render_steps(cx)))
                    .section(
                        Section::new().child(
                            div()
                                .id("app-setup-body")
                                .max_h(vh(0.6, window))
                                .overflow_y_scroll()
                                .track_scroll(&self.scroll_handle)
                                .pr_3()
                                .child(body),
                        ),
                    )
                    .footer(self.render_footer(cx)),
            )
            .vertical_scrollbar_for(&self.scroll_handle, window, cx)
    }
}
