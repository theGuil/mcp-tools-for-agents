//! Onde há fala num arquivo de mídia, por rede neural (Silero VAD) ou por
//! limiar de dB (`silencedetect`).
//!
//! É o que `remove_silence` usa para decidir o que cortar e o que
//! `add_background_music` usa para abaixar a música. As duas abordagens
//! devolvem a mesma lista de trechos `(início, fim)` em segundos, então a
//! tool não precisa saber qual foi usada.

use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{ErrorCode, ToolError, ToolResult};
use crate::core::ffmpeg::FFmpeg;
use crate::core::vad::{self, VadOptions};
use crate::ffargs;

/// Como detectar a fala.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum SpeechMethod {
    /// Rede neural se o binário tiver o modelo; senão limiar de dB.
    #[default]
    Auto,
    /// Silero VAD: reconhece voz mesmo com ruído ou música de fundo.
    Vad,
    /// `silencedetect`: tudo abaixo de `threshold_db` conta como silêncio.
    Db,
}

/// Ajustes do limiar de dB.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DbOptions {
    /// Nível abaixo do qual o áudio conta como silêncio (negativo, em dBFS).
    pub threshold_db: f64,
    /// Duração mínima de uma pausa para ela ser considerada silêncio.
    pub min_silence: f64,
}

/// Resultado da detecção.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeechDetection {
    /// Trechos com fala `(início, fim)`, em segundos, em ordem.
    pub segments: Vec<(f64, f64)>,
    /// Método realmente usado: `"vad"` ou `"db"`.
    pub method: &'static str,
    /// Aviso quando o método pedido não pôde ser usado ou não achou fala.
    pub note: Option<String>,
}

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

/// Inverte os silêncios: o que sobra é fala.
pub fn invert_silences(silences: &[(f64, f64)], duration: f64) -> Vec<(f64, f64)> {
    let mut speech: Vec<(f64, f64)> = Vec::new();
    let mut cursor = 0.0;
    for &(start, end) in silences {
        if start > cursor {
            speech.push((cursor, start));
        }
        cursor = cursor.max(end);
    }
    if cursor < duration {
        speech.push((cursor, duration));
    }
    speech
}

/// Trechos de fala pelo `silencedetect`.
///
/// # Errors
///
/// Falha do ffmpeg.
pub fn detect_by_db(
    ffmpeg: &FFmpeg,
    path: &Path,
    duration: f64,
    options: &DbOptions,
) -> ToolResult<Vec<(f64, f64)>> {
    let stderr = ffmpeg.run(ffargs![
        "-i",
        path,
        "-af",
        format!(
            "silencedetect=noise={}dB:d={}",
            options.threshold_db, options.min_silence
        ),
        "-vn",
        "-f",
        "null",
        "-"
    ])?;
    Ok(invert_silences(
        &parse_silences(&stderr, duration),
        duration,
    ))
}

/// Decodifica a trilha de áudio em PCM mono 16 kHz, como o Silero espera.
///
/// # Errors
///
/// Falha do ffmpeg ou de leitura do arquivo temporário.
pub fn decode_pcm_16k(ffmpeg: &FFmpeg, path: &Path) -> ToolResult<Vec<f32>> {
    let io_error = |error: std::io::Error| {
        ToolError::new(
            format!("Não foi possível preparar o áudio para o VAD: {error}"),
            ErrorCode::FfmpegFailed,
        )
    };
    let raw = tempfile::Builder::new()
        .suffix(".pcm")
        .tempfile()
        .map_err(io_error)?;
    ffmpeg.run(ffargs![
        "-i",
        path,
        "-vn",
        "-sn",
        "-dn",
        "-ac",
        "1",
        "-ar",
        vad::SAMPLE_RATE.to_string(),
        "-f",
        "s16le",
        raw.path()
    ])?;
    let mut bytes = Vec::new();
    std::fs::File::open(raw.path())
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(io_error)?;
    Ok(vad::pcm16_to_f32(&bytes))
}

/// Trechos de fala pelo Silero VAD.
///
/// # Errors
///
/// [`ErrorCode::Unavailable`] sem a feature `vad`; falha do ffmpeg.
pub fn detect_by_vad(
    ffmpeg: &FFmpeg,
    path: &Path,
    duration: f64,
    options: &VadOptions,
) -> ToolResult<Vec<(f64, f64)>> {
    if !vad::is_available() {
        return Err(vad::unavailable());
    }
    let samples = decode_pcm_16k(ffmpeg, path)?;
    Ok(vad::detect_speech(&samples, options)?
        .into_iter()
        .map(|(a, b)| (a.min(duration), b.min(duration)))
        .filter(|(a, b)| b > a)
        .collect())
}

/// Detecta a fala pelo método pedido, com o fallback do modo `auto`.
///
/// Em `auto`, usa o VAD quando disponível; se ele não achar fala nenhuma
/// (vídeo só com música, por exemplo), cai para o limiar de dB e avisa em
/// `note`. Em `vad` sem o modelo, devolve erro.
///
/// # Errors
///
/// [`ErrorCode::Unavailable`] se `method = vad` sem a feature; falha do ffmpeg.
pub fn detect_speech(
    ffmpeg: &FFmpeg,
    path: &Path,
    duration: f64,
    method: SpeechMethod,
    db: &DbOptions,
    vad_options: &VadOptions,
) -> ToolResult<SpeechDetection> {
    let use_vad = match method {
        SpeechMethod::Db => false,
        SpeechMethod::Vad => true,
        SpeechMethod::Auto => vad::is_available(),
    };
    if use_vad {
        let segments = detect_by_vad(ffmpeg, path, duration, vad_options)?;
        if !segments.is_empty() || method == SpeechMethod::Vad {
            return Ok(SpeechDetection {
                segments,
                method: "vad",
                note: None,
            });
        }
    }
    let segments = detect_by_db(ffmpeg, path, duration, db)?;
    let note = match method {
        SpeechMethod::Auto if vad::is_available() => Some(
            "O VAD não encontrou voz; a detecção usou o limiar de dB (method='db').".to_string(),
        ),
        SpeechMethod::Auto => Some(
            "Binário sem a feature 'vad'; a detecção usou o limiar de dB (method='db')."
                .to_string(),
        ),
        _ => None,
    };
    Ok(SpeechDetection {
        segments,
        method: "db",
        note,
    })
}
