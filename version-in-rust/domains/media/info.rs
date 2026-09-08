//! Tool `get_video_info`: lê título, duração, descrição e capítulos de uma URL sem baixar.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::downloader::Chapter;
use crate::core::errors::{guarded, ToolResult};
use crate::domains::{McpServer, Runtime};

/// Metadados do vídeo na origem.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VideoInfoResult {
    pub url: String,
    pub id: String,
    pub title: String,
    pub extractor: String,
    pub channel: Option<String>,
    pub duration: f64,
    pub view_count: Option<i64>,
    pub upload_date: Option<String>,
    pub description: String,
    pub thumbnail: Option<String>,
    pub chapters: Vec<Chapter>,
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// URL inválida ou vídeo inacessível.
pub fn get_video_info(runtime: &Runtime, url: &str) -> ToolResult<VideoInfoResult> {
    let data = runtime.downloader.info(url)?;
    Ok(VideoInfoResult {
        url: url.trim().to_string(),
        id: data.id,
        title: data.title,
        extractor: data.extractor,
        channel: data.channel,
        duration: data.duration,
        view_count: data.view_count,
        upload_date: data.upload_date,
        description: data.description,
        thumbnail: data.thumbnail,
        chapters: data.chapters,
    })
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Endereço da página onde o vídeo está, começando com https://.
    pub url: String,
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "get_video_info",
        "Consulta título, duração, descrição e capítulos de um vídeo na internet, sem baixar.\n\n\
         Aceita a URL de qualquer site, não só do YouTube. Use antes de \
         download_video para confirmar que a página tem mesmo um vídeo e decidir \
         a qualidade, sem gastar um download inteiro. O campo extractor diz quem \
         reconheceu a página (\"Youtube\", \"Vimeo\", \"Generic\" quando o vídeo foi \
         achado lendo o HTML). Os capítulos, quando existem, já indicam bons \
         pontos de corte. Campos como channel e view_count vêm nulos em sites que \
         não os publicam.",
        move |params: Params| guarded(get_video_info(&runtime, &params.url)),
    );
}
