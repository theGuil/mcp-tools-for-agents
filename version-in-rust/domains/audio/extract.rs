//! Tool `extract_audio`: separa a trilha de áudio de um vídeo.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

/// Formato do arquivo de áudio gerado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    Mp3,
    Wav,
    Aac,
    Flac,
}

impl AudioFormat {
    /// Nome do formato, que também é a extensão do arquivo.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Aac => "aac",
            Self::Flac => "flac",
        }
    }

    /// Argumentos de codec do ffmpeg para o formato.
    fn codec_args(self) -> &'static [&'static str] {
        match self {
            Self::Mp3 => &["-c:a", "libmp3lame", "-q:a", "2"],
            Self::Wav => &["-c:a", "pcm_s16le"],
            Self::Aac => &["-c:a", "aac"],
            Self::Flac => &["-c:a", "flac"],
        }
    }
}

/// Arquivo de áudio gerado.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExtractAudioResult {
    pub output: String,
    pub format: AudioFormat,
    pub duration: f64,
}

fn do_extract(
    runtime: &Runtime,
    path: &str,
    audio_format: AudioFormat,
) -> ToolResult<ExtractAudioResult> {
    let source = runtime.workspace.existing(path)?;
    let info = runtime.ffmpeg.probe(&source)?;
    if !info.has_audio {
        return Err(ToolError::with_hint(
            format!("'{path}' não possui trilha de áudio."),
            ErrorCode::InvalidArgument,
            "Confira com probe_video antes de extrair.",
        ));
    }
    let output = runtime
        .workspace
        .output_for(&source, "audio", Some(audio_format.as_str()));
    let mut args = ffargs!["-i", source, "-vn"];
    args.extend(
        audio_format
            .codec_args()
            .iter()
            .map(std::ffi::OsString::from),
    );
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(ExtractAudioResult {
        output: runtime.workspace.relative(&output),
        format: audio_format,
        duration: info.duration,
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Arquivo inexistente, sem trilha de áudio ou falha do ffmpeg.
pub fn extract_audio(
    runtime: &Arc<Runtime>,
    path: &str,
    audio_format: AudioFormat,
    background: bool,
) -> ToolResult<MaybeJob<ExtractAudioResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("extract_audio", move || {
                do_extract(&runtime_job, &path, audio_format)
            }),
        ));
    }
    Ok(MaybeJob::Done(do_extract(runtime, path, audio_format)?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// mp3, wav, aac ou flac.
    #[serde(default = "default_format")]
    pub audio_format: AudioFormat,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_format() -> AudioFormat {
    AudioFormat::Mp3
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "extract_audio",
        "Extrai a trilha de áudio de um vídeo para um arquivo separado.",
        move |params: Params| {
            guarded(extract_audio(
                &runtime,
                &params.path,
                params.audio_format,
                params.background,
            ))
        },
    );
}
