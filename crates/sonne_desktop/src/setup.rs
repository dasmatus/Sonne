//! The project setup wizard: five questions about the app a new project builds,
//! answered in a form, and the prompt they add up to, shown for editing before
//! it starts the project's first chat.
//!
//! Everything it draws comes from mcsapi's components and the theme's
//! [`Tokens`], so restyling those restyles the wizard; the answers and the
//! prompt are plain data ([`AppSetup`]) that never depend on how they were
//! asked.

use std::path::PathBuf;

use egui::{Align, Layout, RichText, ScrollArea, Ui};
use mcsapi_components::{
    Alert, Badge, BadgeVariant, Button, ButtonSize, ButtonVariant, Card, Checkbox, Input,
    RadioGroup, Select, Textarea, Tokens, typography,
};
use serde::{Deserialize, Serialize};

/// What a project is set up to build, saved on the project so the wizard can
/// reopen with the same answers.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppSetup {
    /// A LosOS app: one pm build file.
    pub losos: bool,
    pub flatpak: bool,
    pub wasm: bool,
    pub language: Language,
    pub metadata: Metadata,
    /// What the app should do, in the user's words.
    pub description: String,
}

impl Default for AppSetup {
    fn default() -> Self {
        Self {
            losos: true,
            flatpak: false,
            wasm: false,
            language: Language::Rust,
            metadata: Metadata::default(),
            description: String::new(),
        }
    }
}

/// The languages mcsapi supports an app in. Only Rust gets mcsapi's own
/// widgets, Sonne's live preview and WASM builds; the rest are themed from the
/// outside by x2mcsapi.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    #[default]
    Rust,
    TypeScript,
    Web,
    CGtk,
    CppQt,
    PythonGtk,
}

impl Language {
    pub const ALL: [Self; 6] = [
        Self::Rust,
        Self::TypeScript,
        Self::Web,
        Self::CGtk,
        Self::CppQt,
        Self::PythonGtk,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Rust => "Rust (recommended)",
            Self::TypeScript => "TypeScript",
            Self::Web => "HTML, CSS and JavaScript",
            Self::CGtk => "C with GTK 4",
            Self::CppQt => "C++ with Qt 6",
            Self::PythonGtk => "Python with GTK 4",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::TypeScript => "TypeScript",
            Self::Web => "HTML, CSS and JavaScript",
            Self::CGtk => "C with GTK 4",
            Self::CppQt => "C++ with Qt 6 Widgets",
            Self::PythonGtk => "Python with GTK 4 (PyGObject)",
        }
    }

    /// What the user gets, or gives up, by picking this language.
    pub fn note(self) -> &'static str {
        match self {
            Self::Rust => {
                "mcsapi is written in Rust: the app uses its widgets and theme directly, \
                 shows live in Sonne's preview pane, and builds for WASM."
            }
            Self::TypeScript => {
                "mcsapi's Node bindings cover desktop policy (workspaces, layouts); the UI is a \
                 web page that x2mcsapi restyles. No live preview and no WASM build."
            }
            Self::Web => {
                "x2mcsapi restyles the page from the desktop theme. No mcsapi widgets, no live \
                 preview, no WASM build."
            }
            Self::CGtk | Self::PythonGtk => {
                "x2mcsapi restyles GTK from the desktop theme. No mcsapi widgets, no live \
                 preview, no WASM build."
            }
            Self::CppQt => {
                "x2mcsapi restyles Qt Widgets from the desktop theme. No mcsapi widgets, no live \
                 preview, no WASM build."
            }
        }
    }

    /// Sonne's WASM previews are Rust guests of `sonne_preview`.
    pub fn builds_wasm(self) -> bool {
        self == Self::Rust
    }

    fn toolkit(self) -> &'static str {
        match self {
            Self::Rust => {
                "Draw the UI with egui. For the desktop build, make it an mcsapi app: \
                 `mcsapi_ui::App` with the widgets from `mcsapi-components` \
                 (both from https://github.com/dasmatus/mcsapi), so it follows the desktop \
                 theme. Start with Sonne's `preview_new` tool so the app shows in the preview \
                 pane while you work, and check it with `preview_build` and `preview_status`."
            }
            Self::TypeScript => {
                "Run it on Node.js. Use mcsapi's napi-rs bindings (`bindings/node` in \
                 https://github.com/dasmatus/mcsapi) for anything that touches workspaces or \
                 window layout. Build the UI as a web page; x2mcsapi restyles it from the \
                 desktop theme, so keep its styles plain and semantic."
            }
            Self::Web => {
                "Build it as a web page; x2mcsapi restyles it from the desktop theme, so keep \
                 its styles plain and semantic."
            }
            Self::CGtk | Self::PythonGtk => {
                "Use stock GTK 4 widgets without custom CSS: x2mcsapi restyles GTK from the \
                 desktop theme, and LosOS's GTK carries the adaptive patches for phones."
            }
            Self::CppQt => {
                "Use stock Qt Widgets without a custom style sheet: x2mcsapi restyles Qt from \
                 the desktop theme."
            }
        }
    }

    /// The Flatpak runtime that ships this language's toolkit.
    fn runtime(self) -> FlatpakRuntime {
        match self {
            Self::CGtk | Self::PythonGtk => FlatpakRuntime::Gnome,
            Self::CppQt => FlatpakRuntime::Kde,
            Self::Rust | Self::TypeScript | Self::Web => FlatpakRuntime::Freedesktop,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FlatpakRuntime {
    #[default]
    Freedesktop,
    Gnome,
    Kde,
}

impl FlatpakRuntime {
    pub const ALL: [Self; 3] = [Self::Freedesktop, Self::Gnome, Self::Kde];

    pub fn id(self) -> &'static str {
        match self {
            Self::Freedesktop => "org.freedesktop.Platform",
            Self::Gnome => "org.gnome.Platform",
            Self::Kde => "org.kde.Platform",
        }
    }

    fn sdk(self) -> &'static str {
        match self {
            Self::Freedesktop => "org.freedesktop.Sdk",
            Self::Gnome => "org.gnome.Sdk",
            Self::Kde => "org.kde.Sdk",
        }
    }

    /// A version that exists; the user can type a newer one.
    fn default_version(self) -> &'static str {
        match self {
            Self::Freedesktop => "25.08",
            Self::Gnome => "49",
            Self::Kde => "6.9",
        }
    }
}

/// What a WASM build of the app is.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WasmKind {
    /// Draws through `sonne_preview::serve`, so Sonne can show it.
    #[default]
    Preview,
    /// A `wasi:cli` command run by any WASI runtime.
    Command,
}

impl WasmKind {
    pub const ALL: [Self; 2] = [Self::Preview, Self::Command];

    pub fn label(self) -> &'static str {
        match self {
            Self::Preview => "A window Sonne previews (sonne_preview)",
            Self::Command => "A command-line program (wasi:cli)",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Metadata {
    /// The package name every format shares: cargo, pm and the WASM component.
    pub name: String,
    pub display_name: String,
    pub summary: String,
    pub version: String,
    pub license: String,
    pub homepage: String,
    /// Where the app's source goes; empty means `~/src/<name>`.
    pub folder: String,
    /// pm packages the build file depends on, comma separated.
    pub pm_dependencies: String,
    pub flatpak_id: String,
    pub flatpak_runtime: FlatpakRuntime,
    pub flatpak_runtime_version: String,
    pub flatpak_network: bool,
    pub flatpak_home: bool,
    pub flatpak_gpu: bool,
    pub flatpak_audio: bool,
    pub wasm_kind: WasmKind,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            name: String::new(),
            display_name: String::new(),
            summary: String::new(),
            version: "0.1.0".into(),
            license: "AGPL-3.0-or-later".into(),
            homepage: String::new(),
            folder: String::new(),
            pm_dependencies: String::new(),
            flatpak_id: String::new(),
            flatpak_runtime: FlatpakRuntime::Freedesktop,
            flatpak_runtime_version: FlatpakRuntime::Freedesktop.default_version().into(),
            flatpak_network: false,
            flatpak_home: false,
            flatpak_gpu: true,
            flatpak_audio: false,
            wasm_kind: WasmKind::Preview,
        }
    }
}

/// The wizard's steps, in order.
pub const STEPS: [&str; 5] = ["App kind", "Language", "Package", "What it does", "Prompt"];

impl AppSetup {
    pub fn display_name(&self) -> String {
        let display = self.metadata.display_name.trim();
        if !display.is_empty() {
            return display.to_owned();
        }
        self.metadata
            .name
            .split(['-', '_'])
            .filter(|word| !word.is_empty())
            .map(|word| {
                let mut chars = word.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn folder(&self) -> PathBuf {
        let folder = self.metadata.folder.trim();
        if let Some(rest) = folder.strip_prefix("~/")
            && let Some(home) = dirs::home_dir()
        {
            return home.join(rest);
        }
        if !folder.is_empty() {
            return PathBuf::from(folder);
        }
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("src")
            .join(self.metadata.name.trim())
    }

    /// The first thing wrong with the answers up to and including `step`, said
    /// so the user knows what to change.
    pub fn problem(&self, step: usize) -> Option<String> {
        if !self.losos && !self.flatpak && !self.wasm {
            return Some("Pick at least one kind of app.".into());
        }
        if step < 1 {
            return None;
        }
        if self.wasm && !self.language.builds_wasm() {
            return Some(format!(
                "A WASM app has to be written in Rust; {} cannot build one in Sonne.",
                self.language.name()
            ));
        }
        if step < 2 {
            return None;
        }
        let metadata = &self.metadata;
        let name = metadata.name.trim();
        if name.is_empty() {
            return Some("Give the package a name.".into());
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            || name.starts_with('-')
            || name.starts_with(|c: char| c.is_ascii_digit())
        {
            return Some(
                "The package name takes lowercase letters, digits and dashes, starting with a letter."
                    .into(),
            );
        }
        if version_parts(&metadata.version).is_none() {
            return Some("The version is numbers separated by dots, such as 0.1.0.".into());
        }
        if self.flatpak {
            let id = metadata.flatpak_id.trim();
            let segments: Vec<&str> = id.split('.').collect();
            if segments.len() < 3
                || segments.iter().any(|segment| {
                    segment.is_empty()
                        || !segment
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                })
            {
                return Some(
                    "A Flatpak app ID is a reverse domain name with at least three parts, such as org.example.TodoApp."
                        .into(),
                );
            }
            if metadata.flatpak_runtime_version.trim().is_empty() {
                return Some("Say which version of the Flatpak runtime to use.".into());
            }
        }
        if self.folder().is_file() {
            return Some(format!("{} is a file.", self.folder().display()));
        }
        if step < 3 {
            return None;
        }
        if self.description.trim().is_empty() {
            return Some("Say what the app should do.".into());
        }
        None
    }

    /// Sets the language, and the Flatpak runtime with it unless the user
    /// changed the runtime themselves.
    pub fn set_language(&mut self, language: Language) {
        let old = self.language.runtime();
        self.language = language;
        let metadata = &mut self.metadata;
        if metadata.flatpak_runtime == old
            && metadata.flatpak_runtime_version == old.default_version()
        {
            metadata.flatpak_runtime = language.runtime();
            metadata.flatpak_runtime_version = language.runtime().default_version().into();
        }
    }

    /// What every later chat in the project is told about the app.
    pub fn instructions(&self) -> String {
        let mut kinds = Vec::new();
        if self.losos {
            kinds.push("a LosOS app with a pm build file".to_owned());
        }
        if self.flatpak {
            kinds.push(format!("a Flatpak ({})", self.metadata.flatpak_id.trim()));
        }
        if self.wasm {
            kinds.push("a WASM app (wasm32-wasip2)".to_owned());
        }
        format!(
            "This project builds {} (package `{}`), in {}, written in {}. It ships as {}.",
            self.display_name(),
            self.metadata.name.trim(),
            self.folder().display(),
            self.language.name(),
            join_list(&kinds),
        )
    }

    /// The prompt the answers add up to: everything the agent needs to start,
    /// in the order it will do it.
    pub fn prompt(&self) -> String {
        let metadata = &self.metadata;
        let name = metadata.name.trim();
        let version = metadata.version.trim();
        let mut prompt = format!(
            "Build a new app, {}, in {}.\n\n",
            self.display_name(),
            self.folder().display()
        );
        prompt.push_str("## What it should do\n\n");
        prompt.push_str(self.description.trim());
        prompt.push_str("\n\n## Language\n\n");
        prompt.push_str(&format!(
            "Write it in {}. {}\n\n",
            self.language.name(),
            self.language.toolkit()
        ));

        prompt.push_str("## Package metadata\n\n");
        prompt.push_str(&format!("- Package name: `{name}`\n"));
        prompt.push_str(&format!("- Display name: {}\n", self.display_name()));
        if !metadata.summary.trim().is_empty() {
            prompt.push_str(&format!("- Summary: {}\n", metadata.summary.trim()));
        }
        prompt.push_str(&format!("- Version: {version}\n"));
        if !metadata.license.trim().is_empty() {
            prompt.push_str(&format!("- Licence: {}\n", metadata.license.trim()));
        }
        if !metadata.homepage.trim().is_empty() {
            prompt.push_str(&format!("- Homepage: {}\n", metadata.homepage.trim()));
        }
        prompt.push('\n');

        prompt.push_str("## Packaging\n\n");
        if self.losos {
            let parts = version_parts(version).unwrap_or_default();
            let dependencies: Vec<String> = metadata
                .pm_dependencies
                .split(',')
                .map(str::trim)
                .filter(|dependency| !dependency.is_empty())
                .map(|dependency| format!("'{dependency}'"))
                .collect();
            prompt.push_str(&format!(
                "- **LosOS app.** Write a pm build file, `{name}.yaml`, in the app's folder, with \
                 `name: {name}`, `version: [{}]` and `dependencies: [{}]`, a `Build` stage that \
                 compiles the app and an `Install` stage that puts it under `/dest/usr` \
                 (`usr/bin/{name}`, plus a `.desktop` file and icon under `usr/share`). pm runs \
                 each step in a jail with no shell, `/nix/store` read-only and network only for \
                 build files that need it, so add `--offline` wherever a step can work without \
                 it. Check the file with `pm_explain`, then build it with `pm_build`.\n",
                parts
                    .iter()
                    .map(|part| format!("'{part}'"))
                    .collect::<Vec<_>>()
                    .join(", "),
                dependencies.join(", "),
            ));
        }
        if self.flatpak {
            let runtime = metadata.flatpak_runtime;
            let id = metadata.flatpak_id.trim();
            let mut finish_args = vec!["--socket=wayland", "--socket=fallback-x11", "--share=ipc"];
            if metadata.flatpak_gpu {
                finish_args.push("--device=dri");
            }
            if metadata.flatpak_network {
                finish_args.push("--share=network");
            }
            if metadata.flatpak_home {
                finish_args.push("--filesystem=home");
            }
            if metadata.flatpak_audio {
                finish_args.push("--socket=pulseaudio");
            }
            prompt.push_str(&format!(
                "- **Flatpak.** Write a flatpak-builder manifest, `{id}.yml`, with app ID `{id}`, \
                 runtime `{}` version `{}`, SDK `{}` and whatever SDK extension {} needs, and \
                 exactly these finish-args: {}. Ask before adding any other permission. Add \
                 `{id}.desktop` and `{id}.metainfo.xml` (summary, licence, version {version}) \
                 so Bazaar and Flathub can list it.\n",
                runtime.id(),
                metadata.flatpak_runtime_version.trim(),
                runtime.sdk(),
                self.language.name(),
                finish_args
                    .iter()
                    .map(|arg| format!("`{arg}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        if self.wasm {
            let what = match metadata.wasm_kind {
                WasmKind::Preview => {
                    "a window drawn through `sonne_preview::serve`, so Sonne's preview pane \
                     can run it: build it with `preview_build` and target `wasm`"
                }
                WasmKind::Command => {
                    "a `wasi:cli` command any WASI runtime can run, built with \
                     `cargo build --target wasm32-wasip2`"
                }
            };
            prompt.push_str(&format!(
                "- **WASM app.** Build `{name}` for `wasm32-wasip2` as {what}. It runs \
                 sandboxed with no file or network access, so keep everything it needs in \
                 memory.\n"
            ));
        }
        prompt.push_str(
            "\nWhen it works, tell me what you built, how to run it, and how to install each \
             package.",
        );
        prompt
    }
}

/// `0.1.0` as `["0", "1", "0"]`, the list a pm build file's `version` takes.
fn version_parts(version: &str) -> Option<Vec<String>> {
    let parts: Vec<String> = version.trim().split('.').map(str::to_owned).collect();
    let valid = parts
        .iter()
        .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
    valid.then_some(parts)
}

fn join_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [first @ .., last] => format!("{} and {last}", first.join(", ")),
    }
}

/// The wizard while it is open.
pub struct SetupForm {
    pub step: usize,
    pub setup: AppSetup,
    /// The prompt as shown in the last step; the user may have edited it.
    pub prompt: String,
    /// Whether `prompt` was edited since it was composed, so going back and
    /// changing an answer does not throw the edit away unasked.
    pub prompt_edited: bool,
    /// The project being set up again, or `None` for a new one.
    pub project_id: Option<String>,
}

pub enum SetupAction {
    Cancel,
    /// Create or update the project and send the prompt.
    Start,
}

impl SetupForm {
    pub fn new(setup: AppSetup, project_id: Option<String>) -> Self {
        let prompt = setup.prompt();
        Self {
            step: 0,
            setup,
            prompt,
            prompt_edited: false,
            project_id,
        }
    }

    fn go_to(&mut self, step: usize) {
        if step == STEPS.len() - 1 && !self.prompt_edited {
            self.prompt = self.setup.prompt();
        }
        self.step = step;
    }

    /// The furthest step the answers so far let the user reach.
    fn reachable(&self) -> usize {
        (0..STEPS.len() - 1)
            .find(|step| self.setup.problem(*step).is_some())
            .unwrap_or(STEPS.len() - 1)
    }

    pub fn show(&mut self, ui: &mut Ui, tokens: &Tokens) -> Option<SetupAction> {
        let mut action = None;
        let width = ui.available_width().min(720.0);
        let bare = egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 6));
        egui::Panel::top("setup-steps").frame(bare).show(ui, |ui| {
            column(ui, width, |ui| {
                ui.horizontal(|ui| {
                    let title = if self.project_id.is_some() {
                        "Set up the project's app"
                    } else {
                        "New project"
                    };
                    ui.label(typography::h3(tokens, title));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                Button::new("Cancel")
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Sm),
                            )
                            .clicked()
                        {
                            action = Some(SetupAction::Cancel);
                        }
                    });
                });
                ui.add_space(8.0);
                self.stepper(ui, tokens);
            });
        });
        // The buttons stay put below the card, however tall a step grows.
        let problem = self.setup.problem(self.step.min(STEPS.len() - 2));
        egui::Panel::bottom("setup-buttons")
            .frame(bare)
            .show(ui, |ui| {
                column(ui, width, |ui| {
                    if let Some(problem) = &problem {
                        ui.label(typography::small(tokens, problem).color(tokens.destructive));
                        ui.add_space(4.0);
                    }
                    ui.horizontal(|ui| {
                        if self.step > 0
                            && ui
                                .add(Button::new("Back").variant(ButtonVariant::Outline))
                                .clicked()
                        {
                            self.go_to(self.step - 1);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if self.step + 1 < STEPS.len() {
                                if ui
                                    .add(Button::new("Next").enabled(problem.is_none()))
                                    .clicked()
                                {
                                    self.go_to(self.step + 1);
                                }
                            } else if ui
                                .add(
                                    Button::new("Create project and start").enabled(
                                        problem.is_none() && !self.prompt.trim().is_empty(),
                                    ),
                                )
                                .clicked()
                            {
                                action = Some(SetupAction::Start);
                            }
                        });
                    });
                });
            });
        egui::CentralPanel::no_frame().show(ui, |ui| {
            ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                ui.set_max_width(width);
                Card::new().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    match self.step {
                        0 => self.kind_step(ui, tokens),
                        1 => self.language_step(ui, tokens),
                        2 => self.metadata_step(ui, tokens),
                        3 => self.description_step(ui, tokens),
                        _ => self.prompt_step(ui, tokens),
                    }
                });
            });
        });
        action
    }

    fn stepper(&mut self, ui: &mut Ui, tokens: &Tokens) {
        let reachable = self.reachable();
        ui.horizontal_wrapped(|ui| {
            for (index, title) in STEPS.iter().enumerate() {
                let variant = if index == self.step {
                    ButtonVariant::Default
                } else if index < self.step {
                    ButtonVariant::Secondary
                } else {
                    ButtonVariant::Ghost
                };
                let text = format!("{} {title}", index + 1);
                if ui
                    .add(
                        Button::new(text)
                            .variant(variant)
                            .size(ButtonSize::Sm)
                            .enabled(index <= reachable),
                    )
                    .clicked()
                {
                    self.go_to(index);
                }
                if index + 1 < STEPS.len() {
                    ui.label(RichText::new("›").color(tokens.muted_foreground));
                }
            }
        });
    }

    fn kind_step(&mut self, ui: &mut Ui, tokens: &Tokens) {
        ui.label(typography::h4(tokens, "What kind of app is it?"));
        ui.label(typography::muted(tokens, "Pick one or more."));
        ui.add_space(8.0);
        let setup = &mut self.setup;
        ui.add(Checkbox::new(&mut setup.losos).label("A LosOS app"));
        hint(ui, tokens, "The agent writes a pm build file for it.");
        ui.add(Checkbox::new(&mut setup.flatpak).label("A Flatpak"));
        hint(
            ui,
            tokens,
            "The agent writes a flatpak-builder manifest, installable from Bazaar.",
        );
        ui.add(Checkbox::new(&mut setup.wasm).label("A WASM app"));
        hint(
            ui,
            tokens,
            "Built for wasm32-wasip2 and run sandboxed; Sonne can preview it live.",
        );
    }

    fn language_step(&mut self, ui: &mut Ui, tokens: &Tokens) {
        ui.label(typography::h4(
            tokens,
            "Which language should the agent write it in?",
        ));
        ui.label(typography::muted(
            tokens,
            "These are the languages mcsapi supports. Rust is strongly recommended.",
        ));
        ui.add_space(8.0);
        let labels: Vec<&str> = Language::ALL
            .iter()
            .map(|language| language.label())
            .collect();
        let mut selected = Language::ALL
            .iter()
            .position(|language| *language == self.setup.language)
            .unwrap_or(0);
        if ui.add(RadioGroup::new(&mut selected, &labels)).changed()
            && let Some(language) = Language::ALL.get(selected)
        {
            self.setup.set_language(*language);
        }
        ui.add_space(8.0);
        let language = self.setup.language;
        if language == Language::Rust {
            ui.label(typography::muted(tokens, language.note()));
        } else {
            ui.add(
                Alert::new(format!("{} is not the recommended choice", language.name()))
                    .description(format!(
                        "{} Rust gets all of that and is what Sonne's tools are built around.",
                        language.note()
                    )),
            );
        }
    }

    fn metadata_step(&mut self, ui: &mut Ui, tokens: &Tokens) {
        ui.label(typography::h4(tokens, "Package metadata"));
        ui.label(typography::muted(
            tokens,
            "Shared by every package below; the agent copies it into each format.",
        ));
        ui.add_space(8.0);
        let folder_hint = self.setup.folder().display().to_string();
        let display_hint = self.setup.display_name();
        let metadata = &mut self.setup.metadata;
        field(ui, tokens, "Package name", |ui| {
            ui.add(
                Input::new(&mut metadata.name)
                    .placeholder("todo-app")
                    .width(320.0),
            );
        });
        field(ui, tokens, "Display name", |ui| {
            let placeholder = if display_hint.is_empty() {
                "Todo App".to_owned()
            } else {
                display_hint
            };
            ui.add(
                Input::new(&mut metadata.display_name)
                    .placeholder(placeholder)
                    .width(320.0),
            );
        });
        field(ui, tokens, "Summary", |ui| {
            ui.add(
                Input::new(&mut metadata.summary)
                    .placeholder("One line for app stores")
                    .width(480.0),
            );
        });
        field(ui, tokens, "Version", |ui| {
            ui.add(Input::new(&mut metadata.version).width(120.0));
        });
        field(ui, tokens, "Licence", |ui| {
            ui.add(
                Input::new(&mut metadata.license)
                    .placeholder("SPDX expression")
                    .width(240.0),
            );
        });
        field(ui, tokens, "Homepage", |ui| {
            ui.add(
                Input::new(&mut metadata.homepage)
                    .placeholder("https://…")
                    .width(480.0),
            );
        });
        field(ui, tokens, "Folder", |ui| {
            ui.add(
                Input::new(&mut metadata.folder)
                    .placeholder(folder_hint)
                    .width(480.0),
            );
        });

        if self.setup.losos {
            ui.add_space(12.0);
            ui.label(typography::large(tokens, "pm"));
            let metadata = &mut self.setup.metadata;
            field(ui, tokens, "Dependencies", |ui| {
                ui.add(
                    Input::new(&mut metadata.pm_dependencies)
                        .placeholder("pm packages, comma separated")
                        .width(480.0),
                );
            });
        }
        if self.setup.flatpak {
            ui.add_space(12.0);
            ui.label(typography::large(tokens, "Flatpak"));
            let metadata = &mut self.setup.metadata;
            field(ui, tokens, "App ID", |ui| {
                ui.add(
                    Input::new(&mut metadata.flatpak_id)
                        .placeholder("org.example.TodoApp")
                        .width(320.0),
                );
            });
            field(ui, tokens, "Runtime", |ui| {
                let runtimes: Vec<&str> = FlatpakRuntime::ALL
                    .iter()
                    .map(|runtime| runtime.id())
                    .collect();
                let mut selected = FlatpakRuntime::ALL
                    .iter()
                    .position(|runtime| *runtime == metadata.flatpak_runtime);
                if ui
                    .add(Select::new("flatpak-runtime", &mut selected, &runtimes).width(240.0))
                    .changed()
                    && let Some(runtime) = selected.and_then(|index| FlatpakRuntime::ALL.get(index))
                {
                    metadata.flatpak_runtime = *runtime;
                    metadata.flatpak_runtime_version = runtime.default_version().into();
                }
                ui.add(Input::new(&mut metadata.flatpak_runtime_version).width(80.0));
            });
            field(ui, tokens, "Permissions", |ui| {
                ui.vertical(|ui| {
                    ui.add(Checkbox::new(&mut metadata.flatpak_gpu).label("GPU"));
                    ui.add(Checkbox::new(&mut metadata.flatpak_network).label("Network"));
                    ui.add(Checkbox::new(&mut metadata.flatpak_home).label("Home folder"));
                    ui.add(Checkbox::new(&mut metadata.flatpak_audio).label("Audio"));
                });
            });
        }
        if self.setup.wasm {
            ui.add_space(12.0);
            ui.label(typography::large(tokens, "WASM"));
            let metadata = &mut self.setup.metadata;
            field(ui, tokens, "Build", |ui| {
                let labels: Vec<&str> = WasmKind::ALL.iter().map(|kind| kind.label()).collect();
                let mut selected = WasmKind::ALL
                    .iter()
                    .position(|kind| *kind == metadata.wasm_kind)
                    .unwrap_or(0);
                if ui.add(RadioGroup::new(&mut selected, &labels)).changed()
                    && let Some(kind) = WasmKind::ALL.get(selected)
                {
                    metadata.wasm_kind = *kind;
                }
            });
        }
    }

    fn description_step(&mut self, ui: &mut Ui, tokens: &Tokens) {
        ui.label(typography::h4(tokens, "What should the app do?"));
        ui.label(typography::muted(
            tokens,
            "Describe it the way you would to a person: what it is for, what is on screen, what happens when you use it.",
        ));
        ui.add_space(8.0);
        ui.add(
            Textarea::new(&mut self.setup.description)
                .placeholder(
                    "A todo list with due dates. Items can be filtered by done, today and overdue…",
                )
                .rows(10),
        );
    }

    fn prompt_step(&mut self, ui: &mut Ui, tokens: &Tokens) {
        ui.horizontal(|ui| {
            ui.label(typography::h4(tokens, "The prompt"));
            if self.prompt_edited {
                ui.add(Badge::new("Edited").variant(BadgeVariant::Secondary));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.prompt_edited
                    && ui
                        .add(
                            Button::new("Rebuild from answers")
                                .variant(ButtonVariant::Outline)
                                .size(ButtonSize::Sm),
                        )
                        .clicked()
                {
                    self.prompt = self.setup.prompt();
                    self.prompt_edited = false;
                }
            });
        });
        ui.label(typography::muted(
            tokens,
            "This is what the agent gets as the project's first message. Edit it freely.",
        ));
        ui.add_space(8.0);
        if ui.add(Textarea::new(&mut self.prompt).rows(22)).changed() {
            self.prompt_edited = true;
        }
    }
}

/// A column `width` wide at the left of `ui`, so the steps, the card and
/// the buttons line up.
fn column(ui: &mut Ui, width: f32, content: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        Layout::top_down(Align::Min),
        content,
    );
}

fn hint(ui: &mut Ui, tokens: &Tokens, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(26.0);
        ui.label(typography::small(tokens, text).color(tokens.muted_foreground));
    });
    ui.add_space(6.0);
}

fn field(ui: &mut Ui, tokens: &Tokens, label: &str, content: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [120.0, 20.0],
            egui::Label::new(typography::small(tokens, label).color(tokens.muted_foreground)),
        );
        content(ui);
    });
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn todo() -> AppSetup {
        let mut setup = AppSetup::default();
        setup.metadata.name = "todo-app".into();
        setup.metadata.folder = "/tmp/todo-app".into();
        setup.description = "A todo list.".into();
        setup
    }

    #[test]
    fn each_step_names_what_is_missing() {
        let mut setup = AppSetup {
            losos: false,
            ..AppSetup::default()
        };
        assert!(setup.problem(0).is_some());
        setup.wasm = true;
        assert_eq!(setup.problem(0), None);
        setup.set_language(Language::Web);
        assert!(setup.problem(1).unwrap().contains("Rust"));
        setup.set_language(Language::Rust);
        assert!(setup.problem(2).unwrap().contains("name"));
        setup.metadata.name = "Todo App".into();
        assert!(setup.problem(2).unwrap().contains("lowercase"));
        setup.metadata.name = "todo-app".into();
        setup.metadata.version = "1.x".into();
        assert!(setup.problem(2).unwrap().contains("version"));
        setup.metadata.version = "1.0".into();
        assert_eq!(setup.problem(2), None);
        assert!(setup.problem(3).unwrap().contains("do"));
        setup.description = "Lists todos".into();
        assert_eq!(setup.problem(3), None);
    }

    #[test]
    fn flatpak_needs_a_reverse_domain_id() {
        let mut setup = todo();
        setup.flatpak = true;
        assert!(setup.problem(2).unwrap().contains("app ID"));
        setup.metadata.flatpak_id = "org.example".into();
        assert!(setup.problem(2).is_some());
        setup.metadata.flatpak_id = "org.example.TodoApp".into();
        assert_eq!(setup.problem(3), None);
    }

    #[test]
    fn display_name_falls_back_to_the_package_name() {
        let mut setup = todo();
        assert_eq!(setup.display_name(), "Todo App");
        setup.metadata.display_name = "Todos".into();
        assert_eq!(setup.display_name(), "Todos");
    }

    #[test]
    fn language_picks_the_runtime_until_the_user_does() {
        let mut setup = todo();
        setup.set_language(Language::CppQt);
        assert_eq!(setup.metadata.flatpak_runtime, FlatpakRuntime::Kde);
        assert_eq!(setup.metadata.flatpak_runtime_version, "6.9");
        setup.metadata.flatpak_runtime_version = "6.10".into();
        setup.set_language(Language::CGtk);
        assert_eq!(setup.metadata.flatpak_runtime, FlatpakRuntime::Kde);
    }

    #[test]
    fn prompt_carries_every_answer() {
        let mut setup = todo();
        setup.flatpak = true;
        setup.wasm = true;
        setup.metadata.flatpak_id = "org.example.TodoApp".into();
        setup.metadata.flatpak_network = true;
        setup.metadata.pm_dependencies = "ripgrep, fd".into();
        setup.metadata.summary = "Keeps track of things".into();
        let prompt = setup.prompt();
        assert!(prompt.contains("Build a new app, Todo App, in /tmp/todo-app."));
        assert!(prompt.contains("A todo list."));
        assert!(prompt.contains("Write it in Rust."));
        assert!(prompt.contains("`todo-app.yaml`"));
        assert!(prompt.contains("version: ['0', '1', '0']"));
        assert!(prompt.contains("dependencies: ['ripgrep', 'fd']"));
        assert!(prompt.contains("`org.example.TodoApp.yml`"));
        assert!(prompt.contains("`--share=network`"));
        assert!(!prompt.contains("--filesystem=home"));
        assert!(prompt.contains("wasm32-wasip2"));
        assert!(prompt.contains("Summary: Keeps track of things"));
    }

    #[test]
    fn prompt_leaves_out_kinds_not_picked() {
        let prompt = todo().prompt();
        assert!(prompt.contains("LosOS app"));
        assert!(!prompt.contains("Flatpak"));
        assert!(!prompt.contains("WASM"));
    }

    #[test]
    fn editing_the_prompt_survives_going_back() {
        let mut form = SetupForm::new(todo(), None);
        form.go_to(4);
        form.prompt = "my own words".into();
        form.prompt_edited = true;
        form.go_to(3);
        form.setup.description = "Something else".into();
        form.go_to(4);
        assert_eq!(form.prompt, "my own words");
    }

    #[test]
    fn instructions_name_the_app_and_its_packages() {
        let mut setup = todo();
        setup.flatpak = true;
        setup.metadata.flatpak_id = "org.example.TodoApp".into();
        assert_eq!(
            setup.instructions(),
            "This project builds Todo App (package `todo-app`), in /tmp/todo-app, written in \
             Rust. It ships as a LosOS app with a pm build file and a Flatpak (org.example.TodoApp)."
        );
    }

    #[test]
    fn old_projects_without_a_setup_still_load() -> anyhow::Result<()> {
        let setup: AppSetup = serde_json::from_str(r#"{"flatpak": true}"#)?;
        assert!(setup.losos && setup.flatpak);
        assert_eq!(setup.metadata.version, "0.1.0");
        Ok(())
    }
}
