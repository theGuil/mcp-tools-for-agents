//! Tool `download_sound_effect`: baixa um efeito sonoro do Freesound para o workspace.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::domains::{McpServer, Runtime};

/// Pasta padrão dos efeitos sonoros dentro do workspace.
pub const DEFAULT_SFX_FOLDER: &str = "sfx";

/// Efeito sonoro salvo no workspace.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DownloadSoundEffectResult {
    pub output: String,
    pub sound_id: i64,
    pub name: String,
    pub duration: f64,
    pub license: String,
    pub author: String,
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Id inválido, som inexistente, pasta fora do workspace ou falha do download.
pub fn download_sound_effect(
    runtime: &Runtime,
    sound_id: i64,
    folder: &str,
) -> ToolResult<DownloadSoundEffectResult> {
    let target_dir = runtime.workspace.resolve(folder)?;
    if target_dir.exists() && !target_dir.is_dir() {
        return Err(ToolError::with_hint(
            format!("'{folder}' existe e não é uma pasta."),
            ErrorCode::InvalidArgument,
            "Informe outra pasta em folder.",
        ));
    }
    let candidate = runtime.freesound.sound(sound_id)?;
    let path = runtime
        .freesound
        .download_preview(&candidate, &target_dir)?;
    Ok(DownloadSoundEffectResult {
        output: runtime.workspace.relative(&path),
        sound_id,
        name: candidate.name,
        duration: candidate.duration,
        license: candidate.license,
        author: candidate.author,
    })
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Id do som, devolvido por search_sound_effects.
    pub sound_id: i64,
    /// Pasta do workspace onde salvar. Criada se não existir.
    #[serde(default = "default_folder")]
    pub folder: String,
}

fn default_folder() -> String {
    DEFAULT_SFX_FOLDER.to_string()
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "download_sound_effect",
        "Baixa para o workspace um efeito sonoro do Freesound, em MP3.\n\n\
         Use depois de search_sound_effects, quando quiser guardar o som para \
         reaproveitar em vários vídeos ou ouvir antes de aplicar. Se o objetivo \
         é só colocar o som no vídeo, add_sound_effects já faz o download \
         sozinho a partir do sound_id ou de uma query. Baixar o mesmo som de \
         novo reaproveita o arquivo que já está na pasta.",
        move |params: Params| {
            guarded(download_sound_effect(
                &runtime,
                params.sound_id,
                &params.folder,
            ))
        },
    );
}
