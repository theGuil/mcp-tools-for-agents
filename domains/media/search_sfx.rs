//! Tool `search_sound_effects`: busca efeitos sonoros no Freesound.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ToolResult};
use crate::core::freesound::SoundCandidate;
use crate::domains::{McpServer, Runtime};

/// Sons encontrados para a busca.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchSoundEffectsResult {
    pub query: String,
    pub count: usize,
    pub sounds: Vec<SoundCandidate>,
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Query vazia, limite inválido ou falha da API.
pub fn search_sound_effects(
    runtime: &Runtime,
    query: &str,
    limit: u32,
    max_duration: f64,
) -> ToolResult<SearchSoundEffectsResult> {
    let sounds = runtime.freesound.search(query, limit, Some(max_duration))?;
    Ok(SearchSoundEffectsResult {
        query: query.trim().to_string(),
        count: sounds.len(),
        sounds,
    })
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Descrição do som, de preferência em inglês.
    pub query: String,
    /// Quantos resultados devolver, até 30.
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Ignora sons mais longos que isso, em segundos.
    #[serde(default = "default_max_duration")]
    pub max_duration: f64,
}

fn default_limit() -> u32 {
    5
}

fn default_max_duration() -> f64 {
    10.0
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "search_sound_effects",
        "Busca efeitos sonoros gratuitos (Freesound) por descrição em texto.\n\n\
         Use quando quiser escolher o som antes de aplicá-lo com \
         add_sound_effects, ou para ver opções quando o primeiro resultado não \
         serviu. Descreva o som em inglês, que é o idioma do acervo: \"vine \
         boom\", \"record scratch\", \"notification ding\", \"whoosh\", \"sad trombone\", \
         \"crowd laugh\", \"crickets\". Nada é baixado: cada resultado traz \
         sound_id, nome, duração, licença e autor. Para baixar, use \
         download_sound_effect com o sound_id, ou passe o sound_id direto em \
         add_sound_effects.\n\n\
         Licenças CC0 e CC BY servem para qualquer uso; CC BY pede crédito ao \
         autor na descrição do vídeo. CC BY-NC é só para uso não comercial.",
        move |params: Params| {
            guarded(search_sound_effects(
                &runtime,
                &params.query,
                params.limit,
                params.max_duration,
            ))
        },
    );
}
