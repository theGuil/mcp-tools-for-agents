//! Monta o servidor MCP e registra os domínios ativos.

use std::sync::Arc;

use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParam, CallToolResult, ErrorData, Implementation, ListToolsResult,
    PaginatedRequestParam, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler, ServiceExt};

use crate::config::Settings;
use crate::core::binaries::Binaries;
use crate::core::downloader::Downloader;
use crate::core::errors::{ErrorCode, ToolError, ToolResult};
use crate::core::ffmpeg::FFmpeg;
use crate::core::freesound::Freesound;
use crate::core::jobs::JobManager;
use crate::core::paths::Workspace;
use crate::domains::{register_domains, McpServer, Runtime};

const INSTRUCTIONS: &str = "Servidor de tools para edição de vídeo e áudio. Fluxo típico: \
    download_video (qualquer URL) -> probe_video/extract_frame para assistir -> cut_video -> \
    add_text_overlay/burn_subtitles/apply_template -> set_video_metadata. Para efeitos sonoros \
    (vine boom, ding, whoosh) em instantes específicos use add_sound_effects, que busca no \
    Freesound sozinho a partir de um texto; search_sound_effects deixa você escolher o som. \
    Todos os caminhos são relativos ao workspace. Comece com list_files e probe_video. Operações \
    longas aceitam background=true e devolvem job_id; acompanhe com job_status \
    e busque a saída com job_result. Erros vêm como {error, code, hint}.";

/// Instancia as dependências compartilhadas a partir das configurações.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] se o workspace não puder ser criado.
pub fn build_runtime(settings: Settings) -> ToolResult<Runtime> {
    let workspace = Workspace::at(&settings.workspace_dir).map_err(|error| {
        ToolError::with_hint(
            format!(
                "Não foi possível preparar o workspace '{}': {error}",
                settings.workspace_dir.display()
            ),
            ErrorCode::InvalidArgument,
            "Ajuste WORKSPACE_DIR para uma pasta em que o servidor possa escrever.",
        )
    })?;
    let binaries = Binaries::new(settings.cache_dir.clone(), settings.auto_download);
    Ok(Runtime {
        workspace,
        ffmpeg: FFmpeg::new(
            settings.ffmpeg_bin.clone(),
            settings.ffprobe_bin.clone(),
            settings.ffmpeg_timeout,
            binaries.clone(),
        ),
        jobs: JobManager::new(settings.job_workers),
        downloader: Downloader::new(
            settings.ffmpeg_bin.clone(),
            settings.ytdlp_bin.clone(),
            settings.download_timeout,
            binaries,
        ),
        freesound: Freesound::new(settings.freesound_api_key.clone()),
        settings,
    })
}

/// Cria o servidor com os domínios ativos registrados.
///
/// # Errors
///
/// [`ToolError`] se o runtime não puder ser montado.
pub fn build_server(settings: Settings, runtime: Option<Arc<Runtime>>) -> ToolResult<McpServer> {
    let runtime = match runtime {
        Some(runtime) => runtime,
        None => Arc::new(build_runtime(settings.clone())?),
    };
    let mut mcp = McpServer::new(settings.server_name.clone(), INSTRUCTIONS);
    register_domains(&mut mcp, &runtime, settings.domains.iter());
    Ok(mcp)
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::default(),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: self.name.clone(),
                title: None,
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some(self.instructions.clone()),
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(ToolCallContext::new(self, request, context))
            .await
    }
}

/// Ponto de entrada: sobe o servidor via stdio. Devolve o código de saída.
pub fn main() -> i32 {
    let settings = match Settings::from_env() {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!(
                "Configuração inválida: {} {}",
                error.message,
                error.hint.as_deref().unwrap_or("")
            );
            return 2;
        }
    };
    let server = match build_server(settings, None) {
        Ok(server) => server,
        Err(error) => {
            eprintln!(
                "Configuração inválida: {} {}",
                error.message,
                error.hint.as_deref().unwrap_or("")
            );
            return 2;
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Não foi possível iniciar o runtime assíncrono: {error}");
            return 1;
        }
    };
    runtime.block_on(async move {
        match server.serve(rmcp::transport::stdio()).await {
            Ok(service) => match service.waiting().await {
                Ok(_) => 0,
                Err(error) => {
                    eprintln!("Servidor encerrou com erro: {error}");
                    1
                }
            },
            Err(error) => {
                eprintln!("Falha ao iniciar o servidor MCP: {error}");
                1
            }
        }
    })
}
