//! Tool `add_sound_effects`: coloca efeitos sonoros em instantes específicos do vídeo.

use std::path::PathBuf;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::core::numbers::format_g;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

const DEFAULT_SFX_FOLDER: &str = "sfx";
const DEFAULT_EFFECT_VOLUME: f64 = 1.0;
const MAX_EFFECT_VOLUME: f64 = 3.0;
const MAX_EFFECTS: usize = 20;

/// Um efeito a inserir: de onde vem o som e quando ele toca.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct SoundEffect {
    /// Segundo em que o som começa.
    pub start: f64,
    /// Arquivo de áudio que já está no workspace.
    #[serde(default)]
    pub audio: Option<String>,
    /// Descrição do som em inglês para buscar no Freesound.
    #[serde(default)]
    pub query: Option<String>,
    /// Id devolvido por search_sound_effects.
    #[serde(default)]
    pub sound_id: Option<i64>,
    /// Volume do efeito (1 = como veio, 0.5 mais baixo, até 3).
    #[serde(default)]
    pub volume: Option<f64>,
}

impl SoundEffect {
    /// Efeito a partir de um arquivo do workspace.
    pub fn from_audio(audio: &str, start: f64) -> Self {
        Self {
            start,
            audio: Some(audio.to_string()),
            query: None,
            sound_id: None,
            volume: None,
        }
    }

    /// Efeito buscado no Freesound pela descrição.
    pub fn from_query(query: &str, start: f64) -> Self {
        Self {
            start,
            audio: None,
            query: Some(query.to_string()),
            sound_id: None,
            volume: None,
        }
    }

    /// Efeito por id do Freesound.
    pub fn from_sound_id(sound_id: i64, start: f64) -> Self {
        Self {
            start,
            audio: None,
            query: None,
            sound_id: Some(sound_id),
            volume: None,
        }
    }

    /// Define o volume do efeito.
    pub fn with_volume(mut self, volume: f64) -> Self {
        self.volume = Some(volume);
        self
    }
}

/// Efeito que entrou no vídeo, com o arquivo que foi usado.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppliedSoundEffect {
    pub audio: String,
    pub start: f64,
    pub volume: f64,
    pub duration: f64,
    pub sound_id: Option<i64>,
    pub name: Option<String>,
    pub license: Option<String>,
    pub author: Option<String>,
}

/// Vídeo gerado com os efeitos.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AddSoundEffectsResult {
    pub output: String,
    pub effects: Vec<AppliedSoundEffect>,
    pub original_volume: f64,
}

/// Efeito com o arquivo já no disco, pronto para o ffmpeg.
#[derive(Debug, Clone)]
struct Resolved {
    path: PathBuf,
    applied: AppliedSoundEffect,
}

/// Descobre o arquivo do efeito: do workspace, por id ou por busca no Freesound.
fn resolve_effect(
    runtime: &Runtime,
    effect: &SoundEffect,
    index: usize,
    folder: &str,
) -> ToolResult<Resolved> {
    let start = effect.start;
    if start < 0.0 {
        return Err(ToolError::new(
            format!("effects[{index}].start deve ser >= 0."),
            ErrorCode::InvalidArgument,
        ));
    }
    let volume = effect.volume.unwrap_or(DEFAULT_EFFECT_VOLUME);
    if !(volume > 0.0 && volume <= MAX_EFFECT_VOLUME) {
        return Err(ToolError::with_hint(
            format!(
                "effects[{index}].volume deve estar entre 0 e {}.",
                format_g(MAX_EFFECT_VOLUME)
            ),
            ErrorCode::InvalidArgument,
            "1 mantém o volume do efeito, 0.5 deixa mais baixo, 2 aumenta.",
        ));
    }
    let sources = usize::from(effect.audio.is_some())
        + usize::from(effect.sound_id.is_some())
        + usize::from(effect.query.is_some());
    if sources != 1 {
        return Err(ToolError::with_hint(
            format!("effects[{index}] precisa de exatamente um entre audio, sound_id e query."),
            ErrorCode::InvalidArgument,
            "Use audio para um arquivo do workspace, sound_id para um resultado de \
             search_sound_effects ou query para buscar o som pelo nome.",
        ));
    }
    if let Some(audio) = &effect.audio {
        let path = runtime.workspace.existing(audio)?;
        return Ok(Resolved {
            applied: AppliedSoundEffect {
                audio: runtime.workspace.relative(&path),
                start,
                volume,
                duration: 0.0,
                sound_id: None,
                name: None,
                license: None,
                author: None,
            },
            path,
        });
    }
    let candidate = if let Some(sound_id) = effect.sound_id {
        runtime.freesound.sound(sound_id)?
    } else {
        let query = effect.query.as_deref().unwrap_or("");
        let found = runtime.freesound.search(query, 1, Some(10.0))?;
        found.into_iter().next().ok_or_else(|| {
            ToolError::with_hint(
                format!("Nenhum som encontrado para '{query}'."),
                ErrorCode::NotFound,
                "Tente outra descrição em inglês, mais curta ou mais genérica, \
                 ou busque opções com search_sound_effects.",
            )
        })?
    };
    let target_dir = runtime.workspace.resolve(folder)?;
    let path = runtime
        .freesound
        .download_preview(&candidate, &target_dir)?;
    Ok(Resolved {
        applied: AppliedSoundEffect {
            audio: runtime.workspace.relative(&path),
            start,
            volume,
            duration: candidate.duration,
            sound_id: Some(candidate.sound_id),
            name: Some(candidate.name),
            license: Some(candidate.license),
            author: Some(candidate.author),
        },
        path,
    })
}

/// Monta o filter_complex: cada efeito ganha volume e atraso, e tudo é somado.
fn build_filter(resolved: &[Resolved], has_original: bool, original_volume: f64) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    if has_original {
        parts.push(format!("[0:a]volume={original_volume}[bg]"));
        labels.push("[bg]".to_string());
    }
    for (index, item) in resolved.iter().enumerate() {
        let index = index + 1;
        let applied = &item.applied;
        let delay_ms = (applied.start * 1000.0).round() as i64;
        parts.push(format!(
            "[{index}:a]volume={},adelay={delay_ms}:all=1[fx{index}]",
            applied.volume
        ));
        labels.push(format!("[fx{index}]"));
    }
    let joined = labels.concat();
    if labels.len() == 1 {
        // Só um fluxo: não há o que misturar, mas o apad garante som até o fim do vídeo.
        parts.push(format!("{joined}apad[aout]"));
    } else if has_original {
        parts.push(format!(
            "{joined}amix=inputs={}:duration=first:dropout_transition=0:normalize=0[aout]",
            labels.len()
        ));
    } else {
        parts.push(format!(
            "{joined}amix=inputs={}:duration=longest:dropout_transition=0:normalize=0,apad[aout]",
            labels.len()
        ));
    }
    parts.join(";")
}

fn do_sound_effects(
    runtime: &Runtime,
    path: &str,
    effects: &[SoundEffect],
    original_volume: f64,
    folder: &str,
) -> ToolResult<AddSoundEffectsResult> {
    let source = runtime.workspace.existing(path)?;
    if effects.is_empty() {
        return Err(ToolError::with_hint(
            "effects não pode ser vazio.",
            ErrorCode::InvalidArgument,
            "Informe ao menos um efeito, ex: [{\"query\": \"vine boom\", \"start\": 3.2}].",
        ));
    }
    if effects.len() > MAX_EFFECTS {
        return Err(ToolError::with_hint(
            format!("No máximo {MAX_EFFECTS} efeitos por chamada."),
            ErrorCode::InvalidArgument,
            "Divida em mais de uma chamada, aplicando sobre o vídeo gerado.",
        ));
    }
    if !(0.0..=1.0).contains(&original_volume) {
        return Err(ToolError::with_hint(
            "original_volume deve estar entre 0 e 1.",
            ErrorCode::InvalidArgument,
            "1 mantém o áudio original, 0.5 abaixa pela metade, 0 silencia.",
        ));
    }
    let info = runtime.ffmpeg.probe(&source)?;
    // Confere os instantes antes de baixar qualquer som: erro barato primeiro.
    for (index, effect) in effects.iter().enumerate() {
        if effect.start >= info.duration {
            return Err(ToolError::with_hint(
                format!(
                    "effects[{index}].start={}s ultrapassa a duração do vídeo ({:.2}s).",
                    effect.start, info.duration
                ),
                ErrorCode::InvalidArgument,
                "Use probe_video para conferir a duração.",
            ));
        }
    }
    let mut resolved = effects
        .iter()
        .enumerate()
        .map(|(index, effect)| resolve_effect(runtime, effect, index, folder))
        .collect::<ToolResult<Vec<Resolved>>>()?;
    for item in &mut resolved {
        let effect_info = runtime.ffmpeg.probe(&item.path)?;
        if !effect_info.has_audio {
            return Err(ToolError::with_hint(
                format!("'{}' não tem trilha de áudio.", item.applied.audio),
                ErrorCode::InvalidArgument,
                "Informe um arquivo de áudio (mp3, m4a, wav).",
            ));
        }
        if item.applied.duration == 0.0 {
            item.applied.duration = effect_info.duration;
        }
    }
    let has_original = info.has_audio && original_volume > 0.0;
    let filter_expr = build_filter(&resolved, has_original, original_volume);
    let output = runtime.workspace.output_for(&source, "sfx", None);
    let mut args = ffargs!["-i", source];
    for item in &resolved {
        args.extend(ffargs!["-i", item.path]);
    }
    args.extend(ffargs![
        "-filter_complex",
        filter_expr,
        "-map",
        "0:v:0",
        "-map",
        "[aout]",
        "-c:v",
        "copy",
        "-c:a",
        "aac",
        "-shortest"
    ]);
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(AddSoundEffectsResult {
        output: runtime.workspace.relative(&output),
        effects: resolved.into_iter().map(|item| item.applied).collect(),
        original_volume,
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Argumento inválido, arquivo ou som inexistente, falha de rede ou do ffmpeg.
pub fn add_sound_effects(
    runtime: &Arc<Runtime>,
    path: &str,
    effects: &[SoundEffect],
    original_volume: f64,
    folder: &str,
    background: bool,
) -> ToolResult<MaybeJob<AddSoundEffectsResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        let effects = effects.to_vec();
        let folder = folder.to_string();
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("add_sound_effects", move || {
                do_sound_effects(&runtime_job, &path, &effects, original_volume, &folder)
            }),
        ));
    }
    Ok(MaybeJob::Done(do_sound_effects(
        runtime,
        path,
        effects,
        original_volume,
        folder,
    )?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// Lista de efeitos, cada um com start e query, sound_id ou audio.
    pub effects: Vec<SoundEffect>,
    /// Volume do áudio original, de 0 (mudo) a 1 (igual).
    #[serde(default = "default_original_volume")]
    pub original_volume: f64,
    /// Pasta do workspace onde os sons baixados ficam guardados.
    #[serde(default = "default_folder")]
    pub folder: String,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_original_volume() -> f64 {
    1.0
}

fn default_folder() -> String {
    DEFAULT_SFX_FOLDER.to_string()
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "add_sound_effects",
        "Insere efeitos sonoros (vine boom, ding, whoosh...) em instantes do vídeo.\n\n\
         Use para dar ritmo de TikTok/Reels a um vídeo: um \"boom\" na revelação, \
         um \"record scratch\" na pausa, um \"ding\" quando aparece o texto. Passe \
         quantos efeitos quiser de uma vez; tudo é aplicado em um único passo e \
         o vídeo não é re-encodado, só o áudio.\n\n\
         Cada item de effects tem start (segundo em que o som começa) e uma \
         única origem para o som:\n\
         - query: descreve o som em inglês (\"vine boom\", \"notification ding\", \
         \"crowd laugh\") e a tool busca no Freesound, baixa o primeiro \
         resultado para a pasta sfx/ e usa;\n\
         - sound_id: id devolvido por search_sound_effects, quando você quer \
         escolher o som;\n\
         - audio: arquivo de áudio que já está no workspace.\n\
         volume é opcional (1 = como veio, 0.5 mais baixo, até 3 para reforçar).\n\n\
         O áudio original continua no vídeo em original_volume; use 0 para \
         deixar só os efeitos. Descubra os instantes certos com \
         transcribe_audio ou detect_scenes. O resultado lista, para cada efeito, \
         o arquivo usado e a licença; sons CC BY pedem crédito ao autor.",
        move |params: Params| {
            guarded(add_sound_effects(
                &runtime,
                &params.path,
                &params.effects,
                params.original_volume,
                &params.folder,
                params.background,
            ))
        },
    );
}
