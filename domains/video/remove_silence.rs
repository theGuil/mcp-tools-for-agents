//! Tool `remove_silence`: corta os trechos em que ninguém fala.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::core::numbers::round_to;
use crate::core::speech::{self, DbOptions, SpeechMethod};
use crate::core::vad::VadOptions;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

pub use crate::core::speech::parse_silences;

const DEFAULT_THRESHOLD_DB: f64 = -30.0;
const DEFAULT_MIN_SILENCE: f64 = 0.5;
const DEFAULT_MARGIN: f64 = 0.2;
const MIN_SEGMENT: f64 = 0.05;

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
    /// Método que decidiu onde há fala: "vad" (rede neural) ou "db" (limiar).
    pub method: String,
    pub threshold_db: f64,
    pub min_silence: f64,
    pub margin: f64,
    pub original_duration: f64,
    pub duration: f64,
    pub removed_duration: f64,
    pub silences_removed: usize,
    pub segments: Vec<Segment>,
    /// Aviso quando o método pedido não pôde ser usado.
    pub note: Option<String>,
}

/// Como a tool decide onde há fala.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemoveSilenceOptions {
    pub method: SpeechMethod,
    pub threshold_db: f64,
    pub min_silence: f64,
    pub margin: f64,
}

impl Default for RemoveSilenceOptions {
    fn default() -> Self {
        Self {
            method: SpeechMethod::Auto,
            threshold_db: DEFAULT_THRESHOLD_DB,
            min_silence: DEFAULT_MIN_SILENCE,
            margin: DEFAULT_MARGIN,
        }
    }
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

/// Aplica a margem aos trechos de fala e funde os que se sobrepõem.
pub fn pad_and_merge(speech: &[(f64, f64)], duration: f64, margin: f64) -> Vec<Segment> {
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for &(start, end) in speech {
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

/// Inverte os silêncios, aplica a margem e funde trechos que se sobrepõem.
pub fn speech_segments(silences: &[(f64, f64)], duration: f64, margin: f64) -> Vec<Segment> {
    pad_and_merge(
        &speech::invert_silences(silences, duration),
        duration,
        margin,
    )
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

fn do_remove(
    runtime: &Runtime,
    path: &str,
    options: RemoveSilenceOptions,
) -> ToolResult<RemoveSilenceResult> {
    let RemoveSilenceOptions {
        method,
        threshold_db,
        min_silence,
        margin,
    } = options;
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

    let detection = speech::detect_speech(
        &runtime.ffmpeg,
        &source,
        duration,
        method,
        &DbOptions {
            threshold_db,
            min_silence,
        },
        &VadOptions {
            // A margem já é aplicada abaixo; o VAD só precisa juntar as pausas curtas.
            min_silence,
            speech_pad: 0.0,
            ..VadOptions::default()
        },
    )?;
    let segments = pad_and_merge(&detection.segments, duration, margin);
    if segments.is_empty() {
        let hint = if detection.method == "vad" {
            "Nenhuma voz reconhecida. Se o áudio não é fala (música, ambiente), use method='db'."
        } else {
            "Aumente threshold_db (ex.: -40) ou confira o áudio com extract_audio."
        };
        return Err(ToolError::with_hint(
            "Nenhum trecho com fala foi encontrado no arquivo.",
            ErrorCode::InvalidArgument,
            hint,
        ));
    }
    let silences_removed = segments.len() - 1
        + usize::from(segments[0].start > 0.0)
        + usize::from(segments[segments.len() - 1].end < duration - MIN_SEGMENT);

    let output = runtime.workspace.output_for(&source, "nosilence", None);
    // O ffmpeg 8 removeu `-filter_complex_script`, então o mesmo script vai
    // inline: o parser de filtergraph ignora as quebras de linha.
    let mut args = ffargs![
        "-i",
        source,
        "-filter_complex",
        filter_script(&segments, info.has_video)
    ];
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
        method: detection.method.to_string(),
        threshold_db,
        min_silence,
        margin,
        original_duration: round_to(duration, 3),
        duration: round_to(kept, 3),
        removed_duration: round_to((duration - kept).max(0.0), 3),
        silences_removed,
        segments,
        note: detection.note,
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
    options: RemoveSilenceOptions,
    background: bool,
) -> ToolResult<MaybeJob<RemoveSilenceResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("remove_silence", move || {
                do_remove(&runtime_job, &path, options)
            }),
        ));
    }
    Ok(MaybeJob::Done(do_remove(runtime, path, options)?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo ou áudio de origem, relativo ao workspace.
    pub path: String,
    /// Como decidir onde há fala. "auto" (padrão) usa a rede neural Silero VAD,
    /// que reconhece voz mesmo com ruído ou música de fundo, e cai para o
    /// limiar de dB se o binário não tiver o modelo. "vad" exige a rede neural.
    /// "db" usa só o nível de volume (threshold_db), útil para áudio sem voz.
    #[serde(default)]
    pub method: SpeechMethod,
    /// Nível abaixo do qual o áudio conta como silêncio, em dB. Só vale com
    /// method="db" (ou no fallback). -30 serve para a maioria das gravações;
    /// use -40 se cortar fala baixa.
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
         Use em aulas, palestras, podcasts e gravações de tela para tirar os trechos \
         em que o locutor fica em silêncio. Por padrão a fala é reconhecida por uma \
         rede neural (Silero VAD), então ruído de ar-condicionado, música de fundo e \
         respiração não contam como fala e não atrapalham o corte; method=\"db\" \
         volta ao limiar de volume clássico. O original não é modificado. O \
         resultado é re-encodado para que os cortes caiam exatamente onde a fala \
         começa e termina, então prefira background=true em vídeos longos e \
         acompanhe com job_status. Devolve o arquivo gerado, o método usado, quanto \
         tempo foi removido e a lista dos trechos mantidos.",
        move |params: Params| {
            guarded(remove_silence(
                &runtime,
                &params.path,
                RemoveSilenceOptions {
                    method: params.method,
                    threshold_db: params.threshold_db,
                    min_silence: params.min_silence,
                    margin: params.margin,
                },
                params.background,
            ))
        },
    );
}
