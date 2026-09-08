//! Tool `concat_videos`: junta vários vídeos em sequência, com ou sem transição.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::ffmpeg::ProbeResult;
use crate::core::jobs::MaybeJob;
use crate::core::numbers::{format_g, round_to};
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

/// Transições do filtro `xfade` disponíveis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TransitionName {
    Fade,
    Fadeblack,
    Fadewhite,
    Dissolve,
    Wipeleft,
    Wiperight,
    Wipeup,
    Wipedown,
    Slideleft,
    Slideright,
    Slideup,
    Slidedown,
    Smoothleft,
    Smoothright,
    Circleopen,
    Circleclose,
    Radial,
    Zoomin,
    Pixelize,
    Hblur,
}

impl TransitionName {
    /// Nome da transição como o `xfade` espera.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fade => "fade",
            Self::Fadeblack => "fadeblack",
            Self::Fadewhite => "fadewhite",
            Self::Dissolve => "dissolve",
            Self::Wipeleft => "wipeleft",
            Self::Wiperight => "wiperight",
            Self::Wipeup => "wipeup",
            Self::Wipedown => "wipedown",
            Self::Slideleft => "slideleft",
            Self::Slideright => "slideright",
            Self::Slideup => "slideup",
            Self::Slidedown => "slidedown",
            Self::Smoothleft => "smoothleft",
            Self::Smoothright => "smoothright",
            Self::Circleopen => "circleopen",
            Self::Circleclose => "circleclose",
            Self::Radial => "radial",
            Self::Zoomin => "zoomin",
            Self::Pixelize => "pixelize",
            Self::Hblur => "hblur",
        }
    }
}

/// Todas as transições, na ordem da documentação.
pub const TRANSITIONS: &[TransitionName] = &[
    TransitionName::Fade,
    TransitionName::Fadeblack,
    TransitionName::Fadewhite,
    TransitionName::Dissolve,
    TransitionName::Wipeleft,
    TransitionName::Wiperight,
    TransitionName::Wipeup,
    TransitionName::Wipedown,
    TransitionName::Slideleft,
    TransitionName::Slideright,
    TransitionName::Slideup,
    TransitionName::Slidedown,
    TransitionName::Smoothleft,
    TransitionName::Smoothright,
    TransitionName::Circleopen,
    TransitionName::Circleclose,
    TransitionName::Radial,
    TransitionName::Zoomin,
    TransitionName::Pixelize,
    TransitionName::Hblur,
];
const MIN_INPUTS: usize = 2;
const MAX_TRANSITION: f64 = 5.0;
const DEFAULT_FPS: f64 = 30.0;

/// Arquivo gerado pela concatenação.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ConcatVideosResult {
    pub output: String,
    pub inputs: Vec<String>,
    pub count: usize,
    pub transition: Option<TransitionName>,
    pub transition_duration: Option<f64>,
    pub has_audio: bool,
}

fn escape_for_concat(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "'\\''")
}

fn resolve_output(
    runtime: &Runtime,
    sources: &[PathBuf],
    output_name: Option<&str>,
) -> ToolResult<PathBuf> {
    let output = match output_name {
        Some(name) if !name.is_empty() => runtime.workspace.resolve(name)?,
        _ => runtime
            .workspace
            .output_for(&sources[0], &format!("concat_{}", sources.len()), None),
    };
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            ToolError::new(
                format!("Não foi possível criar a pasta de saída: {error}"),
                ErrorCode::InvalidArgument,
            )
        })?;
    }
    Ok(output)
}

fn io_error(error: &std::io::Error) -> ToolError {
    ToolError::new(
        format!("Não foi possível criar o arquivo temporário: {error}"),
        ErrorCode::FfmpegFailed,
    )
}

fn concat_copy(runtime: &Runtime, sources: &[PathBuf], output: &Path) -> ToolResult<()> {
    let mut listing = tempfile::Builder::new()
        .suffix(".txt")
        .tempfile()
        .map_err(|e| io_error(&e))?;
    for source in sources {
        writeln!(listing, "file '{}'", escape_for_concat(source)).map_err(|e| io_error(&e))?;
    }
    listing.flush().map_err(|e| io_error(&e))?;
    let mut args = ffargs![
        "-f",
        "concat",
        "-safe",
        "0",
        "-i",
        listing.path(),
        "-c",
        "copy"
    ];
    args.push(output.to_path_buf().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(())
}

/// Instante em que cada transição começa, já descontando as sobreposições anteriores.
pub fn transition_offsets(durations: &[f64], transition_duration: f64) -> Vec<f64> {
    let mut offsets = Vec::new();
    let mut elapsed = 0.0;
    for duration in durations.iter().take(durations.len().saturating_sub(1)) {
        elapsed += duration - transition_duration;
        offsets.push(round_to(elapsed, 3));
    }
    offsets
}

fn transition_filter(
    infos: &[ProbeResult],
    transition: TransitionName,
    transition_duration: f64,
    width: i64,
    height: i64,
    fps: f64,
    with_audio: bool,
) -> String {
    let mut chains: Vec<String> = Vec::new();
    for index in 0..infos.len() {
        chains.push(format!(
            "[{index}:v]scale={width}:{height}:force_original_aspect_ratio=decrease,\
             pad={width}:{height}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={},\
             format=yuv420p,settb=AVTB[v{index}]",
            format_g(fps)
        ));
        if with_audio {
            chains.push(format!(
                "[{index}:a]aformat=sample_rates=48000:channel_layouts=stereo,\
                 asetpts=PTS-STARTPTS[a{index}]"
            ));
        }
    }
    let durations: Vec<f64> = infos.iter().map(|i| i.duration).collect();
    let offsets = transition_offsets(&durations, transition_duration);
    let mut previous = "[v0]".to_string();
    for (position, offset) in offsets.iter().enumerate() {
        let index = position + 1;
        let label = if index == offsets.len() {
            "[vout]".to_string()
        } else {
            format!("[x{index}]")
        };
        chains.push(format!(
            "{previous}[v{index}]xfade=transition={}:duration={transition_duration:.3}:offset={offset:.3}{label}",
            transition.as_str()
        ));
        previous = label;
    }
    if with_audio {
        previous = "[a0]".to_string();
        for index in 1..infos.len() {
            let label = if index == infos.len() - 1 {
                "[aout]".to_string()
            } else {
                format!("[ax{index}]")
            };
            chains.push(format!(
                "{previous}[a{index}]acrossfade=d={transition_duration:.3}:c1=tri:c2=tri{label}"
            ));
            previous = label;
        }
    }
    chains.join(";")
}

fn concat_transition(
    runtime: &Runtime,
    sources: &[PathBuf],
    output: &Path,
    transition: TransitionName,
    transition_duration: f64,
) -> ToolResult<bool> {
    if !(transition_duration > 0.0 && transition_duration <= MAX_TRANSITION) {
        return Err(ToolError::with_hint(
            format!(
                "transition_duration deve estar entre 0 e {} segundos.",
                format_g(MAX_TRANSITION)
            ),
            ErrorCode::InvalidArgument,
            "0.5 é o padrão; 1 fica mais lento e cinematográfico.",
        ));
    }
    let infos = sources
        .iter()
        .map(|s| runtime.ffmpeg.probe(s))
        .collect::<ToolResult<Vec<ProbeResult>>>()?;
    for (source, info) in sources.iter().zip(infos.iter()) {
        if !info.has_video || info.width.is_none() || info.height.is_none() {
            return Err(ToolError::new(
                format!(
                    "'{}' não tem trilha de vídeo.",
                    runtime.workspace.relative(source)
                ),
                ErrorCode::InvalidArgument,
            ));
        }
        if info.duration <= transition_duration {
            return Err(ToolError::with_hint(
                format!(
                    "'{}' dura {:.2}s, menos que a transição de {}s.",
                    runtime.workspace.relative(source),
                    info.duration,
                    format_g(transition_duration)
                ),
                ErrorCode::InvalidArgument,
                "Reduza transition_duration ou use clipes mais longos.",
            ));
        }
    }
    let first = &infos[0];
    let width = first.width.unwrap_or(0);
    let height = first.height.unwrap_or(0);
    let fps = first.fps.unwrap_or(DEFAULT_FPS);
    let with_audio = infos.iter().all(|i| i.has_audio);
    let filter_expr = transition_filter(
        &infos,
        transition,
        transition_duration,
        width,
        height,
        fps,
        with_audio,
    );
    let mut args: Vec<std::ffi::OsString> = Vec::new();
    for source in sources {
        args.extend(ffargs!["-i", source]);
    }
    args.extend(ffargs!["-filter_complex", filter_expr, "-map", "[vout]"]);
    if with_audio {
        args.extend(ffargs!["-map", "[aout]", "-c:a", "aac", "-b:a", "192k"]);
    }
    args.extend(ffargs![
        "-c:v", "libx264", "-preset", "fast", "-pix_fmt", "yuv420p"
    ]);
    args.push(output.to_path_buf().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(with_audio)
}

fn do_concat(
    runtime: &Runtime,
    paths: &[String],
    output_name: Option<&str>,
    transition: Option<TransitionName>,
    transition_duration: f64,
) -> ToolResult<ConcatVideosResult> {
    if paths.len() < MIN_INPUTS {
        return Err(ToolError::new(
            "Informe pelo menos dois vídeos para concatenar.",
            ErrorCode::InvalidArgument,
        ));
    }
    let sources = paths
        .iter()
        .map(|p| runtime.workspace.existing(p))
        .collect::<ToolResult<Vec<PathBuf>>>()?;
    let output = resolve_output(runtime, &sources, output_name)?;
    let (has_audio, applied_duration) = match transition {
        None => {
            concat_copy(runtime, &sources, &output)?;
            (runtime.ffmpeg.probe(&output)?.has_audio, None)
        }
        Some(transition) => (
            concat_transition(runtime, &sources, &output, transition, transition_duration)?,
            Some(transition_duration),
        ),
    };
    Ok(ConcatVideosResult {
        output: runtime.workspace.relative(&output),
        inputs: sources
            .iter()
            .map(|s| runtime.workspace.relative(s))
            .collect(),
        count: sources.len(),
        transition,
        transition_duration: applied_duration,
        has_audio,
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Menos de dois vídeos, transição inválida, arquivo inexistente ou falha do ffmpeg.
pub fn concat_videos(
    runtime: &Arc<Runtime>,
    paths: &[String],
    output_name: Option<&str>,
    transition: Option<TransitionName>,
    transition_duration: f64,
    background: bool,
) -> ToolResult<MaybeJob<ConcatVideosResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let paths = paths.to_vec();
        let output_name = output_name.map(str::to_string);
        return Ok(MaybeJob::Job(runtime.jobs.submit(
            "concat_videos",
            move || {
                do_concat(
                    &runtime_job,
                    &paths,
                    output_name.as_deref(),
                    transition,
                    transition_duration,
                )
            },
        )));
    }
    Ok(MaybeJob::Done(do_concat(
        runtime,
        paths,
        output_name,
        transition,
        transition_duration,
    )?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Lista de vídeos, relativa ao workspace, na ordem desejada.
    pub paths: Vec<String>,
    /// Nome do arquivo de saída. Gerado automaticamente se omitido.
    #[serde(default)]
    pub output_name: Option<String>,
    /// Nome da transição entre os clipes. Omita para corte seco.
    #[serde(default)]
    pub transition: Option<TransitionName>,
    /// Duração de cada transição em segundos (0.3 a 1 é o usual).
    #[serde(default = "default_transition_duration")]
    pub transition_duration: f64,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_transition_duration() -> f64 {
    0.5
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "concat_videos",
        "Junta dois ou mais vídeos na ordem informada, com corte seco ou com transição.\n\n\
         Sem transition: emenda direta e sem re-encode, os vídeos precisam ter o \
         mesmo codec e resolução (por exemplo, cortes do mesmo original). Com \
         transition: os clipes são redimensionados para o tamanho do primeiro e \
         emendados com o efeito escolhido, o áudio faz crossfade e o resultado é \
         re-encodado (use background=true para muitos clipes).\n\n\
         Transições disponíveis: fade (dissolve suave, a mais usada), fadeblack, \
         fadewhite, dissolve, wipeleft, wiperight, wipeup, wipedown, slideleft, \
         slideright, slideup, slidedown, smoothleft, smoothright, circleopen, \
         circleclose, radial, zoomin, pixelize, hblur. Cada transição consome \
         transition_duration segundos de cada clipe, então o resultado fica um pouco \
         mais curto que a soma. Os arquivos de entrada não são alterados.",
        move |params: Params| {
            guarded(concat_videos(
                &runtime,
                &params.paths,
                params.output_name.as_deref(),
                params.transition,
                params.transition_duration,
                params.background,
            ))
        },
    );
}
