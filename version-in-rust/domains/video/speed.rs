//! Tool `change_speed`: acelera ou desacelera o vídeo inteiro ou um trecho.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::core::numbers::{format_g, round_to};
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

const MIN_FACTOR: f64 = 0.25;
const MAX_FACTOR: f64 = 8.0;
// atempo aceita de 0.5 a 100 por instância; abaixo de 0.5 encadeamos várias.
const ATEMPO_MIN: f64 = 0.5;
const ATEMPO_MAX: f64 = 100.0;

/// Vídeo gerado com a velocidade alterada.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChangeSpeedResult {
    pub output: String,
    pub factor: f64,
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub original_duration: f64,
    pub new_duration: f64,
}

/// Monta a cadeia `atempo` que mantém o tom da voz em qualquer fator.
pub fn atempo_chain(factor: f64) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut remaining = factor;
    while remaining < ATEMPO_MIN {
        parts.push(format!("atempo={}", format_g(ATEMPO_MIN)));
        remaining /= ATEMPO_MIN;
    }
    while remaining > ATEMPO_MAX {
        parts.push(format!("atempo={}", format_g(ATEMPO_MAX)));
        remaining /= ATEMPO_MAX;
    }
    parts.push(format!("atempo={}", format_g(remaining)));
    parts.join(",")
}

fn validate(factor: f64, start: Option<f64>, end: Option<f64>, duration: f64) -> ToolResult<()> {
    if !(MIN_FACTOR..=MAX_FACTOR).contains(&factor) {
        return Err(ToolError::with_hint(
            format!(
                "factor deve estar entre {} e {}.",
                format_g(MIN_FACTOR),
                format_g(MAX_FACTOR)
            ),
            ErrorCode::InvalidArgument,
            "2 dobra a velocidade, 0.5 deixa em câmera lenta, 1.25 acelera levemente.",
        ));
    }
    if factor == 1.0 {
        return Err(ToolError::with_hint(
            "factor=1 não altera nada.",
            ErrorCode::InvalidArgument,
            "Informe um fator diferente de 1.",
        ));
    }
    if start.is_none() != end.is_none() {
        return Err(ToolError::new(
            "Informe start e end juntos, ou nenhum dos dois para o vídeo inteiro.",
            ErrorCode::InvalidArgument,
        ));
    }
    if let (Some(start), Some(end)) = (start, end) {
        if start < 0.0 || end <= start {
            return Err(ToolError::with_hint(
                format!("Intervalo inválido: start={start}, end={end}."),
                ErrorCode::InvalidArgument,
                "start deve ser >= 0 e end maior que start, em segundos.",
            ));
        }
        if end > duration + 0.05 {
            return Err(ToolError::with_hint(
                format!("end={end}s ultrapassa a duração do vídeo ({duration:.2}s)."),
                ErrorCode::InvalidArgument,
                "Use probe_video para conferir a duração.",
            ));
        }
    }
    Ok(())
}

fn whole_filter(factor: f64, has_audio: bool) -> (String, Vec<&'static str>) {
    let video = format!("[0:v]setpts=PTS/{}[vout]", format_g(factor));
    if !has_audio {
        return (video, vec!["-map", "[vout]"]);
    }
    (
        format!("{video};[0:a]{}[aout]", atempo_chain(factor)),
        vec!["-map", "[vout]", "-map", "[aout]"],
    )
}

/// Divide em antes / trecho / depois, altera só o trecho e junta de novo.
fn segment_filter(
    factor: f64,
    start: f64,
    end: f64,
    duration: f64,
    has_audio: bool,
) -> (String, Vec<&'static str>) {
    let mut pieces: Vec<(f64, f64, f64)> = Vec::new();
    if start > 0.0 {
        pieces.push((0.0, start, 1.0));
    }
    pieces.push((start, end, factor));
    if end < duration {
        pieces.push((end, duration, 1.0));
    }
    let mut chains: Vec<String> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    for (index, (seg_start, seg_end, seg_factor)) in pieces.iter().enumerate() {
        let speed = if *seg_factor == 1.0 {
            String::new()
        } else {
            format!(",setpts=PTS/{}", format_g(*seg_factor))
        };
        chains.push(format!(
            "[0:v]trim=start={seg_start:.3}:end={seg_end:.3},setpts=PTS-STARTPTS{speed}[v{index}]"
        ));
        labels.push(format!("[v{index}]"));
        if has_audio {
            let tempo = if *seg_factor == 1.0 {
                String::new()
            } else {
                format!(",{}", atempo_chain(*seg_factor))
            };
            chains.push(format!(
                "[0:a]atrim=start={seg_start:.3}:end={seg_end:.3},asetpts=PTS-STARTPTS{tempo}[a{index}]"
            ));
            labels.push(format!("[a{index}]"));
        }
    }
    let joined = labels.concat();
    if has_audio {
        chains.push(format!(
            "{joined}concat=n={}:v=1:a=1[vout][aout]",
            pieces.len()
        ));
        return (chains.join(";"), vec!["-map", "[vout]", "-map", "[aout]"]);
    }
    chains.push(format!("{joined}concat=n={}:v=1:a=0[vout]", pieces.len()));
    (chains.join(";"), vec!["-map", "[vout]"])
}

fn do_change_speed(
    runtime: &Runtime,
    path: &str,
    factor: f64,
    start: Option<f64>,
    end: Option<f64>,
) -> ToolResult<ChangeSpeedResult> {
    let source = runtime.workspace.existing(path)?;
    let info = runtime.ffmpeg.probe(&source)?;
    if !info.has_video {
        return Err(ToolError::with_hint(
            format!("'{path}' não tem trilha de vídeo."),
            ErrorCode::InvalidArgument,
            "change_speed só se aplica a vídeos.",
        ));
    }
    let duration = info.duration;
    validate(factor, start, end, duration)?;
    let has_audio = info.has_audio;
    let (filter_expr, map_args, new_duration, end) = match (start, end) {
        (Some(start), Some(end)) => {
            let end = end.min(duration);
            let (expr, maps) = segment_filter(factor, start, end, duration, has_audio);
            (
                expr,
                maps,
                duration - (end - start) + (end - start) / factor,
                Some(end),
            )
        }
        _ => {
            let (expr, maps) = whole_filter(factor, has_audio);
            (expr, maps, duration / factor, None)
        }
    };
    let output =
        runtime
            .workspace
            .output_for(&source, &format!("speed_{}x", format_g(factor)), None);
    let mut args = ffargs!["-i", source, "-filter_complex", filter_expr];
    args.extend(map_args.iter().map(std::ffi::OsString::from));
    args.extend(ffargs![
        "-c:v", "libx264", "-preset", "fast", "-pix_fmt", "yuv420p"
    ]);
    if has_audio {
        args.extend(ffargs!["-c:a", "aac"]);
    }
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(ChangeSpeedResult {
        output: runtime.workspace.relative(&output),
        factor,
        start,
        end,
        original_duration: round_to(duration, 3),
        new_duration: round_to(new_duration, 3),
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Fator ou intervalo inválido, arquivo inexistente ou falha do ffmpeg.
pub fn change_speed(
    runtime: &Arc<Runtime>,
    path: &str,
    factor: f64,
    start: Option<f64>,
    end: Option<f64>,
    background: bool,
) -> ToolResult<MaybeJob<ChangeSpeedResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("change_speed", move || {
                do_change_speed(&runtime_job, &path, factor, start, end)
            }),
        ));
    }
    Ok(MaybeJob::Done(do_change_speed(
        runtime, path, factor, start, end,
    )?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// Multiplicador de velocidade entre 0.25 e 8. 2 = duas vezes mais
    /// rápido, 0.5 = metade da velocidade.
    pub factor: f64,
    /// Início do trecho a alterar, em segundos. Omita para o vídeo todo.
    #[serde(default)]
    pub start: Option<f64>,
    /// Fim do trecho a alterar, em segundos. Obrigatório junto com start.
    #[serde(default)]
    pub end: Option<f64>,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "change_speed",
        "Acelera ou desacelera o vídeo inteiro ou só um trecho, mantendo o tom da voz.\n\n\
         Use para dar ritmo a um corte: acelere partes lentas (factor=1.5), faça \
         um time-lapse (factor=4) ou uma câmera lenta de impacto (factor=0.5). \
         Com start e end só aquele trecho muda de velocidade e o resto fica \
         normal, tudo em um único arquivo. O áudio é ajustado sem virar \"voz de \
         esquilo\". O original não é modificado; o vídeo é re-encodado.\n\n\
         Devolve a duração antes e depois, útil para recalcular tempos de legenda.",
        move |params: Params| {
            guarded(change_speed(
                &runtime,
                &params.path,
                params.factor,
                params.start,
                params.end,
                params.background,
            ))
        },
    );
}
