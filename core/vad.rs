//! Detecção de fala (VAD) com o Silero VAD, para cortes e ducking robustos.
//!
//! O limiar de dB do `silencedetect` confunde ruído de fundo, música e
//! respiração com fala. O Silero VAD é uma rede neural de ~1 MB, treinada em
//! milhares de horas de voz em mais de 6 000 idiomas, que diz a cada 32 ms se
//! há alguém falando. O modelo ONNX (versão 16 kHz, opset 15, licença MIT) vai
//! embutido no binário via `include_bytes!` e roda no `tract`, o mesmo runtime
//! Rust puro do YuNet: nada de onnxruntime instalado no sistema. Depende da
//! feature opcional `vad`; sem ela [`detect_speech`] devolve um erro claro e as
//! tools voltam para o limiar de dB.

use crate::core::errors::{ErrorCode, ToolError, ToolResult};

/// Modelo Silero VAD embutido no binário.
pub const MODEL_BYTES: &[u8] = include_bytes!("models/silero_vad_16k.onnx");
/// Taxa de amostragem que o modelo espera.
pub const SAMPLE_RATE: usize = 16_000;
/// Janela de análise em amostras (32 ms a 16 kHz).
pub const WINDOW: usize = 512;
/// Amostras da janela anterior que o modelo recebe junto com a atual.
#[cfg_attr(not(feature = "vad"), allow(dead_code))]
const CONTEXT: usize = 64;
#[cfg_attr(not(feature = "vad"), allow(dead_code))]
const STATE_LEN: usize = 128;

/// Ajustes da detecção, em segundos e probabilidade.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VadOptions {
    /// Probabilidade a partir da qual a janela conta como fala (0 a 1).
    pub threshold: f32,
    /// Fala mais curta que isso é descartada.
    pub min_speech: f64,
    /// Pausa mais curta que isso não separa dois trechos de fala.
    pub min_silence: f64,
    /// Margem adicionada antes e depois de cada trecho.
    pub speech_pad: f64,
}

impl Default for VadOptions {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            min_speech: 0.25,
            min_silence: 0.1,
            speech_pad: 0.03,
        }
    }
}

/// Indica se o binário foi compilado com o modelo.
pub const fn is_available() -> bool {
    cfg!(feature = "vad")
}

/// Trechos de fala `(início, fim)` em segundos, em áudio mono 16 kHz.
///
/// # Errors
///
/// [`ErrorCode::Unavailable`] sem a feature `vad` ou se o modelo falhar.
pub fn detect_speech(samples: &[f32], options: &VadOptions) -> ToolResult<Vec<(f64, f64)>> {
    #[cfg(feature = "vad")]
    {
        let probabilities = silero::probabilities(samples)?;
        Ok(timestamps(&probabilities, samples.len(), options))
    }
    #[cfg(not(feature = "vad"))]
    {
        let _ = (samples, options);
        Err(unavailable())
    }
}

/// Erro padrão quando o binário não tem o modelo.
pub fn unavailable() -> ToolError {
    ToolError::with_hint(
        "Detecção de fala por rede neural indisponível: binário compilado sem a feature 'vad'.",
        ErrorCode::Unavailable,
        "Use o binário completo (cargo build --release --features full) ou method='db'.",
    )
}

/// Converte a probabilidade de cada janela em trechos de fala, seguindo o
/// `get_speech_timestamps` oficial do Silero.
pub fn timestamps(
    probabilities: &[f32],
    total_samples: usize,
    options: &VadOptions,
) -> Vec<(f64, f64)> {
    let to_samples = |seconds: f64| (seconds * SAMPLE_RATE as f64).round().max(0.0) as usize;
    let min_speech = to_samples(options.min_speech);
    let min_silence = to_samples(options.min_silence);
    let pad = to_samples(options.speech_pad);
    let threshold = options.threshold;
    let neg_threshold = (threshold - 0.15).max(0.01);

    let mut segments: Vec<(usize, usize)> = Vec::new();
    let mut triggered = false;
    let mut start = 0usize;
    let mut temp_end = 0usize;
    for (index, &probability) in probabilities.iter().enumerate() {
        let position = index * WINDOW;
        if probability >= threshold && temp_end != 0 {
            temp_end = 0;
        }
        if probability >= threshold && !triggered {
            triggered = true;
            start = position;
            continue;
        }
        if probability < neg_threshold && triggered {
            if temp_end == 0 {
                temp_end = position;
            }
            if position - temp_end < min_silence {
                continue;
            }
            if temp_end - start > min_speech {
                segments.push((start, temp_end));
            }
            temp_end = 0;
            triggered = false;
        }
    }
    if triggered && total_samples > start && total_samples - start > min_speech {
        segments.push((start, total_samples));
    }

    let count = segments.len();
    let mut padded: Vec<(usize, usize)> = Vec::with_capacity(count);
    for index in 0..count {
        let (mut seg_start, mut seg_end) = segments[index];
        seg_start = seg_start.saturating_sub(pad);
        if index + 1 < count {
            let gap = segments[index + 1].0.saturating_sub(seg_end);
            if gap < 2 * pad {
                seg_end += gap / 2;
                segments[index + 1].0 = segments[index + 1].0.saturating_sub(gap / 2);
            } else {
                seg_end = (seg_end + pad).min(total_samples);
            }
        } else {
            seg_end = (seg_end + pad).min(total_samples);
        }
        padded.push((seg_start, seg_end));
    }
    padded
        .into_iter()
        .filter(|(a, b)| b > a)
        .map(|(a, b)| (a as f64 / SAMPLE_RATE as f64, b as f64 / SAMPLE_RATE as f64))
        .collect()
}

/// Converte PCM s16le em amostras normalizadas de -1 a 1.
pub fn pcm16_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32_768.0)
        .collect()
}

#[cfg(feature = "vad")]
mod silero {
    //! Inferência do Silero VAD no `tract`: uma janela de 512 amostras por vez,
    //! com 64 amostras de contexto e o estado recorrente da janela anterior.

    use std::sync::{Arc, OnceLock};

    use tract_onnx::prelude::*;

    use super::{CONTEXT, MODEL_BYTES, STATE_LEN, WINDOW};
    use crate::core::errors::{ErrorCode, ToolError, ToolResult};

    type Model = Arc<TypedRunnableModel>;

    fn model() -> ToolResult<&'static Model> {
        static MODEL: OnceLock<Result<Model, String>> = OnceLock::new();
        MODEL
            .get_or_init(|| load().map_err(|error| error.to_string()))
            .as_ref()
            .map_err(|error| {
                ToolError::with_hint(
                    format!("Modelo Silero VAD não pôde ser carregado: {error}"),
                    ErrorCode::Unavailable,
                    "Reinstale o projeto; o modelo vem embutido em core/models.",
                )
            })
    }

    fn load() -> TractResult<Model> {
        let mut cursor = std::io::Cursor::new(MODEL_BYTES);
        tract_onnx::onnx()
            .model_for_read(&mut cursor)?
            .with_input_fact(0, f32::fact([1, CONTEXT + WINDOW]).into())?
            .with_input_fact(1, f32::fact([2, 1, STATE_LEN]).into())?
            .into_optimized()?
            .into_runnable()
    }

    /// Copia uma saída do modelo como vetor de `f32`.
    fn values(outputs: &TVec<TValue>, index: usize) -> ToolResult<Vec<f32>> {
        let tensor: &Tensor = &outputs[index];
        tensor
            .try_as_plain()
            .and_then(|view| view.as_slice::<f32>().map(<[f32]>::to_vec))
            .map_err(|error| ToolError::new(format!("Silero VAD: {error}"), ErrorCode::Unavailable))
    }

    /// Probabilidade de fala de cada janela de 512 amostras.
    pub fn probabilities(samples: &[f32]) -> ToolResult<Vec<f32>> {
        let model = model()?;
        let mut state = tract_ndarray::Array3::<f32>::zeros((2, 1, STATE_LEN));
        let mut context = vec![0f32; CONTEXT];
        let windows = samples.len().div_ceil(WINDOW);
        let mut out = Vec::with_capacity(windows);
        for index in 0..windows {
            let begin = index * WINDOW;
            let end = (begin + WINDOW).min(samples.len());
            let mut input = tract_ndarray::Array2::<f32>::zeros((1, CONTEXT + WINDOW));
            for (i, value) in context.iter().enumerate() {
                input[[0, i]] = *value;
            }
            for (i, value) in samples[begin..end].iter().enumerate() {
                input[[0, CONTEXT + i]] = *value;
            }
            let outputs = model
                .run(tvec!(
                    Tensor::from(input.clone()).into(),
                    Tensor::from(state.clone()).into()
                ))
                .map_err(|error| {
                    ToolError::new(
                        format!("Silero VAD falhou: {error}"),
                        ErrorCode::Unavailable,
                    )
                })?;
            let probability = values(&outputs, 0)?.first().copied().ok_or_else(|| {
                ToolError::new("Silero VAD devolveu saída vazia.", ErrorCode::Unavailable)
            })?;
            if outputs.len() > 1 {
                let next_state = values(&outputs, 1)?;
                state = tract_ndarray::Array3::from_shape_vec((2, 1, STATE_LEN), next_state)
                    .map_err(|error| {
                        ToolError::new(format!("Silero VAD: {error}"), ErrorCode::Unavailable)
                    })?;
            }
            out.push(probability);
            let last = input.as_slice().unwrap_or(&[]);
            context.copy_from_slice(&last[last.len() - CONTEXT..]);
        }
        Ok(out)
    }
}
