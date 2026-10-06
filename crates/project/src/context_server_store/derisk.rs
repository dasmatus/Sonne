use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use context_server::ContextServerCommand;
use extension::ContextServerConfiguration;
use gpui::{App, AsyncApp, Entity, Task};

use crate::worktree_store::WorktreeStore;

use super::registry::{ContextServerDescriptor, ContextServerDescriptorRegistry};

/// The id `derisk mcp` is listed under in the agent's settings, where it can
/// be turned off like any other server.
pub const SERVER_ID: &str = "derisk";

/// On a derisk desktop, lets the agent see and drive it: `derisk mcp` serves
/// the session's agent tools (windows, apps, the accessibility tree, input)
/// over MCP. The agent, whichever model key or agent CLI is set up for it,
/// gets the desktop the way it gets any other context server, and derisk's
/// command palette hands requests its own assistant can't follow to it.
pub fn init(cx: &mut App) {
    let Ok(path) = which::which("derisk") else {
        return;
    };
    ContextServerDescriptorRegistry::default_global(cx).update(cx, |registry, cx| {
        registry.register_context_server_descriptor(
            SERVER_ID.into(),
            Arc::new(DeriskDescriptor { path }),
            cx,
        )
    });
}

struct DeriskDescriptor {
    path: PathBuf,
}

impl ContextServerDescriptor for DeriskDescriptor {
    fn command(
        &self,
        _: Entity<WorktreeStore>,
        _: &AsyncApp,
    ) -> Task<Result<ContextServerCommand>> {
        Task::ready(Ok(ContextServerCommand {
            path: self.path.clone(),
            args: vec!["mcp".to_owned()],
            env: None,
            timeout: None,
        }))
    }

    fn configuration(
        &self,
        _: Entity<WorktreeStore>,
        _: &AsyncApp,
    ) -> Task<Result<Option<ContextServerConfiguration>>> {
        Task::ready(Ok(None))
    }
}
