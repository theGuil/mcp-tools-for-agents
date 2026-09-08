//! Domínios de tools. Cada submódulo expõe `register(mcp, runtime)`.
//!
//! Regras:
//! - Um arquivo por tool.
//! - Domínio não importa outro domínio. O que é comum vai para `core`.
//! - Tool valida entrada, chama o `core` e devolve uma struct tipada.
//!
//! Aqui também mora [`McpServer`], a fachada sobre o SDK `rmcp` que toda tool
//! usa para se registrar: é o equivalente do `MCPServer` que a versão Python
//! importa do SDK oficial.

pub mod audio;
pub mod files;
pub mod jobs;
pub mod media;
pub mod video;

use std::sync::Arc;

use rmcp::handler::server::common::schema_for_type;
use rmcp::handler::server::router::tool::{ToolRoute, ToolRouter};
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{CallToolResult, Content, ErrorData, Tool};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::config::{DomainName, Settings};
use crate::core::downloader::Downloader;
use crate::core::ffmpeg::FFmpeg;
use crate::core::freesound::Freesound;
use crate::core::jobs::JobManager;
use crate::core::paths::Workspace;

/// Dependências compartilhadas que toda tool recebe.
#[derive(Debug)]
pub struct Runtime {
    pub settings: Settings,
    pub workspace: Workspace,
    pub ffmpeg: FFmpeg,
    pub jobs: JobManager,
    pub downloader: Downloader,
    pub freesound: Freesound,
}

/// Servidor MCP: nome, instruções e a tabela de tools registradas.
///
/// A docstring de cada tool (a `description` passada a [`McpServer::tool`]) e
/// os comentários dos campos da struct de parâmetros são o que o agente lê
/// para decidir usar a tool. Escreva para o modelo, não para o dev.
pub struct McpServer {
    pub name: String,
    pub instructions: String,
    router: ToolRouter<McpServer>,
}

impl std::fmt::Debug for McpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServer")
            .field("name", &self.name)
            .field("tools", &self.tool_names())
            .finish()
    }
}

impl McpServer {
    /// Cria o servidor sem tools.
    pub fn new(name: impl Into<String>, instructions: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            instructions: instructions.into(),
            router: ToolRouter::new(),
        }
    }

    /// Registra uma tool. Equivalente do decorator `@mcp.tool(name=...)`.
    ///
    /// * `P`: struct de parâmetros (`Deserialize` + `JsonSchema`); os `///` de
    ///   cada campo viram a descrição do argumento no schema.
    /// * `R`: retorno serializável, normalmente `Guarded<Resultado>`.
    /// * `handler`: função síncrona; roda em thread de bloqueio, então pode
    ///   chamar ffmpeg à vontade.
    pub fn tool<P, R, F>(&mut self, name: &'static str, description: &str, handler: F)
    where
        P: DeserializeOwned + JsonSchema + Send + 'static,
        R: Serialize + Send + 'static,
        F: Fn(P) -> R + Send + Sync + 'static,
    {
        let attr = Tool::new(name, description.to_string(), schema_for_type::<P>());
        let handler = Arc::new(handler);
        let route = ToolRoute::new_dyn(attr, move |context: ToolCallContext<'_, McpServer>| {
            let handler = Arc::clone(&handler);
            let arguments = context.arguments.unwrap_or_default();
            Box::pin(async move {
                let params: P =
                    serde_json::from_value(Value::Object(arguments)).map_err(|error| {
                        ErrorData::invalid_params(format!("parâmetros inválidos: {error}"), None)
                    })?;
                let value =
                    tokio::task::spawn_blocking(move || serde_json::to_value(handler(params)))
                        .await
                        .map_err(|error| {
                            ErrorData::internal_error(format!("tool interrompida: {error}"), None)
                        })?
                        .map_err(|error| {
                            ErrorData::internal_error(
                                format!("resposta não serializável: {error}"),
                                None,
                            )
                        })?;
                Ok(render(value))
            })
        });
        self.router.add_route(route);
    }

    /// Tools registradas, na ordem alfabética do nome.
    pub fn tools(&self) -> Vec<Tool> {
        let mut tools = self.router.list_all();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }

    /// Nomes das tools registradas, em ordem alfabética.
    pub fn tool_names(&self) -> Vec<String> {
        self.tools()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect()
    }

    /// Executa a tool pedida em `context`. Usado pelo `ServerHandler` em `server.rs`.
    ///
    /// # Errors
    ///
    /// [`ErrorData`] se a tool não existir ou os parâmetros forem inválidos.
    pub async fn call(
        &self,
        context: ToolCallContext<'_, McpServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.router.call(context).await
    }
}

/// Monta a resposta MCP: o JSON como texto (para hosts antigos) e como
/// `structuredContent` (para hosts que leem o objeto direto).
fn render(value: Value) -> CallToolResult {
    let text = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    CallToolResult {
        content: vec![Content::text(text)],
        structured_content: Some(value),
        is_error: None,
        meta: None,
    }
}

/// Registra os domínios pedidos. Devolve os nomes registrados, em ordem.
pub fn register_domains<'a>(
    mcp: &mut McpServer,
    runtime: &Arc<Runtime>,
    names: impl IntoIterator<Item = &'a DomainName>,
) -> Vec<String> {
    let mut chosen: Vec<DomainName> = names.into_iter().copied().collect();
    chosen.sort();
    chosen.dedup();
    let mut registered = Vec::with_capacity(chosen.len());
    for name in chosen {
        match name {
            DomainName::Audio => audio::register(mcp, runtime),
            DomainName::Files => files::register(mcp, runtime),
            DomainName::Jobs => jobs::register(mcp, runtime),
            DomainName::Media => media::register(mcp, runtime),
            DomainName::Video => video::register(mcp, runtime),
        }
        registered.push(name.as_str().to_string());
    }
    registered
}
