//! Tools de arquivos do workspace.

use std::sync::Arc;

use crate::domains::{McpServer, Runtime};

pub mod delete;
pub mod listing;

/// Registra todas as tools do domínio.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    listing::register(mcp, runtime);
    delete::register(mcp, runtime);
}
