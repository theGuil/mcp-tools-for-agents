//! Tool `delete_file`: remove um arquivo do workspace.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::domains::{McpServer, Runtime};

/// Confirmação da remoção.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeleteFileResult {
    pub deleted: String,
    pub freed_bytes: u64,
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// [`ErrorCode::NotFound`] se o arquivo não existir.
pub fn delete_file(runtime: &Runtime, path: &str) -> ToolResult<DeleteFileResult> {
    let target = runtime.workspace.existing(path)?;
    let size = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
    std::fs::remove_file(&target).map_err(|error| {
        ToolError::new(
            format!("Não foi possível apagar '{path}': {error}"),
            ErrorCode::NotFound,
        )
    })?;
    Ok(DeleteFileResult {
        deleted: runtime.workspace.relative(&target),
        freed_bytes: size,
    })
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Arquivo, relativo ao workspace.
    pub path: String,
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "delete_file",
        "Apaga permanentemente um arquivo do workspace.\n\n\
         Use para limpar saídas intermediárias. Não há lixeira nem desfazer.",
        move |params: Params| guarded(delete_file(&runtime, &params.path)),
    );
}
