//! Wrapper tipado para `ffmpeg` e `ffprobe`.
//!
//! Toda operação de vídeo e áudio passa por aqui. O restante do código nunca
//! monta linha de comando nem interpreta saída bruta.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::binaries::{Binaries, ExternalTool};
use crate::core::errors::{ErrorCode, ToolError, ToolResult};
use crate::core::process::{self, ProcessError};

const STDERR_TAIL: usize = 1500;

/// Monta a lista de argumentos do ffmpeg a partir de itens heterogêneos
/// (`&str`, `String`, `&Path`, `PathBuf`...), como a lista do Python.
#[macro_export]
macro_rules! ffargs {
    ($($item:expr),* $(,)?) => {
        vec![$(::std::ffi::OsString::from(::std::convert::AsRef::<::std::ffi::OsStr>::as_ref(&$item))),*]
    };
}

/// Metadados essenciais de um arquivo de mídia.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeResult {
    pub duration: f64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub fps: Option<f64>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub has_video: bool,
    pub has_audio: bool,
    pub size_bytes: u64,
    pub format_name: String,
}

/// Executa binários do FFmpeg com timeout e erros amigáveis.
#[derive(Debug, Clone)]
pub struct FFmpeg {
    pub ffmpeg_bin: String,
    pub ffprobe_bin: String,
    pub timeout_seconds: f64,
    pub binaries: Binaries,
}

impl Default for FFmpeg {
    fn default() -> Self {
        Self::new("ffmpeg", "ffprobe", 600.0, Binaries::default())
    }
}

impl FFmpeg {
    /// Cria o wrapper com os nomes (ou caminhos) dos binários e o timeout por chamada.
    pub fn new(
        ffmpeg_bin: impl Into<String>,
        ffprobe_bin: impl Into<String>,
        timeout_seconds: f64,
        binaries: Binaries,
    ) -> Self {
        Self {
            ffmpeg_bin: ffmpeg_bin.into(),
            ffprobe_bin: ffprobe_bin.into(),
            timeout_seconds,
            binaries,
        }
    }

    /// Indica se os dois binários já estão disponíveis, sem baixar nada.
    pub fn is_available(&self) -> bool {
        self.binaries
            .locate(ExternalTool::Ffmpeg, &self.ffmpeg_bin)
            .is_some()
            && self
                .binaries
                .locate(ExternalTool::Ffprobe, &self.ffprobe_bin)
                .is_some()
    }

    /// Garante os dois binários, baixando se a configuração permitir.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Unavailable`] se ffmpeg ou ffprobe não forem encontrados.
    pub fn require(&self) -> ToolResult<(PathBuf, PathBuf)> {
        let ffmpeg = self.binaries.ensure(ExternalTool::Ffmpeg, &self.ffmpeg_bin);
        let ffprobe = self
            .binaries
            .ensure(ExternalTool::Ffprobe, &self.ffprobe_bin);
        match (ffmpeg, ffprobe) {
            (Ok(ffmpeg), Ok(ffprobe)) => Ok((ffmpeg, ffprobe)),
            (Err(error), _) | (_, Err(error)) if error.code == ErrorCode::DownloadFailed => {
                Err(error)
            }
            _ => Err(ToolError::with_hint(
                "ffmpeg/ffprobe não encontrados no PATH.",
                ErrorCode::Unavailable,
                "Instale o FFmpeg, ajuste FFMPEG_BIN e FFPROBE_BIN, ou deixe \
                 MCP_AUTO_DOWNLOAD=true para o servidor baixar sozinho.",
            )),
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs_f64(self.timeout_seconds.max(0.001))
    }

    /// Roda `ffmpeg` com os argumentos dados e devolve o stderr.
    ///
    /// O stderr é onde o FFmpeg escreve progresso e informações de filtros.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::FfmpegFailed`] se o processo falhar, [`ErrorCode::Timeout`]
    /// se exceder o timeout.
    pub fn run<I, S>(&self, args: I) -> ToolResult<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let (ffmpeg, _) = self.require()?;
        let mut full: Vec<std::ffi::OsString> = ffargs!["-hide_banner", "-nostdin", "-y"];
        full.extend(args.into_iter().map(|a| a.as_ref().to_os_string()));
        let output = match process::run(&ffmpeg, &full, self.timeout()) {
            Ok(output) => output,
            Err(ProcessError::Timeout(_)) => {
                return Err(ToolError::with_hint(
                    format!("ffmpeg excedeu o timeout de {:.0}s.", self.timeout_seconds),
                    ErrorCode::Timeout,
                    "Use background=true para operações longas ou aumente FFMPEG_TIMEOUT.",
                ))
            }
            Err(ProcessError::Spawn(error)) => {
                return Err(ToolError::with_hint(
                    format!("ffmpeg não pôde ser executado: {error}"),
                    ErrorCode::Unavailable,
                    "Confira FFMPEG_BIN ou apague a pasta de cache para baixar de novo.",
                ))
            }
        };
        if !output.success() {
            return Err(ToolError::with_hint(
                format!(
                    "ffmpeg falhou (código {}): {}",
                    output
                        .status_code
                        .map_or("?".to_string(), |c| c.to_string()),
                    tail(&output.stderr, STDERR_TAIL)
                ),
                ErrorCode::FfmpegFailed,
                "Verifique se o arquivo de entrada é válido com probe_video.",
            ));
        }
        Ok(output.stderr)
    }

    /// Lê metadados de um arquivo de mídia via `ffprobe`.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::FfmpegFailed`] se o ffprobe falhar ou a saída não puder ser
    /// interpretada; [`ErrorCode::Timeout`] se exceder o timeout.
    pub fn probe(&self, path: &Path) -> ToolResult<ProbeResult> {
        let (_, ffprobe) = self.require()?;
        let args = ffargs![
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            path
        ];
        let output = match process::run(&ffprobe, &args, self.timeout()) {
            Ok(output) => output,
            Err(ProcessError::Timeout(_)) => {
                return Err(ToolError::new(
                    "ffprobe excedeu o timeout.",
                    ErrorCode::Timeout,
                ))
            }
            Err(ProcessError::Spawn(error)) => {
                return Err(ToolError::with_hint(
                    format!("ffprobe não pôde ser executado: {error}"),
                    ErrorCode::Unavailable,
                    "Confira FFPROBE_BIN ou apague a pasta de cache para baixar de novo.",
                ))
            }
        };
        if !output.success() {
            return Err(ToolError::with_hint(
                format!("ffprobe falhou: {}", tail(&output.stderr, STDERR_TAIL)),
                ErrorCode::FfmpegFailed,
                "O arquivo pode estar corrompido ou não ser um vídeo/áudio.",
            ));
        }
        parse_probe(&output.stdout, path)
    }
}

/// Últimos `limit` caracteres de `text`, sem espaços nas pontas.
fn tail(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    text.chars()
        .skip(count.saturating_sub(limit))
        .collect::<String>()
        .trim()
        .to_string()
}

/// Interpreta o JSON do `ffprobe`. Público para os testes, como `_parse_probe`.
///
/// # Errors
///
/// [`ErrorCode::FfmpegFailed`] se o JSON for inválido ou sem `format`/`streams`.
pub fn parse_probe(raw: &str, path: &Path) -> ToolResult<ProbeResult> {
    let data: Value = serde_json::from_str(raw).map_err(|_| {
        ToolError::new(
            "Saída do ffprobe não é JSON válido.",
            ErrorCode::FfmpegFailed,
        )
    })?;
    let (Some(fmt), Some(streams)) = (
        data.get("format").and_then(Value::as_object),
        data.get("streams").and_then(Value::as_array),
    ) else {
        return Err(ToolError::new(
            "Saída do ffprobe sem 'format' ou 'streams'.",
            ErrorCode::FfmpegFailed,
        ));
    };
    let stream_of = |kind: &str| {
        streams
            .iter()
            .filter_map(Value::as_object)
            .find(|s| s.get("codec_type").and_then(Value::as_str) == Some(kind))
    };
    let video = stream_of("video");
    let audio = stream_of("audio");
    let size_bytes = as_int(fmt.get("size"))
        .and_then(|v| u64::try_from(v).ok())
        .or_else(|| std::fs::metadata(path).ok().map(|m| m.len()))
        .unwrap_or(0);
    Ok(ProbeResult {
        duration: as_float(fmt.get("duration")).unwrap_or(0.0),
        width: video.and_then(|v| as_int(v.get("width"))),
        height: video.and_then(|v| as_int(v.get("height"))),
        fps: video.and_then(|v| as_fps(v.get("avg_frame_rate"))),
        video_codec: video.and_then(|v| as_str(v.get("codec_name"))),
        audio_codec: audio.and_then(|a| as_str(a.get("codec_name"))),
        has_video: video.is_some(),
        has_audio: audio.is_some(),
        size_bytes,
        format_name: as_str(fmt.get("format_name")).unwrap_or_default(),
    })
}

fn as_float(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn as_int(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn as_str(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

fn as_fps(value: Option<&Value>) -> Option<f64> {
    let raw = value?.as_str()?;
    if raw.is_empty() || raw == "0/0" {
        return None;
    }
    let fps = match raw.split_once('/') {
        Some((num, den)) => {
            let num: f64 = num.trim().parse().ok()?;
            let den: f64 = den.trim().parse().ok()?;
            if den == 0.0 {
                return None;
            }
            num / den
        }
        None => raw.trim().parse().ok()?,
    };
    Some((fps * 1000.0).round() / 1000.0)
}
