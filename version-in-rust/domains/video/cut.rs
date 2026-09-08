//! Tool `cut_video`: recorta um trecho de um vídeo.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::core::numbers::{format_g, round_to};
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

/// Arquivo gerado pelo corte.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CutVideoResult {
    pub output: String,
    pub start: f64,
    pub end: f64,
    pub duration: f64,
    pub reencoded: bool,
}

fn do_cut(
    runtime: &Runtime,
    path: &str,
    start: f64,
    end: f64,
    reencode: bool,
) -> ToolResult<CutVideoResult> {
    let source = runtime.workspace.existing(path)?;
    if start < 0.0 || end <= start {
        return Err(ToolError::with_hint(
            format!("Intervalo inválido: start={start}, end={end}."),
            ErrorCode::InvalidArgument,
            "start deve ser >= 0 e end maior que start, em segundos.",
        ));
    }
    let info = runtime.ffmpeg.probe(&source)?;
    if end > info.duration + 0.05 {
        return Err(ToolError::with_hint(
            format!(
                "end={end}s ultrapassa a duração do vídeo ({:.2}s).",
                info.duration
            ),
            ErrorCode::InvalidArgument,
            "Use probe_video para conferir a duração.",
        ));
    }
    let output = runtime.workspace.output_for(
        &source,
        &format!("cut_{}-{}", format_g(start), format_g(end)),
        None,
    );
    let codec_args: Vec<&str> = if reencode {
        vec!["-c:v", "libx264", "-c:a", "aac"]
    } else {
        vec!["-c", "copy"]
    };
    let mut args = ffargs![
        "-ss",
        start.to_string(),
        "-to",
        end.to_string(),
        "-i",
        source
    ];
    args.extend(codec_args.iter().map(std::ffi::OsString::from));
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(CutVideoResult {
        output: runtime.workspace.relative(&output),
        start,
        end,
        duration: round_to(end - start, 3),
        reencoded: reencode,
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Intervalo inválido, arquivo inexistente ou falha do ffmpeg.
pub fn cut_video(
    runtime: &Arc<Runtime>,
    path: &str,
    start: f64,
    end: f64,
    reencode: bool,
    background: bool,
) -> ToolResult<MaybeJob<CutVideoResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        return Ok(MaybeJob::Job(runtime.jobs.submit("cut_video", move || {
            do_cut(&runtime_job, &path, start, end, reencode)
        })));
    }
    Ok(MaybeJob::Done(do_cut(runtime, path, start, end, reencode)?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo de origem, relativo ao workspace.
    pub path: String,
    /// Início do trecho em segundos.
    pub start: f64,
    /// Fim do trecho em segundos.
    pub end: f64,
    /// Re-encoda para corte exato (mais lento).
    #[serde(default)]
    pub reencode: bool,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "cut_video",
        "Corta o trecho entre start e end (segundos) e salva um novo arquivo.\n\n\
         O original não é modificado. Por padrão copia os streams sem re-encodar, \
         o que é rápido mas corta no keyframe mais próximo. Use reencode=true para \
         corte exato no frame. Para vídeos longos use background=true e acompanhe \
         com job_status.",
        move |params: Params| {
            guarded(cut_video(
                &runtime,
                &params.path,
                params.start,
                params.end,
                params.reencode,
                params.background,
            ))
        },
    );
}
