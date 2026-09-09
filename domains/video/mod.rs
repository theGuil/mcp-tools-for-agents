//! Tools de vídeo: inspeção, corte, ritmo, legenda, som, enquadramento e exportação.

use std::sync::Arc;

use crate::domains::{McpServer, Runtime};

pub mod background_light;
pub mod background_music;
pub mod banner;
pub mod burn_subtitles;
pub mod concat;
pub mod cut;
pub mod dynamic_subtitles;
pub mod export;
pub mod fade;
pub mod frame;
pub mod metadata;
pub mod narration;
pub mod probe;
pub mod remove_silence;
pub mod scenes;
pub mod smart_crop;
pub mod sound_effects;
pub mod speed;
pub mod stabilize;
pub mod subtitles;
pub mod template;
pub mod text_overlay;
pub mod thumbnail;
pub mod zoom;

/// Registra todas as tools do domínio.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    probe::register(mcp, runtime);
    cut::register(mcp, runtime);
    concat::register(mcp, runtime);
    scenes::register(mcp, runtime);
    remove_silence::register(mcp, runtime);
    frame::register(mcp, runtime);
    text_overlay::register(mcp, runtime);
    subtitles::register(mcp, runtime);
    burn_subtitles::register(mcp, runtime);
    metadata::register(mcp, runtime);
    narration::register(mcp, runtime);
    sound_effects::register(mcp, runtime);
    template::register(mcp, runtime);
    banner::register(mcp, runtime);
    background_music::register(mcp, runtime);
    background_light::register(mcp, runtime);
    fade::register(mcp, runtime);
    speed::register(mcp, runtime);
    zoom::register(mcp, runtime);
    dynamic_subtitles::register(mcp, runtime);
    thumbnail::register(mcp, runtime);
    smart_crop::register(mcp, runtime);
    stabilize::register(mcp, runtime);
    export::register(mcp, runtime);
}
