//! `sonne` opens the window. Its subcommands are what the window and its
//! agents start: `sonne mcp` (Sonne's MCP server for one chat), `sonne routine
//! run <id>` (a routine's systemd service) and `sonne preview-wasm <file>` (a
//! wasm preview's sandbox).

use anyhow::{Context as _, Result, bail};
use mcsapi_ui::{App as _, Theme};
use sonne_desktop::{Sonne, derisk_theme, routines, store::Store, tools::ToolContext};

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => window(),
        Some("mcp") => {
            let flag = |name: &str| {
                args.iter()
                    .position(|arg| arg == name)
                    .and_then(|index| args.get(index + 1))
                    .cloned()
            };
            sonne_desktop::mcp::serve_stdio(&ToolContext {
                store: Store::open_default()?,
                project_id: flag("--project").context("sonne mcp needs --project <id>")?,
                chat_id: flag("--chat"),
            })
        }
        Some("routine") => match (args.get(1).map(String::as_str), args.get(2)) {
            (Some("run"), Some(id)) => {
                let chat = routines::run(&Store::open_default()?, id)?;
                println!("{chat}");
                Ok(())
            }
            _ => bail!("usage: sonne routine run <id>"),
        },
        Some("preview-wasm") => {
            let component = args
                .get(1)
                .context("usage: sonne preview-wasm <component.wasm>")?;
            run_wasm(component.as_ref())
        }
        Some("--help" | "-h") => {
            println!(
                "usage: sonne                       open the window\n       \
                 sonne mcp --project <id> [--chat <id>]\n       \
                 sonne routine run <id>\n       \
                 sonne preview-wasm <component.wasm>"
            );
            Ok(())
        }
        Some(other) => bail!("unknown command {other}; see sonne --help"),
    }
}

#[cfg(feature = "wasm")]
fn run_wasm(component: &std::path::Path) -> Result<()> {
    sonne_preview::wasm::run(component)
}

#[cfg(not(feature = "wasm"))]
fn run_wasm(_component: &std::path::Path) -> Result<()> {
    bail!("this sonne was built without wasm previews")
}

struct Window {
    sonne: Sonne,
    theme: Theme,
}

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::no_frame().show(ui, |ui| self.sonne.ui(ui, &self.theme));
    }
}

fn window() -> Result<()> {
    let store = Store::open_default()?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Sonne")
            .with_app_id("org.derisk.sonne")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Sonne",
        options,
        Box::new(move |_| {
            Ok(Box::new(Window {
                sonne: Sonne::new(store),
                theme: derisk_theme().unwrap_or_default(),
            }))
        }),
    )
    .map_err(|error| anyhow::anyhow!("{error}"))
}
