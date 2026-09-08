//! Tool `download_video`: baixa um vídeo de qualquer URL para o workspace.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::downloader::VideoQuality;
use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::domains::{McpServer, Runtime};

const DEFAULT_FOLDER: &str = "downloads";

/// Arquivo baixado e seus metadados.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DownloadVideoResult {
    pub output: String,
    pub title: String,
    pub duration: f64,
    pub quality: VideoQuality,
    pub size_bytes: u64,
}

fn do_download(
    runtime: &Runtime,
    url: &str,
    quality: VideoQuality,
    folder: &str,
) -> ToolResult<DownloadVideoResult> {
    let target_dir = runtime.workspace.resolve(folder)?;
    if target_dir.exists() && !target_dir.is_dir() {
        return Err(ToolError::with_hint(
            format!("'{folder}' existe e não é uma pasta."),
            ErrorCode::InvalidArgument,
            "Informe outra pasta em folder.",
        ));
    }
    let path = runtime.downloader.download(url, &target_dir, quality)?;
    let info = runtime.ffmpeg.probe(&path)?;
    Ok(DownloadVideoResult {
        output: runtime.workspace.relative(&path),
        title: path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default(),
        duration: info.duration,
        quality,
        size_bytes: info.size_bytes,
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// URL inválida, pasta fora do workspace ou falha do download.
pub fn download_video(
    runtime: &Arc<Runtime>,
    url: &str,
    quality: VideoQuality,
    folder: &str,
    background: bool,
) -> ToolResult<MaybeJob<DownloadVideoResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let url = url.to_string();
        let folder = folder.to_string();
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("download_video", move || {
                do_download(&runtime_job, &url, quality, &folder)
            }),
        ));
    }
    Ok(MaybeJob::Done(do_download(runtime, url, quality, folder)?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Endereço da página onde o vídeo está, começando com https://.
    pub url: String,
    /// best, 1080p, 720p, 480p, 360p ou audio (só o áudio, em m4a).
    #[serde(default = "default_quality")]
    pub quality: VideoQuality,
    /// Pasta do workspace onde salvar. Criada se não existir.
    #[serde(default = "default_folder")]
    pub folder: String,
    /// Executa como job e devolve job_id.
    #[serde(default = "default_true")]
    pub background: bool,
}

fn default_quality() -> VideoQuality {
    VideoQuality::P720
}

fn default_folder() -> String {
    DEFAULT_FOLDER.to_string()
}

fn default_true() -> bool {
    true
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "download_video",
        "Baixa para o workspace um vídeo de qualquer site, em MP4.\n\n\
         Use sempre que o usuário der uma URL e pedir o vídeo, seja de onde for: \
         YouTube, Vimeo, Twitch, X/Twitter, TikTok, Instagram, Facebook, portais \
         de notícia, plataformas de aula (eaulas.usp.br e afins) ou uma página \
         comum com player embutido. Quando nenhum site é reconhecido, a tool lê o \
         HTML da página e procura sozinha a fonte do vídeo, inclusive streams HLS \
         (.m3u8) e DASH (.mpd). Passe a URL da página onde o vídeo aparece; só use \
         a URL direta do arquivo se a página falhar.\n\n\
         Não há como baixar conteúdo com DRM (Netflix, Disney+, cursos com \
         Widevine) nem páginas que exigem login — nesses casos o hint do erro \
         avisa para não insistir.\n\n\
         Ponto de partida do fluxo de edição: depois do download use probe_video, \
         extract_frame ou transcribe_audio para \"assistir\" e escolher os trechos, \
         cut_video para cortar e as demais tools para legendar e estilizar. \
         Downloads são lentos, então roda em background por padrão: acompanhe com \
         job_status e pegue o caminho do arquivo em job_result.",
        move |params: Params| {
            guarded(download_video(
                &runtime,
                &params.url,
                params.quality,
                &params.folder,
                params.background,
            ))
        },
    );
}
