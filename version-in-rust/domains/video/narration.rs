//! Tool `add_narration`: mistura um áudio de narração no vídeo.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

/// Vídeo gerado com a narração.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AddNarrationResult {
    pub output: String,
    pub narration: String,
    pub start: f64,
    pub original_volume: f64,
    pub replaced_audio: bool,
}

fn do_narration(
    runtime: &Runtime,
    path: &str,
    audio_path: &str,
    start: f64,
    original_volume: f64,
) -> ToolResult<AddNarrationResult> {
    let source = runtime.workspace.existing(path)?;
    let narration = runtime.workspace.existing(audio_path)?;
    if start < 0.0 {
        return Err(ToolError::new(
            "start deve ser >= 0.",
            ErrorCode::InvalidArgument,
        ));
    }
    if !(0.0..=1.0).contains(&original_volume) {
        return Err(ToolError::with_hint(
            "original_volume deve estar entre 0 e 1.",
            ErrorCode::InvalidArgument,
            "0 silencia o áudio original, 1 mantém o volume, 0.2 deixa de fundo.",
        ));
    }
    let info = runtime.ffmpeg.probe(&source)?;
    let narration_info = runtime.ffmpeg.probe(&narration)?;
    if !narration_info.has_audio {
        return Err(ToolError::with_hint(
            format!("'{audio_path}' não tem trilha de áudio."),
            ErrorCode::InvalidArgument,
            "Informe um arquivo de áudio (mp3, m4a, wav) ou um vídeo com som.",
        ));
    }
    if start >= info.duration {
        return Err(ToolError::with_hint(
            format!(
                "start={start}s ultrapassa a duração do vídeo ({:.2}s).",
                info.duration
            ),
            ErrorCode::InvalidArgument,
            "Use probe_video para conferir a duração.",
        ));
    }
    let delay_ms = (start * 1000.0).round() as i64;
    let replace = original_volume == 0.0 || !info.has_audio;
    let filter_expr = if replace {
        format!("[1:a]adelay={delay_ms}:all=1,apad[aout]")
    } else {
        format!(
            "[0:a]volume={original_volume}[bg];\
             [1:a]adelay={delay_ms}:all=1[nar];\
             [bg][nar]amix=inputs=2:duration=first:dropout_transition=0:normalize=0[aout]"
        )
    };
    let output = runtime.workspace.output_for(&source, "narrated", None);
    let mut args = ffargs![
        "-i",
        source,
        "-i",
        narration,
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
    ];
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(AddNarrationResult {
        output: runtime.workspace.relative(&output),
        narration: runtime.workspace.relative(&narration),
        start,
        original_volume,
        replaced_audio: replace,
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Argumento inválido, arquivo inexistente ou falha do ffmpeg.
pub fn add_narration(
    runtime: &Arc<Runtime>,
    path: &str,
    audio_path: &str,
    start: f64,
    original_volume: f64,
    background: bool,
) -> ToolResult<MaybeJob<AddNarrationResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        let audio_path = audio_path.to_string();
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("add_narration", move || {
                do_narration(&runtime_job, &path, &audio_path, start, original_volume)
            }),
        ));
    }
    Ok(MaybeJob::Done(do_narration(
        runtime,
        path,
        audio_path,
        start,
        original_volume,
    )?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// Áudio da narração (mp3, m4a, wav), relativo ao workspace.
    pub audio_path: String,
    /// Segundo em que a narração começa.
    #[serde(default)]
    pub start: f64,
    /// Volume do áudio original, de 0 (mudo) a 1 (igual).
    #[serde(default = "default_original_volume")]
    pub original_volume: f64,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_original_volume() -> f64 {
    0.2
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "add_narration",
        "Mistura um áudio de narração (voz, locução) por cima do som do vídeo.\n\n\
         Use quando você gerou ou recebeu um áudio com a descrição falada e quer \
         colocá-lo no vídeo. O áudio original fica de fundo no volume indicado; \
         original_volume=0 substitui o som por completo. O vídeo não é re-encodado.",
        move |params: Params| {
            guarded(add_narration(
                &runtime,
                &params.path,
                &params.audio_path,
                params.start,
                params.original_volume,
                params.background,
            ))
        },
    );
}
