//! Erros com mensagem clara para o agente.
//!
//! Um agente de IA não sabe ler stack trace. Toda tool devolve, em caso de
//! falha, um objeto `{"error": ..., "code": ..., "hint": ...}` que o modelo
//! consegue interpretar e usar para se corrigir sozinho.
//!
//! O equivalente do decorator `@guarded` da versão Python é o tipo
//! [`ToolResult`]: a função pura devolve `Err(ToolError)` e o registro da
//! tool (em `domains`) converte o erro em [`ErrorPayload`] antes de responder.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Categoria estável do erro, para o agente ramificar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    InvalidArgument,
    OutsideWorkspace,
    FfmpegFailed,
    Timeout,
    JobNotFound,
    JobNotFinished,
    Unavailable,
    DownloadFailed,
}

impl ErrorCode {
    /// Nome do código como aparece no JSON devolvido ao agente.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::InvalidArgument => "invalid_argument",
            Self::OutsideWorkspace => "outside_workspace",
            Self::FfmpegFailed => "ffmpeg_failed",
            Self::Timeout => "timeout",
            Self::JobNotFound => "job_not_found",
            Self::JobNotFinished => "job_not_finished",
            Self::Unavailable => "unavailable",
            Self::DownloadFailed => "download_failed",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Formato único de erro devolvido por qualquer tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub error: String,
    pub code: ErrorCode,
    pub hint: Option<String>,
}

/// Falha esperada de uma tool, com orientação para o agente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    /// O que deu errado, em uma frase.
    pub message: String,
    /// Categoria do erro, estável para o agente ramificar.
    pub code: ErrorCode,
    /// O que o agente pode fazer para resolver.
    pub hint: Option<String>,
}

impl ToolError {
    /// Cria o erro sem `hint`.
    pub fn new(message: impl Into<String>, code: ErrorCode) -> Self {
        Self {
            message: message.into(),
            code,
            hint: None,
        }
    }

    /// Cria o erro com `hint`.
    pub fn with_hint(message: impl Into<String>, code: ErrorCode, hint: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code,
            hint: Some(hint.into()),
        }
    }

    /// Serializa para o objeto devolvido ao agente.
    pub fn to_payload(&self) -> ErrorPayload {
        ErrorPayload {
            error: self.message.clone(),
            code: self.code,
            hint: self.hint.clone(),
        }
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ToolError {}

/// Resultado de toda função pura de tool. `Err` vira `ErrorPayload` no registro.
pub type ToolResult<T> = Result<T, ToolError>;

/// Resposta serializada de uma tool: sucesso tipado ou o payload de erro.
///
/// É o que o servidor devolve ao agente. `untagged` faz o JSON ser exatamente
/// o objeto de sucesso ou exatamente `{error, code, hint}`, sem envelope.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Guarded<T> {
    Ok(T),
    Err(ErrorPayload),
}

impl<T> From<ToolResult<T>> for Guarded<T> {
    fn from(result: ToolResult<T>) -> Self {
        match result {
            Ok(value) => Self::Ok(value),
            Err(error) => Self::Err(error.to_payload()),
        }
    }
}

/// Converte `Err(ToolError)` em `ErrorPayload`, preservando o sucesso tipado.
///
/// Aplicado em toda função exposta como tool, no `register`. Qualquer outra
/// falha (panic) é bug e não erro de uso, por isso não passa por aqui.
pub fn guarded<T>(result: ToolResult<T>) -> Guarded<T> {
    result.into()
}
