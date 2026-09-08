//! Tools de mídia da internet: vídeo de qualquer URL e efeitos sonoros do Freesound.

use std::sync::Arc;

use crate::domains::{McpServer, Runtime};

pub mod download;
pub mod download_sfx;
pub mod info;
pub mod search_sfx;

/// Registra todas as tools do domínio.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    info::register(mcp, runtime);
    download::register(mcp, runtime);
    search_sfx::register(mcp, runtime);
    download_sfx::register(mcp, runtime);
}
