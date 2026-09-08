//! Tool `detect_scenes`: encontra mudanças de cena em um vídeo.

use std::sync::{Arc, OnceLock};

use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::core::numbers::round_to;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

const DEFAULT_THRESHOLD: f64 = 0.4;

fn pts_time() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"pts_time:(?P<t>[0-9]+(?:\.[0-9]+)?)").expect("regex válida"))
}

/// Um trecho contínuo entre duas mudanças de cena.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Scene {
    pub index: usize,
    pub start: f64,
    pub end: f64,
    pub duration: f64,
}

/// Cenas encontradas em um vídeo.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DetectScenesResult {
    pub path: String,
    pub threshold: f64,
    pub scenes: Vec<Scene>,
    pub count: usize,
}

fn do_detect(runtime: &Runtime, path: &str, threshold: f64) -> ToolResult<DetectScenesResult> {
    if !(threshold > 0.0 && threshold <= 1.0) {
        return Err(ToolError::with_hint(
            format!("threshold={threshold} fora do intervalo (0, 1]."),
            ErrorCode::InvalidArgument,
            "Valores típicos: 0.3 (sensível) a 0.6 (só cortes bruscos).",
        ));
    }
    let source = runtime.workspace.existing(path)?;
    let info = runtime.ffmpeg.probe(&source)?;
    let stderr = runtime.ffmpeg.run(&ffargs![
        "-i",
        source,
        "-vf",
        format!("select='gt(scene,{threshold})',showinfo"),
        "-an",
        "-f",
        "null",
        "-"
    ])?;
    let mut cuts: Vec<f64> = pts_time()
        .captures_iter(&stderr)
        .filter_map(|m| m.name("t").and_then(|t| t.as_str().parse().ok()))
        .collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut boundaries = vec![0.0];
    boundaries.extend(cuts);
    boundaries.push(info.duration);
    let scenes: Vec<Scene> = boundaries
        .windows(2)
        .filter(|pair| pair[1] > pair[0])
        .enumerate()
        .map(|(index, pair)| Scene {
            index,
            start: round_to(pair[0], 3),
            end: round_to(pair[1], 3),
            duration: round_to(pair[1] - pair[0], 3),
        })
        .collect();
    Ok(DetectScenesResult {
        path: runtime.workspace.relative(&source),
        threshold,
        count: scenes.len(),
        scenes,
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Threshold fora de (0, 1], arquivo inexistente ou falha do ffmpeg.
pub fn detect_scenes(
    runtime: &Arc<Runtime>,
    path: &str,
    threshold: f64,
    background: bool,
) -> ToolResult<MaybeJob<DetectScenesResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("detect_scenes", move || {
                do_detect(&runtime_job, &path, threshold)
            }),
        ));
    }
    Ok(MaybeJob::Done(do_detect(runtime, path, threshold)?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// Sensibilidade entre 0 e 1. Menor detecta mais cenas.
    #[serde(default = "default_threshold")]
    pub threshold: f64,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_threshold() -> f64 {
    DEFAULT_THRESHOLD
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "detect_scenes",
        "Detecta mudanças de cena e devolve os intervalos de cada cena.\n\n\
         Use para decidir onde cortar. Percorre o vídeo inteiro, então em vídeos \
         longos prefira background=true.",
        move |params: Params| {
            guarded(detect_scenes(
                &runtime,
                &params.path,
                params.threshold,
                params.background,
            ))
        },
    );
}
