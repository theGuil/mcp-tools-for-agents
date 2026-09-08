//! Tool `job_status`: consulta o andamento de um job.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::core::errors::{guarded, ToolResult};
use crate::core::jobs::JobStatusPayload;
use crate::domains::{McpServer, Runtime};

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// [`crate::core::errors::ErrorCode::JobNotFound`] se o id não existir.
pub fn job_status(runtime: &Runtime, job_id: &str) -> ToolResult<JobStatusPayload> {
    Ok(runtime.jobs.get(job_id)?.to_payload())
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Id devolvido pela tool que iniciou o job.
    pub job_id: String,
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "job_status",
        "Informa se um job em background está pending, running, done ou failed.\n\n\
         Quando estiver done, chame job_result para obter a saída.",
        move |params: Params| guarded(job_status(&runtime, &params.job_id)),
    );
}
