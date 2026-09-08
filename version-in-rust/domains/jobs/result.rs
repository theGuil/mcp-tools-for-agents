//! Tool `job_result`: devolve a saída de um job concluído.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ToolResult};
use crate::core::jobs::JobResult;
use crate::domains::{McpServer, Runtime};

/// Saída de um job, no mesmo formato que a tool síncrona devolveria.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JobResultPayload {
    pub job_id: String,
    pub tool: String,
    pub result: JobResult,
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Se o job não existir, ainda rodar ou tiver falhado.
pub fn job_result(runtime: &Runtime, job_id: &str) -> ToolResult<JobResultPayload> {
    let job = runtime.jobs.get(job_id)?;
    Ok(JobResultPayload {
        job_id: job.id,
        tool: job.tool,
        result: runtime.jobs.result(job_id)?,
    })
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
        "job_result",
        "Obtém o resultado de um job que já terminou.\n\n\
         Retorna erro job_not_finished se ainda estiver rodando.",
        move |params: Params| guarded(job_result(&runtime, &params.job_id)),
    );
}
