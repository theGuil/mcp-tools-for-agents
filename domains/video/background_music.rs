//! Tool `add_background_music`: trilha sonora em loop, com fade e ducking automático.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::fonts::escape_filter_path;
use crate::core::jobs::MaybeJob;
use crate::core::numbers::{format_g, round_to};
use crate::core::speech::{self, DbOptions, SpeechMethod};
use crate::core::vad::VadOptions;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

const MAX_FADE: f64 = 30.0;
const MAX_DUCK_DB: f64 = 40.0;
const DEFAULT_DUCK_DB: f64 = 12.0;
/// Limiar do compressor de sidechain (-30.5 dBFS): bem abaixo do sinal de controle.
const DUCK_THRESHOLD: f64 = 0.03;
/// Razão quase de limiter: a redução fica praticamente igual ao pedido.
const DUCK_RATIO: f64 = 20.0;
/// O gerador `sine` do ffmpeg sai com amplitude 1/8 (-18 dBFS).
const SINE_LEVEL: f64 = 0.125;
/// A música começa a abaixar um pouco antes da primeira palavra.
const DUCK_PRE_ROLL: f64 = 0.15;
/// Pausa mínima na fala para a música voltar a subir.
const DUCK_MIN_SILENCE: f64 = 0.6;

/// Vídeo gerado com a trilha de fundo.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AddBackgroundMusicResult {
    pub output: String,
    pub music: String,
    pub music_volume: f64,
    pub ducking: bool,
    /// Quantos dB a música abaixa durante a fala.
    pub duck_db: f64,
    /// Como a fala foi localizada para o ducking: "vad", "db" ou null.
    pub ducking_method: Option<String>,
    /// Trechos de fala encontrados para o ducking.
    pub speech_segments: usize,
    pub looped: bool,
    pub fade_in: f64,
    pub fade_out: f64,
    pub duration: f64,
    /// Aviso quando o ducking não pôde seguir o método pedido.
    pub note: Option<String>,
}

/// Ajustes do ducking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DuckingOptions {
    /// Abaixa a música enquanto há fala.
    pub enabled: bool,
    /// Quantos dB abaixar (1 a 40).
    pub duck_db: f64,
    /// Como localizar a fala.
    pub method: SpeechMethod,
}

impl Default for DuckingOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            duck_db: DEFAULT_DUCK_DB,
            method: SpeechMethod::Auto,
        }
    }
}

fn validate(
    music_volume: f64,
    fade_in: f64,
    fade_out: f64,
    start: f64,
    duration: f64,
    duck_db: f64,
) -> ToolResult<()> {
    if !(duck_db > 0.0 && duck_db <= MAX_DUCK_DB) {
        return Err(ToolError::with_hint(
            format!(
                "duck_db deve estar entre 0 (exclusivo) e {}.",
                format_g(MAX_DUCK_DB)
            ),
            ErrorCode::InvalidArgument,
            "8 a 15 dB é o usual em podcast e narração; 20 deixa a música quase inaudível na fala.",
        ));
    }
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

/// Volume do sinal de controle do sidechain para o compressor reduzir `duck_db`.
///
/// Redução = (nível_sc - limiar) x (1 - 1/ratio), tudo em dB; aqui a conta é
/// invertida para achar o nível do sinal de controle.
pub fn gate_level(duck_db: f64) -> f64 {
    let ratio_factor = 1.0 - 1.0 / DUCK_RATIO;
    let threshold_db = 20.0 * DUCK_THRESHOLD.log10();
    let level_db = threshold_db + duck_db / ratio_factor;
    10f64.powf(level_db / 20.0) / SINE_LEVEL
}

/// Arquivo do `asendcmd`: liga o sinal de controle no início de cada fala e
/// desliga no fim. O compressor faz as rampas (attack/release).
pub fn duck_commands(segments: &[(f64, f64)], duration: f64) -> String {
    let mut lines: Vec<String> = Vec::new();
    for &(start, end) in segments {
        let on = (start - DUCK_PRE_ROLL).max(0.0);
        let off = end.min(duration);
        if off <= on {
            continue;
        }
        lines.push(format!("{on:.3} volume@gate volume 1;"));
        lines.push(format!("{off:.3} volume@gate volume 0;"));
    }
    format!("{}\n", lines.join("\n"))
}

/// Arquivo temporário do asendcmd, apagado ao sair de escopo.
struct CommandFile(tempfile::NamedTempFile);

fn write_command_file(content: &str) -> ToolResult<CommandFile> {
    let io_error = |error: std::io::Error| {
        ToolError::new(
            format!("Não foi possível gravar o arquivo de comandos do ducking: {error}"),
            ErrorCode::FfmpegFailed,
        )
    };
    let mut handle = tempfile::Builder::new()
        .suffix(".cmd")
        .tempfile()
        .map_err(io_error)?;
    handle.write_all(content.as_bytes()).map_err(io_error)?;
    handle.flush().map_err(io_error)?;
    Ok(CommandFile(handle))
}

/// Localiza a fala do vídeo para o ducking.
fn locate_speech(
    runtime: &Runtime,
    source: &Path,
    duration: f64,
    method: SpeechMethod,
) -> ToolResult<speech::SpeechDetection> {
    speech::detect_speech(
        &runtime.ffmpeg,
        source,
        duration,
        method,
        &DbOptions {
            threshold_db: -35.0,
            min_silence: DUCK_MIN_SILENCE,
        },
        &VadOptions {
            min_silence: DUCK_MIN_SILENCE,
            speech_pad: 0.1,
            ..VadOptions::default()
        },
    )
}

/// Filtro do ducking guiado pela fala: um tom constante ligado só durante a
/// fala vira o sidechain do compressor, então a música abaixa exatamente
/// `duck_db` quando alguém fala, com attack e release suaves, sem depender do
/// volume da voz.
fn ducking_filter(chain: &str, command_file: &Path, level: f64) -> String {
    let commands = escape_filter_path(command_file);
    format!(
        "[1:a]{chain}[music];\
         sine=frequency=1000:sample_rate=48000,asendcmd=f='{commands}',volume@gate=0[sc];\
         [music][sc]sidechaincompress=threshold={DUCK_THRESHOLD}:ratio={DUCK_RATIO}\
         :attack=30:release=500:makeup=1:level_sc={level:.4}[ducked];\
         [0:a][ducked]amix=inputs=2:duration=first:dropout_transition=0:normalize=0[aout]"
    )
}

#[allow(clippy::too_many_arguments)] // espelha a assinatura da tool Python
fn do_add_music(
    runtime: &Runtime,
    path: &str,
    music_path: &str,
    music_volume: f64,
    ducking: DuckingOptions,
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
    validate(
        music_volume,
        fade_in,
        fade_out,
        start,
        duration,
        ducking.duck_db,
    )?;
    let chain = music_chain(music_volume, fade_in, fade_out, start, duration, do_loop);
    let has_voice = info.has_audio;
    let mut ducking_method: Option<String> = None;
    let mut speech_segments = 0usize;
    let mut note: Option<String> = None;
    // O arquivo de comandos precisa viver até o ffmpeg terminar.
    let mut command_file: Option<CommandFile> = None;
    let filter_expr = if !has_voice {
        format!("[1:a]{chain},apad[aout]")
    } else if ducking.enabled {
        let detection = locate_speech(runtime, &source, duration, ducking.method)?;
        note = detection.note;
        speech_segments = detection.segments.len();
        if detection.segments.is_empty() {
            note = Some(
                "Nenhuma fala encontrada no vídeo; a música ficou no volume fixo, sem ducking."
                    .to_string(),
            );
            format!(
                "[1:a]{chain}[music];\
                 [0:a][music]amix=inputs=2:duration=first:dropout_transition=0:normalize=0[aout]"
            )
        } else {
            ducking_method = Some(detection.method.to_string());
            let level = gate_level(ducking.duck_db);
            let file = write_command_file(&duck_commands(&detection.segments, duration))?;
            let expr = ducking_filter(&chain, file.0.path(), level);
            command_file = Some(file);
            expr
        }
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
    drop(command_file);
    Ok(AddBackgroundMusicResult {
        output: runtime.workspace.relative(&output),
        music: runtime.workspace.relative(&music),
        music_volume,
        ducking: ducking_method.is_some(),
        duck_db: ducking.duck_db,
        ducking_method,
        speech_segments,
        looped: do_loop,
        fade_in,
        fade_out,
        duration: round_to(duration, 3),
        note,
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
    ducking: DuckingOptions,
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
    /// Quantos dB a música abaixa durante a fala (1 a 40). 12 é o padrão de
    /// podcast e narração; 6 é sutil; 20 deixa a música quase inaudível na fala.
    #[serde(default = "default_duck_db")]
    pub duck_db: f64,
    /// Como localizar a fala para o ducking: "auto" (rede neural Silero VAD,
    /// com fallback), "vad" ou "db" (limiar de volume).
    #[serde(default)]
    pub method: SpeechMethod,
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

fn default_duck_db() -> f64 {
    DEFAULT_DUCK_DB
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
         music_volume. Com ducking=true a fala é localizada por uma rede neural \
         (Silero VAD) e a música abaixa duck_db decibéis um pouco antes de cada frase, \
         voltando suavemente nas pausas, como um editor faria na mão: ruído de fundo e \
         respiração não disparam o ducking e a redução é sempre a mesma, independente \
         do volume da voz. A fala original é preservada. O vídeo não é re-encodado, só \
         o áudio.\n\n\
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
                DuckingOptions {
                    enabled: params.ducking,
                    duck_db: params.duck_db,
                    method: params.method,
                },
                params.fade_in,
                params.fade_out,
                params.start,
                params.do_loop,
                params.background,
            ))
        },
    );
}
