//! Tool `add_background_music`: trilha sonora em loop, com fade e ducking automático.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::core::numbers::{format_g, round_to};
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

const MAX_FADE: f64 = 30.0;

/// Vídeo gerado com a trilha de fundo.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AddBackgroundMusicResult {
    pub output: String,
    pub music: String,
    pub music_volume: f64,
    pub ducking: bool,
    pub looped: bool,
    pub fade_in: f64,
    pub fade_out: f64,
    pub duration: f64,
}

fn validate(
    music_volume: f64,
    fade_in: f64,
    fade_out: f64,
    start: f64,
    duration: f64,
) -> ToolResult<()> {
    if !(music_volume > 0.0 && music_volume <= 1.0) {
        return Err(ToolError::with_hint(
            "music_volume deve estar entre 0 (exclusivo) e 1.",
            ErrorCode::InvalidArgument,
            "0.15 a 0.3 é o usual para fundo de fala; 1 deixa a música no volume original.",
        ));
    }
    if fade_in < 0.0 || fade_out < 0.0 || fade_in > MAX_FADE || fade_out > MAX_FADE {
        return Err(ToolError::new(
            format!(
                "fade_in e fade_out devem estar entre 0 e {} segundos.",
                format_g(MAX_FADE)
            ),
            ErrorCode::InvalidArgument,
        ));
    }
    if start < 0.0 {
        return Err(ToolError::new(
            "start deve ser >= 0.",
            ErrorCode::InvalidArgument,
        ));
    }
    if start >= duration {
        return Err(ToolError::with_hint(
            format!("start={start}s ultrapassa a duração do vídeo ({duration:.2}s)."),
            ErrorCode::InvalidArgument,
            "Use probe_video para conferir a duração.",
        ));
    }
    Ok(())
}

/// Prepara a música: loop, corte na duração do vídeo, fades e volume.
pub fn music_chain(
    music_volume: f64,
    fade_in: f64,
    fade_out: f64,
    start: f64,
    duration: f64,
    do_loop: bool,
) -> String {
    let music_len = duration - start;
    let mut steps: Vec<String> = if do_loop {
        vec!["aloop=loop=-1:size=2e9".to_string()]
    } else {
        Vec::new()
    };
    steps.push(format!("atrim=0:{music_len:.3}"));
    steps.push("asetpts=PTS-STARTPTS".to_string());
    if fade_in > 0.0 {
        steps.push(format!("afade=t=in:st=0:d={fade_in:.3}"));
    }
    if fade_out > 0.0 {
        let fade_start = (music_len - fade_out).max(0.0);
        steps.push(format!("afade=t=out:st={fade_start:.3}:d={fade_out:.3}"));
    }
    steps.push(format!("volume={music_volume}"));
    if start > 0.0 {
        steps.push(format!("adelay={}:all=1", (start * 1000.0).round() as i64));
    }
    steps.join(",")
}

#[allow(clippy::too_many_arguments)] // espelha a assinatura da tool Python
fn do_add_music(
    runtime: &Runtime,
    path: &str,
    music_path: &str,
    music_volume: f64,
    ducking: bool,
    fade_in: f64,
    fade_out: f64,
    start: f64,
    do_loop: bool,
) -> ToolResult<AddBackgroundMusicResult> {
    let source = runtime.workspace.existing(path)?;
    let music = runtime.workspace.existing(music_path)?;
    let info = runtime.ffmpeg.probe(&source)?;
    let music_info = runtime.ffmpeg.probe(&music)?;
    if !music_info.has_audio {
        return Err(ToolError::with_hint(
            format!("'{music_path}' não tem trilha de áudio."),
            ErrorCode::InvalidArgument,
            "Informe um arquivo de música (mp3, m4a, wav, ogg).",
        ));
    }
    let duration = info.duration;
    validate(music_volume, fade_in, fade_out, start, duration)?;
    let chain = music_chain(music_volume, fade_in, fade_out, start, duration, do_loop);
    let has_voice = info.has_audio;
    let apply_ducking = ducking && has_voice;
    let filter_expr = if !has_voice {
        format!("[1:a]{chain},apad[aout]")
    } else if apply_ducking {
        // sidechaincompress abaixa a música sempre que a fala (sidechain) passa do threshold.
        format!(
            "[1:a]{chain}[music];\
             [0:a]asplit=2[voice][sc];\
             [music][sc]sidechaincompress=threshold=0.03:ratio=8:attack=20:release=400\
             :makeup=1[ducked];\
             [voice][ducked]amix=inputs=2:duration=first:dropout_transition=0:normalize=0[aout]"
        )
    } else {
        format!(
            "[1:a]{chain}[music];\
             [0:a][music]amix=inputs=2:duration=first:dropout_transition=0:normalize=0[aout]"
        )
    };
    let output = runtime.workspace.output_for(&source, "music", None);
    let mut args = ffargs![
        "-i",
        source,
        "-i",
        music,
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
        "-b:a",
        "192k",
        "-shortest"
    ];
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(AddBackgroundMusicResult {
        output: runtime.workspace.relative(&output),
        music: runtime.workspace.relative(&music),
        music_volume,
        ducking: apply_ducking,
        looped: do_loop,
        fade_in,
        fade_out,
        duration: round_to(duration, 3),
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Argumento inválido, arquivo inexistente ou falha do ffmpeg.
#[allow(clippy::too_many_arguments)] // espelha a assinatura da tool Python
pub fn add_background_music(
    runtime: &Arc<Runtime>,
    path: &str,
    music_path: &str,
    music_volume: f64,
    ducking: bool,
    fade_in: f64,
    fade_out: f64,
    start: f64,
    do_loop: bool,
    background: bool,
) -> ToolResult<MaybeJob<AddBackgroundMusicResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        let music_path = music_path.to_string();
        return Ok(MaybeJob::Job(runtime.jobs.submit(
            "add_background_music",
            move || {
                do_add_music(
                    &runtime_job,
                    &path,
                    &music_path,
                    music_volume,
                    ducking,
                    fade_in,
                    fade_out,
                    start,
                    do_loop,
                )
            },
        )));
    }
    Ok(MaybeJob::Done(do_add_music(
        runtime,
        path,
        music_path,
        music_volume,
        ducking,
        fade_in,
        fade_out,
        start,
        do_loop,
    )?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// Arquivo de música (mp3, m4a, wav, ogg), relativo ao workspace.
    pub music_path: String,
    /// Volume da música de 0 a 1. 0.2 é fundo discreto para fala, 0.5
    /// bem presente, 1 volume original (só para vídeo sem fala).
    #[serde(default = "default_music_volume")]
    pub music_volume: f64,
    /// Abaixa a música automaticamente enquanto há fala no vídeo.
    #[serde(default = "default_true")]
    pub ducking: bool,
    /// Segundos de entrada suave da música no início.
    #[serde(default = "default_fade_in")]
    pub fade_in: f64,
    /// Segundos de saída suave da música no final do vídeo.
    #[serde(default = "default_fade_out")]
    pub fade_out: f64,
    /// Segundo do vídeo em que a música começa.
    #[serde(default)]
    pub start: f64,
    /// Repete a música até o fim do vídeo. Com false ela toca uma vez e para.
    #[serde(default = "default_true", rename = "loop")]
    pub do_loop: bool,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_music_volume() -> f64 {
    0.2
}

fn default_fade_in() -> f64 {
    1.0
}

fn default_fade_out() -> f64 {
    2.0
}

fn default_true() -> bool {
    true
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "add_background_music",
        "Coloca música de fundo no vídeo, em loop, com fade e volume que abaixa quando há fala.\n\n\
         Use para dar clima a um corte de TikTok, Reels ou YouTube depois de já ter o \
         vídeo cortado e legendado. A música é repetida até cobrir o vídeo inteiro \
         (loop=true), entra com fade_in, sai com fade_out no final e toca no volume \
         music_volume. Com ducking=true a música abaixa sozinha enquanto alguém fala e \
         volta ao normal nas pausas (sidechain), o padrão dos editores profissionais. \
         A fala original é preservada. O vídeo não é re-encodado, só o áudio.\n\n\
         Diferença para add_narration: add_narration coloca uma VOZ por cima do vídeo; \
         add_background_music coloca uma MÚSICA por baixo da voz que já existe.\n\n\
         Fluxo típico: cut_video -> burn_subtitles -> add_background_music -> \
         normalize_audio -> export_for_platform.",
        move |params: Params| {
            guarded(add_background_music(
                &runtime,
                &params.path,
                &params.music_path,
                params.music_volume,
                params.ducking,
                params.fade_in,
                params.fade_out,
                params.start,
                params.do_loop,
                params.background,
            ))
        },
    );
}
