//! Tool `transcribe_audio`: transcreve fala com timestamps.
//!
//! Depende da feature opcional `transcribe` (whisper.cpp via `whisper-rs`).
//! Sem ela a tool existe, mas devolve um erro explicando como habilitar.

use std::io::Read;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::core::whisper;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

/// Uma palavra com seus tempos, para legendas dinâmicas.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptWord {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

/// Um trecho de fala com seus tempos.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscriptSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub words: Vec<TranscriptWord>,
}

/// Transcrição completa.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranscribeResult {
    pub path: String,
    pub model: String,
    pub language: Option<String>,
    pub segments: Vec<TranscriptSegment>,
    pub text: String,
}

fn io_error(what: &str, error: &std::io::Error) -> ToolError {
    ToolError::new(
        format!("Não foi possível {what}: {error}"),
        ErrorCode::FfmpegFailed,
    )
}

/// Decodifica o áudio para PCM float 16 kHz mono, o formato que o whisper espera.
fn decode_pcm(runtime: &Runtime, source: &std::path::Path) -> ToolResult<Vec<f32>> {
    let temp = tempfile::Builder::new()
        .prefix("transcribe_")
        .suffix(".raw")
        .tempfile()
        .map_err(|error| io_error("criar o arquivo temporário do áudio", &error))?;
    let mut args = ffargs!["-i", source, "-vn", "-ac", "1", "-ar", "16000", "-f", "f32le"];
    args.push(temp.path().as_os_str().to_os_string());
    runtime.ffmpeg.run(&args)?;
    let mut bytes = Vec::new();
    std::fs::File::open(temp.path())
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(|error| io_error("ler o áudio decodificado", &error))?;
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

fn do_transcribe(
    runtime: &Runtime,
    path: &str,
    model_size: &str,
    language: Option<&str>,
    word_timestamps: bool,
) -> ToolResult<TranscribeResult> {
    let source = runtime.workspace.existing(path)?;
    let model_path = whisper::ensure_model(
        model_size,
        &runtime.settings.cache_dir,
        runtime.settings.auto_download,
    )?;
    let pcm = decode_pcm(runtime, &source)?;
    let raw_segments = whisper::transcribe(&model_path, &pcm, language, word_timestamps)?;
    let segments: Vec<TranscriptSegment> = raw_segments
        .into_iter()
        .map(|segment| TranscriptSegment {
            start: segment.start,
            end: segment.end,
            text: segment.text,
            words: if word_timestamps {
                segment
                    .words
                    .into_iter()
                    .map(|word| TranscriptWord {
                        start: word.start,
                        end: word.end,
                        word: word.word,
                    })
                    .collect()
            } else {
                Vec::new()
            },
        })
        .collect();
    let text = segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(TranscribeResult {
        path: runtime.workspace.relative(&source),
        model: model_size.to_string(),
        language: language.map(str::to_string),
        segments,
        text,
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Arquivo inexistente, `model_size` inválido, feature `transcribe` desligada,
/// modelo indisponível ou falha do ffmpeg/whisper.
pub fn transcribe_audio(
    runtime: &Arc<Runtime>,
    path: &str,
    model_size: &str,
    language: Option<&str>,
    word_timestamps: bool,
    background: bool,
) -> ToolResult<MaybeJob<TranscribeResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        let model_size = model_size.to_string();
        let language = language.map(str::to_string);
        return Ok(MaybeJob::Job(runtime.jobs.submit(
            "transcribe_audio",
            move || {
                do_transcribe(
                    &runtime_job,
                    &path,
                    &model_size,
                    language.as_deref(),
                    word_timestamps,
                )
            },
        )));
    }
    Ok(MaybeJob::Done(do_transcribe(
        runtime,
        path,
        model_size,
        language,
        word_timestamps,
    )?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Áudio ou vídeo, relativo ao workspace.
    pub path: String,
    /// Modelo whisper: tiny, base, small, medium ou large-v3.
    #[serde(default = "default_model_size")]
    pub model_size: String,
    /// Código do idioma (pt, en). Detecta automaticamente se omitido.
    #[serde(default)]
    pub language: Option<String>,
    /// Inclui o tempo de cada palavra dentro de cada segment.
    #[serde(default = "default_true")]
    pub word_timestamps: bool,
    /// Executa como job e devolve job_id.
    #[serde(default = "default_true")]
    pub background: bool,
}

fn default_model_size() -> String {
    "base".to_string()
}

fn default_true() -> bool {
    true
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "transcribe_audio",
        "Transcreve a fala de um áudio ou vídeo, com tempos de cada trecho e de cada palavra.\n\n\
         Combine com cut_video para cortar por conteúdo falado, com create_subtitles \
         para legenda comum e com create_dynamic_subtitles para legenda animada \
         palavra por palavra (estilo TikTok). Cada segment traz words, uma lista \
         {start, end, word}, quando word_timestamps=true. É lento, por isso roda em \
         background por padrão: pegue o resultado com job_result.",
        move |params: Params| {
            guarded(transcribe_audio(
                &runtime,
                &params.path,
                &params.model_size,
                params.language.as_deref(),
                params.word_timestamps,
                params.background,
            ))
        },
    );
}
