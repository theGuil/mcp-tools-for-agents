//! Tools de áudio: extração, transcrição e normalização.

use std::sync::Arc;

use crate::domains::{McpServer, Runtime};

pub mod extract;
pub mod normalize;
pub mod transcribe;

/// Registra todas as tools do domínio.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    extract::register(mcp, runtime);
    transcribe::register(mcp, runtime);
    normalize::register(mcp, runtime);
}
