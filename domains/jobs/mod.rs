//! Tools para acompanhar tarefas em background.

use std::sync::Arc;

use crate::domains::{McpServer, Runtime};

pub mod result;
pub mod status;

/// Registra todas as tools do domínio.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    status::register(mcp, runtime);
    result::register(mcp, runtime);
}
