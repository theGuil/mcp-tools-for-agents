//! Tool `probe_video`: lê metadados de um vídeo sem alterá-lo.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ToolResult};
use crate::core::ffmpeg::ProbeResult;
use crate::domains::{McpServer, Runtime};

/// Metadados do vídeo mais o caminho consultado.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProbeVideoResult {
    pub path: String,
    pub info: ProbeResult,
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Se o arquivo não existir ou o ffprobe falhar.
pub fn probe_video(runtime: &Runtime, path: &str) -> ToolResult<ProbeVideoResult> {
    let source = runtime.workspace.existing(path)?;
    Ok(ProbeVideoResult {
        path: runtime.workspace.relative(&source),
        info: runtime.ffmpeg.probe(&source)?,
    })
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Caminho do vídeo, relativo ao workspace.
    pub path: String,
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "probe_video",
        "Lê duração, resolução, fps, codecs e tamanho de um vídeo.\n\n\
         Use antes de cortar ou concatenar para conhecer o arquivo. Não altera nada.",
        move |params: Params| guarded(probe_video(&runtime, &params.path)),
    );
}
