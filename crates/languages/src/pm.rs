use anyhow::{Result, bail};
use async_trait::async_trait;
use gpui::AsyncApp;
pub use language::*;
use lsp::{LanguageServerBinary, LanguageServerName};
use std::{future::Future, path::PathBuf, sync::Arc};

/// pm's language server for `.rhai` recipes, the one pm's own Zed extension
/// starts. It ships with pm, so Sonne only looks for it on `PATH` and never
/// downloads one.
pub struct PmLspAdapter;

impl PmLspAdapter {
    const SERVER_NAME: LanguageServerName = LanguageServerName::new_static("pm-lsp");
}

impl LspInstaller for PmLspAdapter {
    type BinaryVersion = ();

    async fn check_if_user_installed(
        &self,
        delegate: &Arc<dyn LspAdapterDelegate>,
        _: Option<Toolchain>,
        _: &AsyncApp,
    ) -> Option<LanguageServerBinary> {
        let path = delegate.which(Self::SERVER_NAME.as_ref()).await?;
        Some(LanguageServerBinary {
            path,
            arguments: Vec::new(),
            env: None,
        })
    }

    async fn fetch_latest_server_version(
        &self,
        _: &Arc<dyn LspAdapterDelegate>,
        _: bool,
        _: &mut AsyncApp,
    ) -> Result<()> {
        bail!("pm-lsp was not found on PATH; it is installed with pm")
    }

    fn fetch_server_binary(
        &self,
        _: (),
        _: PathBuf,
        _: &Arc<dyn LspAdapterDelegate>,
    ) -> impl Send + Future<Output = Result<LanguageServerBinary>> + use<> {
        async { bail!("pm-lsp was not found on PATH; it is installed with pm") }
    }

    async fn cached_server_binary(
        &self,
        _: PathBuf,
        _: &dyn LspAdapterDelegate,
    ) -> Option<LanguageServerBinary> {
        None
    }
}

#[async_trait(?Send)]
impl LspAdapter for PmLspAdapter {
    fn name(&self) -> LanguageServerName {
        Self::SERVER_NAME
    }
}
