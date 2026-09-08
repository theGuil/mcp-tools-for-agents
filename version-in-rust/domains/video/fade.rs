//! Tool `add_fade`: fade de entrada e saída na imagem e no som.

use std::ffi::OsString;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::core::numbers::{format_g, round_to};
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

const MAX_FADE: f64 = 30.0;

/// Cor do fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FadeColor {
    Black,
    White,
}

impl FadeColor {
    /// Nome da cor como o ffmpeg entende.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Black => "black",
            Self::White => "white",
        }
    }
}

/// Vídeo gerado com os fades.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AddFadeResult {
    pub output: String,
    pub fade_in: f64,
    pub fade_out: f64,
    pub color: FadeColor,
    pub audio_faded: bool,
    pub duration: f64,
}

fn validate(fade_in: f64, fade_out: f64) -> ToolResult<()> {
    if fade_in < 0.0 || fade_out < 0.0 {
        return Err(ToolError::new(
            "fade_in e fade_out devem ser >= 0.",
            ErrorCode::InvalidArgument,
        ));
    }
    if fade_in == 0.0 && fade_out == 0.0 {
        return Err(ToolError::with_hint(
            "Informe fade_in ou fade_out maior que zero.",
            ErrorCode::InvalidArgument,
            "Ex: fade_in=0.5, fade_out=1.0.",
        ));
    }
    if fade_in > MAX_FADE || fade_out > MAX_FADE {
        return Err(ToolError::new(
            format!(
                "fade_in e fade_out devem ter no máximo {} segundos.",
                format_g(MAX_FADE)
            ),
            ErrorCode::InvalidArgument,
        ));
    }
    Ok(())
}

fn audio_args(audio_steps: &[String], fade_audio: bool, has_audio: bool) -> Vec<OsString> {
    if fade_audio {
        return ffargs!["-af", audio_steps.join(","), "-c:a", "aac"];
    }
    if has_audio {
        return ffargs!["-c:a", "copy"];
    }
    Vec::new()
}

fn do_fade(
    runtime: &Runtime,
    path: &str,
    fade_in: f64,
    fade_out: f64,
    color: FadeColor,
    audio: bool,
) -> ToolResult<AddFadeResult> {
    let source = runtime.workspace.existing(path)?;
    validate(fade_in, fade_out)?;
    let info = runtime.ffmpeg.probe(&source)?;
    if !info.has_video {
        return Err(ToolError::with_hint(
            format!("'{path}' não tem trilha de vídeo."),
            ErrorCode::InvalidArgument,
            "Para áudio puro use normalize_audio ou add_background_music.",
        ));
    }
    let duration = info.duration;
    if fade_in + fade_out > duration {
        return Err(ToolError::with_hint(
            format!(
                "fade_in + fade_out ({}s) é maior que o vídeo ({duration:.2}s).",
                format_g(fade_in + fade_out)
            ),
            ErrorCode::InvalidArgument,
            "Use probe_video para conferir a duração e reduza os fades.",
        ));
    }
    let color_name = color.as_str();
    let mut video_steps: Vec<String> = Vec::new();
    let mut audio_steps: Vec<String> = Vec::new();
    if fade_in > 0.0 {
        video_steps.push(format!("fade=t=in:st=0:d={fade_in:.3}:color={color_name}"));
        audio_steps.push(format!("afade=t=in:st=0:d={fade_in:.3}"));
    }
    if fade_out > 0.0 {
        let out_start = (duration - fade_out).max(0.0);
        video_steps.push(format!(
            "fade=t=out:st={out_start:.3}:d={fade_out:.3}:color={color_name}"
        ));
        audio_steps.push(format!("afade=t=out:st={out_start:.3}:d={fade_out:.3}"));
    }
    let fade_audio = audio && info.has_audio;
    let audio_args = audio_args(&audio_steps, fade_audio, info.has_audio);
    let output = runtime.workspace.output_for(&source, "fade", None);
    let mut args = ffargs![
        "-i",
        source,
        "-vf",
        video_steps.join(","),
        "-c:v",
        "libx264",
        "-preset",
        "fast",
        "-pix_fmt",
        "yuv420p"
    ];
    args.extend(audio_args);
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(AddFadeResult {
        output: runtime.workspace.relative(&output),
        fade_in,
        fade_out,
        color,
        audio_faded: fade_audio,
        duration: round_to(duration, 3),
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Argumento inválido, arquivo inexistente ou falha do ffmpeg.
pub fn add_fade(
    runtime: &Arc<Runtime>,
    path: &str,
    fade_in: f64,
    fade_out: f64,
    color: FadeColor,
    audio: bool,
    background: bool,
) -> ToolResult<MaybeJob<AddFadeResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        return Ok(MaybeJob::Job(runtime.jobs.submit("add_fade", move || {
            do_fade(&runtime_job, &path, fade_in, fade_out, color, audio)
        })));
    }
    Ok(MaybeJob::Done(do_fade(
        runtime, path, fade_in, fade_out, color, audio,
    )?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// Segundos do fade de entrada. 0 desativa.
    #[serde(default = "default_fade_in")]
    pub fade_in: f64,
    /// Segundos do fade de saída. 0 desativa.
    #[serde(default = "default_fade_out")]
    pub fade_out: f64,
    /// Cor do fade: "black" (padrão) ou "white".
    #[serde(default = "default_color")]
    pub color: FadeColor,
    /// Aplica o mesmo fade no áudio.
    #[serde(default = "default_true")]
    pub audio: bool,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_fade_in() -> f64 {
    0.5
}

fn default_fade_out() -> f64 {
    1.0
}

fn default_color() -> FadeColor {
    FadeColor::Black
}

fn default_true() -> bool {
    true
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "add_fade",
        "Aplica fade de entrada (do preto para a imagem) e de saída (da imagem para o preto).\n\n\
         Use como último acabamento antes de exportar: evita começo e fim bruscos, \
         que são a marca de um corte amador. O som acompanha o fade por padrão \
         (audio=true). Passe 0 em fade_in ou fade_out para aplicar só um dos dois. \
         O original não é modificado; o vídeo é re-encodado.\n\n\
         Para transições ENTRE clipes use concat_videos com transition=\"fade\".",
        move |params: Params| {
            guarded(add_fade(
                &runtime,
                &params.path,
                params.fade_in,
                params.fade_out,
                params.color,
                params.audio,
                params.background,
            ))
        },
    );
}
