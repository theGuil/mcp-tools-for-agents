//! Tool `remove_silence`: corta os trechos em que ninguém fala.

use std::io::Write;
use std::sync::{Arc, OnceLock};

use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::core::numbers::round_to;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

const DEFAULT_THRESHOLD_DB: f64 = -30.0;
const DEFAULT_MIN_SILENCE: f64 = 0.5;
const DEFAULT_MARGIN: f64 = 0.2;
const MIN_SEGMENT: f64 = 0.05;

fn silence_start() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"silence_start:\s*(?P<t>-?[0-9]+(?:\.[0-9]+)?)").expect("regex válida")
    })
}

fn silence_end() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"silence_end:\s*(?P<t>-?[0-9]+(?:\.[0-9]+)?)").expect("regex válida")
    })
}

/// Trecho com fala mantido no resultado.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub duration: f64,
}

/// Arquivo gerado sem os silêncios.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RemoveSilenceResult {
    pub output: String,
    pub threshold_db: f64,
    pub min_silence: f64,
    pub margin: f64,
    pub original_duration: f64,
    pub duration: f64,
    pub removed_duration: f64,
    pub silences_removed: usize,
    pub segments: Vec<Segment>,
}

fn validate(threshold_db: f64, min_silence: f64, margin: f64) -> ToolResult<()> {
    if threshold_db >= 0.0 {
        return Err(ToolError::with_hint(
            format!("threshold_db={threshold_db} deve ser negativo."),
            ErrorCode::InvalidArgument,
            "Use valores em dBFS como -30 (padrão) ou -40 para ambientes silenciosos.",
        ));
    }
    if min_silence <= 0.0 {
        return Err(ToolError::with_hint(
            format!("min_silence={min_silence} deve ser maior que zero."),
            ErrorCode::InvalidArgument,
            "Valores típicos: 0.3 a 1.0 segundos.",
        ));
    }
    if margin < 0.0 {
        return Err(ToolError::with_hint(
            format!("margin={margin} não pode ser negativa."),
            ErrorCode::InvalidArgument,
            "Use 0 para nenhuma margem ou algo como 0.2 segundos.",
        ));
    }
    Ok(())
}

fn times(re: &Regex, stderr: &str) -> Vec<f64> {
    re.captures_iter(stderr)
        .filter_map(|m| m.name("t").and_then(|t| t.as_str().parse().ok()))
        .collect()
}

/// Extrai os intervalos de silêncio reportados pelo `silencedetect`.
pub fn parse_silences(stderr: &str, duration: f64) -> Vec<(f64, f64)> {
    let starts = times(silence_start(), stderr);
    let mut ends = times(silence_end(), stderr);
    if ends.len() < starts.len() {
        ends.push(duration);
    }
    starts
        .into_iter()
        .zip(ends)
        .map(|(s, e)| (s.max(0.0), e.min(duration)))
        .filter(|(s, e)| e > s)
        .collect()
}

/// Inverte os silêncios, aplica a margem e funde trechos que se sobrepõem.
pub fn speech_segments(silences: &[(f64, f64)], duration: f64, margin: f64) -> Vec<Segment> {
    let mut raw: Vec<(f64, f64)> = Vec::new();
    let mut cursor = 0.0;
    for &(s_start, s_end) in silences {
        if s_start > cursor {
            raw.push((cursor, s_start));
        }
        cursor = cursor.max(s_end);
    }
    if cursor < duration {
        raw.push((cursor, duration));
    }

    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (start, end) in raw {
        let a = (start - margin).max(0.0);
        let b = (end + margin).min(duration);
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }

    merged
        .into_iter()
        .filter(|(a, b)| b - a >= MIN_SEGMENT)
        .map(|(a, b)| Segment {
            start: round_to(a, 3),
            end: round_to(b, 3),
            duration: round_to(b - a, 3),
        })
        .collect()
}

/// Monta o filter_complex que recorta e concatena os trechos com fala.
fn filter_script(segments: &[Segment], has_video: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut labels = String::new();
    for (i, seg) in segments.iter().enumerate() {
        let (start, end) = (seg.start, seg.end);
        if has_video {
            lines.push(format!(
                "[0:v]trim=start={start}:end={end},setpts=PTS-STARTPTS[v{i}];"
            ));
            labels.push_str(&format!("[v{i}]"));
        }
        lines.push(format!(
            "[0:a]atrim=start={start}:end={end},asetpts=PTS-STARTPTS[a{i}];"
        ));
        labels.push_str(&format!("[a{i}]"));
    }
    let outputs = if has_video { "[v][a]" } else { "[a]" };
    lines.push(format!(
        "{labels}concat=n={}:v={}:a=1{outputs}",
        segments.len(),
        u8::from(has_video)
    ));
    lines.join("\n")
}

fn io_error(error: &std::io::Error) -> ToolError {
    ToolError::new(
        format!("Não foi possível criar o arquivo temporário: {error}"),
        ErrorCode::FfmpegFailed,
    )
}

fn do_remove(
    runtime: &Runtime,
    path: &str,
    threshold_db: f64,
    min_silence: f64,
    margin: f64,
) -> ToolResult<RemoveSilenceResult> {
    validate(threshold_db, min_silence, margin)?;
    let source = runtime.workspace.existing(path)?;
    let info = runtime.ffmpeg.probe(&source)?;
    if !info.has_audio {
        return Err(ToolError::with_hint(
            "O arquivo não tem trilha de áudio, então não há silêncio a detectar.",
            ErrorCode::InvalidArgument,
            "Use probe_video para conferir os streams do arquivo.",
        ));
    }
    let duration = info.duration;

    let stderr = runtime.ffmpeg.run(&ffargs![
        "-i",
        source,
        "-af",
        format!("silencedetect=noise={threshold_db}dB:d={min_silence}"),
        "-vn",
        "-f",
        "null",
        "-"
    ])?;
    let silences = parse_silences(&stderr, duration);
    let segments = speech_segments(&silences, duration, margin);
    if segments.is_empty() {
        return Err(ToolError::with_hint(
            "Nenhum trecho com fala foi encontrado: o arquivo inteiro está abaixo do limiar.",
            ErrorCode::InvalidArgument,
            "Aumente threshold_db (ex.: -40) ou confira o áudio com extract_audio.",
        ));
    }

    let output = runtime.workspace.output_for(&source, "nosilence", None);
    let mut script = tempfile::Builder::new()
        .suffix(".txt")
        .tempfile()
        .map_err(|e| io_error(&e))?;
    script
        .write_all(filter_script(&segments, info.has_video).as_bytes())
        .map_err(|e| io_error(&e))?;
    script.flush().map_err(|e| io_error(&e))?;
    let mut args = ffargs!["-i", source, "-filter_complex_script", script.path()];
    if info.has_video {
        args.extend(ffargs![
            "-map", "[v]", "-map", "[a]", "-c:v", "libx264", "-c:a", "aac"
        ]);
    } else {
        args.extend(ffargs!["-map", "[a]", "-c:a", "aac"]);
    }
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;

    let kept: f64 = segments.iter().map(|seg| seg.duration).sum();
    Ok(RemoveSilenceResult {
        output: runtime.workspace.relative(&output),
        threshold_db,
        min_silence,
        margin,
        original_duration: round_to(duration, 3),
        duration: round_to(kept, 3),
        removed_duration: round_to((duration - kept).max(0.0), 3),
        silences_removed: silences.len(),
        segments,
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Parâmetros inválidos, arquivo inexistente ou sem áudio, ou falha do ffmpeg.
pub fn remove_silence(
    runtime: &Arc<Runtime>,
    path: &str,
    threshold_db: f64,
    min_silence: f64,
    margin: f64,
    background: bool,
) -> ToolResult<MaybeJob<RemoveSilenceResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("remove_silence", move || {
                do_remove(&runtime_job, &path, threshold_db, min_silence, margin)
            }),
        ));
    }
    Ok(MaybeJob::Done(do_remove(
        runtime,
        path,
        threshold_db,
        min_silence,
        margin,
    )?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo ou áudio de origem, relativo ao workspace.
    pub path: String,
    /// Nível abaixo do qual o áudio conta como silêncio, em dB.
    /// -30 serve para a maioria das gravações; use -40 se cortar fala baixa.
    #[serde(default = "default_threshold_db")]
    pub threshold_db: f64,
    /// Duração mínima, em segundos, para uma pausa ser removida.
    #[serde(default = "default_min_silence")]
    pub min_silence: f64,
    /// Segundos preservados antes e depois de cada fala para não
    /// cortar o início ou o fim das palavras.
    #[serde(default = "default_margin")]
    pub margin: f64,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_threshold_db() -> f64 {
    DEFAULT_THRESHOLD_DB
}

fn default_min_silence() -> f64 {
    DEFAULT_MIN_SILENCE
}

fn default_margin() -> f64 {
    DEFAULT_MARGIN
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "remove_silence",
        "Remove as pausas sem fala de um vídeo ou áudio e salva um novo arquivo.\n\n\
         Use em aulas, palestras e gravações de tela para tirar os trechos em que \
         o locutor fica em silêncio. O original não é modificado. O resultado é \
         re-encodado para que os cortes caiam exatamente onde a fala começa e \
         termina, então prefira background=true em vídeos longos e acompanhe com \
         job_status. Devolve o arquivo gerado, quanto tempo foi removido e a lista \
         dos trechos mantidos.",
        move |params: Params| {
            guarded(remove_silence(
                &runtime,
                &params.path,
                params.threshold_db,
                params.min_silence,
                params.margin,
                params.background,
            ))
        },
    );
}
