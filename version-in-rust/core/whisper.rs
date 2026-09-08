//! Transcrição de fala com o whisper.cpp, usada por `transcribe_audio`.
//!
//! Usa a crate `whisper-rs` (binding do whisper.cpp) e os modelos GGML
//! oficiais (`ggml-<size>.bin`), procurados na pasta de cache ou ao lado do
//! executável e baixados sob demanda quando `MCP_AUTO_DOWNLOAD` permite.
//! Depende da feature opcional `transcribe`. Sem ela `ensure_model` e
//! `transcribe` devolvem um erro claro dizendo como habilitar.

use std::path::{Path, PathBuf};

use crate::core::errors::{ErrorCode, ToolError, ToolResult};

/// Tamanhos de modelo aceitos, os mesmos nomes do faster-whisper.
pub const MODEL_SIZES: [&str; 5] = ["tiny", "base", "small", "medium", "large-v3"];

/// Uma palavra com seus tempos em segundos.
#[derive(Debug, Clone, PartialEq)]
pub struct WhisperWord {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

/// Um trecho de fala com seus tempos em segundos e as palavras que o compõem.
#[derive(Debug, Clone, PartialEq)]
pub struct WhisperSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub words: Vec<WhisperWord>,
}

/// Nome do arquivo do modelo GGML para o tamanho dado.
pub fn model_file_name(model_size: &str) -> String {
    format!("ggml-{model_size}.bin")
}

/// Valida o tamanho do modelo pedido pelo agente.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] se o tamanho não for um dos aceitos.
pub fn validate_model_size(model_size: &str) -> ToolResult<()> {
    if MODEL_SIZES.contains(&model_size) {
        return Ok(());
    }
    Err(ToolError::with_hint(
        format!("model_size '{model_size}' não existe."),
        ErrorCode::InvalidArgument,
        format!("Use um de: {}.", MODEL_SIZES.join(", ")),
    ))
}

/// Localiza o modelo `ggml-<size>.bin`, baixando se for permitido e necessário.
///
/// Procura em `<cache_dir>/models/` e na pasta `models/` ao lado do executável.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] para tamanho desconhecido;
/// [`ErrorCode::Unavailable`] sem a feature `transcribe` ou quando não há
/// modelo nem como baixar; [`ErrorCode::DownloadFailed`] quando o download falha.
pub fn ensure_model(
    model_size: &str,
    cache_dir: &Path,
    auto_download: bool,
) -> ToolResult<PathBuf> {
    validate_model_size(model_size)?;
    #[cfg(feature = "transcribe")]
    {
        engine::ensure_model(model_size, cache_dir, auto_download)
    }
    #[cfg(not(feature = "transcribe"))]
    {
        let _ = (cache_dir, auto_download);
        Err(unavailable())
    }
}

/// Transcreve áudio PCM float 16 kHz mono com o modelo dado.
///
/// Com `word_timestamps` liga os timestamps por token e agrupa os tokens em
/// palavras. Tempos em segundos, com três casas.
///
/// # Errors
///
/// [`ErrorCode::Unavailable`] sem a feature `transcribe` ou se o modelo não
/// puder ser carregado; [`ErrorCode::InvalidArgument`] se o áudio for vazio
/// ou a inferência falhar.
pub fn transcribe(
    model_path: &Path,
    pcm: &[f32],
    language: Option<&str>,
    word_timestamps: bool,
) -> ToolResult<Vec<WhisperSegment>> {
    #[cfg(feature = "transcribe")]
    {
        engine::transcribe(model_path, pcm, language, word_timestamps)
    }
    #[cfg(not(feature = "transcribe"))]
    {
        let _ = (model_path, pcm, language, word_timestamps);
        Err(unavailable())
    }
}

#[cfg(not(feature = "transcribe"))]
fn unavailable() -> ToolError {
    ToolError::with_hint(
        "Transcrição indisponível: binário compilado sem a feature 'transcribe'.",
        ErrorCode::Unavailable,
        "Use o binário completo (cargo build --release --features full).",
    )
}

#[cfg(feature = "transcribe")]
mod engine {
    //! Localização/download do modelo e chamada do whisper.cpp.

    use std::fs;
    use std::io::{Read, Write};
    use std::path::{Path, PathBuf};
    use std::sync::Once;
    use std::time::Duration;

    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

    use super::{model_file_name, WhisperSegment, WhisperWord};
    use crate::core::errors::{ErrorCode, ToolError, ToolResult};
    use crate::core::numbers::round_to;

    const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(1800);
    const USER_AGENT: &str = "mcp-tools-for-agents";
    const MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

    /// Pastas onde o modelo pode estar: cache e ao lado do executável.
    fn search_dirs(cache_dir: &Path) -> Vec<PathBuf> {
        let mut dirs = vec![cache_dir.join("models")];
        if let Some(exe_dir) = crate::core::binaries::exe_dir() {
            dirs.push(exe_dir.join("models"));
        }
        dirs
    }

    fn locate(model_size: &str, cache_dir: &Path) -> Option<PathBuf> {
        let name = model_file_name(model_size);
        search_dirs(cache_dir)
            .into_iter()
            .map(|dir| dir.join(&name))
            .find(|candidate| candidate.is_file())
    }

    fn unavailable_model(model_size: &str, cache_dir: &Path) -> ToolError {
        let name = model_file_name(model_size);
        ToolError::with_hint(
            format!("Modelo whisper '{model_size}' não encontrado."),
            ErrorCode::Unavailable,
            format!(
                "Coloque o arquivo {name} em {} ou na pasta models/ ao lado do mcp-tools \
                 (download em {MODEL_BASE_URL}/{name}), ou deixe MCP_AUTO_DOWNLOAD=true \
                 para o servidor baixar sozinho.",
                cache_dir.join("models").display()
            ),
        )
    }

    fn download_failed(message: impl Into<String>) -> ToolError {
        ToolError::with_hint(
            message.into(),
            ErrorCode::DownloadFailed,
            "Confira a conexão com a internet ou baixe o modelo manualmente para a pasta \
             models/ do cache.",
        )
    }

    pub fn ensure_model(
        model_size: &str,
        cache_dir: &Path,
        auto_download: bool,
    ) -> ToolResult<PathBuf> {
        if let Some(found) = locate(model_size, cache_dir) {
            return Ok(found);
        }
        if !auto_download {
            return Err(unavailable_model(model_size, cache_dir));
        }
        let models_dir = cache_dir.join("models");
        fs::create_dir_all(&models_dir).map_err(|error| {
            download_failed(format!(
                "não foi possível criar {}: {error}",
                models_dir.display()
            ))
        })?;
        let name = model_file_name(model_size);
        let target = models_dir.join(&name);
        download(&format!("{MODEL_BASE_URL}/{name}"), &target)?;
        locate(model_size, cache_dir).ok_or_else(|| unavailable_model(model_size, cache_dir))
    }

    /// Baixa a URL direto para `<target>.part` e renomeia no fim, para nunca
    /// deixar um modelo pela metade com o nome final.
    fn download(url: &str, target: &Path) -> ToolResult<()> {
        let agent = crate::core::http::agent(DOWNLOAD_TIMEOUT, USER_AGENT);
        let response = agent
            .get(url)
            .call()
            .map_err(|error| download_failed(format!("falha ao baixar {url}: {error}")))?;
        let temp = target.with_extension("part");
        let write_error = |error: std::io::Error| {
            download_failed(format!(
                "não foi possível gravar {}: {error}",
                temp.display()
            ))
        };
        let mut written: u64 = 0;
        {
            let mut file = fs::File::create(&temp).map_err(write_error)?;
            let mut reader = response.into_body().into_reader();
            let mut buffer = vec![0u8; 1 << 20];
            loop {
                let read = reader
                    .read(&mut buffer)
                    .map_err(|error| download_failed(format!("falha ao ler {url}: {error}")))?;
                if read == 0 {
                    break;
                }
                file.write_all(&buffer[..read]).map_err(write_error)?;
                written += read as u64;
            }
            file.flush().map_err(write_error)?;
        }
        if written == 0 {
            let _ = fs::remove_file(&temp);
            return Err(download_failed(format!("{url} devolveu um arquivo vazio.")));
        }
        fs::rename(&temp, target).map_err(|error| {
            download_failed(format!(
                "não foi possível gravar {}: {error}",
                target.display()
            ))
        })
    }

    /// Silencia os logs do whisper.cpp/GGML uma única vez: o servidor fala
    /// pelo stdio e não quer ruído no terminal do agente.
    fn quiet_logs() {
        static ONCE: Once = Once::new();
        ONCE.call_once(whisper_rs::install_logging_hooks);
    }

    /// Token especial do whisper (`[_BEG_]`, `[_TT_123]`, `<|pt|>`), sem texto falado.
    fn is_special(token: &str) -> bool {
        token.starts_with("[_") || token.starts_with("<|")
    }

    /// Centésimos de segundo do whisper.cpp em segundos com três casas.
    fn seconds(centiseconds: i64) -> f64 {
        round_to(centiseconds as f64 / 100.0, 3)
    }

    pub fn transcribe(
        model_path: &Path,
        pcm: &[f32],
        language: Option<&str>,
        word_timestamps: bool,
    ) -> ToolResult<Vec<WhisperSegment>> {
        if pcm.is_empty() {
            return Err(ToolError::with_hint(
                "O áudio não tem amostras para transcrever.",
                ErrorCode::InvalidArgument,
                "Confira com probe_video se o arquivo tem trilha de áudio e duração maior que zero.",
            ));
        }
        quiet_logs();
        let model_text = model_path.to_string_lossy();
        let mut ctx_params = WhisperContextParameters::default();
        ctx_params.use_gpu(false);
        let context = WhisperContext::new_with_params(&model_text, ctx_params).map_err(|error| {
            ToolError::with_hint(
                format!("Modelo whisper não pôde ser carregado ({model_text}): {error}"),
                ErrorCode::Unavailable,
                "Apague o arquivo do modelo e deixe o servidor baixar de novo, ou use outro model_size.",
            )
        })?;
        let mut state = context.create_state().map_err(|error| {
            ToolError::new(
                format!("whisper não pôde iniciar o estado: {error}"),
                ErrorCode::Unavailable,
            )
        })?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(language);
        params.set_token_timestamps(word_timestamps);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_n_threads(threads());
        state.full(params, pcm).map_err(|error| {
            ToolError::with_hint(
                format!("whisper falhou ao transcrever: {error}"),
                ErrorCode::InvalidArgument,
                "Confira com probe_video se o arquivo tem trilha de áudio válida.",
            )
        })?;

        let mut segments = Vec::new();
        for segment in state.as_iter() {
            let text = segment.to_str_lossy().map(|t| t.trim().to_string());
            let Ok(text) = text else {
                continue;
            };
            let words = if word_timestamps {
                words_of(&segment)
            } else {
                Vec::new()
            };
            segments.push(WhisperSegment {
                start: seconds(segment.start_timestamp()),
                end: seconds(segment.end_timestamp()),
                text,
                words,
            });
        }
        Ok(segments)
    }

    /// Threads da inferência: todos os núcleos, no máximo 8, como o whisper.cpp sugere.
    fn threads() -> i32 {
        let available = std::thread::available_parallelism().map_or(4, |n| n.get());
        i32::try_from(available.clamp(1, 8)).unwrap_or(4)
    }

    /// Agrupa os tokens do segmento em palavras: um token que começa com
    /// espaço abre uma palavra nova, os demais continuam a anterior.
    fn words_of(segment: &whisper_rs::WhisperSegment<'_>) -> Vec<WhisperWord> {
        let mut words: Vec<(i64, i64, String)> = Vec::new();
        for index in 0..segment.n_tokens() {
            let Some(token) = segment.get_token(index) else {
                continue;
            };
            let Ok(text) = token.to_str_lossy() else {
                continue;
            };
            if is_special(&text) {
                continue;
            }
            let data = token.token_data();
            let starts_word = text.starts_with(char::is_whitespace) || words.is_empty();
            if starts_word {
                words.push((data.t0, data.t1, text.into_owned()));
            } else if let Some(last) = words.last_mut() {
                last.1 = data.t1;
                last.2.push_str(&text);
            }
        }
        words
            .into_iter()
            .filter_map(|(t0, t1, text)| {
                let word = text.trim().to_string();
                (!word.is_empty()).then(|| WhisperWord {
                    start: seconds(t0),
                    end: seconds(t1),
                    word,
                })
            })
            .collect()
    }
}
