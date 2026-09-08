//! Tool `create_subtitles`: gera um arquivo .srt a partir de trechos com tempos.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::numbers::round_to;
use crate::domains::{McpServer, Runtime};

/// Um trecho de legenda.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct SubtitleSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Arquivo .srt gerado.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CreateSubtitlesResult {
    pub output: String,
    pub segments: usize,
    pub duration: f64,
}

/// Converte segundos para o formato `HH:MM:SS,mmm` do SRT.
pub fn format_timestamp(seconds: f64) -> String {
    // `round()` do Python arredonda meio para o par; aqui basta o inteiro mais
    // próximo, com o mesmo resultado para tempos reais de legenda.
    let total_ms = (seconds * 1000.0).round().max(0.0) as u64;
    let hours = total_ms / 3_600_000;
    let rest = total_ms % 3_600_000;
    let minutes = rest / 60_000;
    let rest = rest % 60_000;
    let secs = rest / 1000;
    let millis = rest % 1000;
    format!("{hours:02}:{minutes:02}:{secs:02},{millis:03}")
}

/// Monta o conteúdo SRT validando os trechos.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] se algum trecho tiver tempos ou texto inválidos.
pub fn build_srt(segments: &[SubtitleSegment]) -> ToolResult<String> {
    if segments.is_empty() {
        return Err(ToolError::with_hint(
            "segments está vazio.",
            ErrorCode::InvalidArgument,
            "Envie ao menos um trecho {start, end, text}.",
        ));
    }
    let mut blocks: Vec<String> = Vec::with_capacity(segments.len());
    for (position, seg) in segments.iter().enumerate() {
        let index = position + 1;
        let (start, end, text) = (seg.start, seg.end, seg.text.trim());
        if start < 0.0 || end <= start {
            return Err(ToolError::with_hint(
                format!("Trecho {index} com intervalo inválido: start={start}, end={end}."),
                ErrorCode::InvalidArgument,
                "start deve ser >= 0 e end maior que start, em segundos.",
            ));
        }
        if text.is_empty() {
            return Err(ToolError::new(
                format!("Trecho {index} sem texto."),
                ErrorCode::InvalidArgument,
            ));
        }
        blocks.push(format!(
            "{index}\n{} --> {}\n{text}\n",
            format_timestamp(start),
            format_timestamp(end)
        ));
    }
    Ok(blocks.join("\n"))
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Extensão diferente de .srt, trechos inválidos ou caminho fora do workspace.
pub fn create_subtitles(
    runtime: &Runtime,
    segments: &[SubtitleSegment],
    output: &str,
) -> ToolResult<CreateSubtitlesResult> {
    let target = runtime.workspace.resolve(output)?;
    let suffix = target
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if suffix != "srt" {
        return Err(ToolError::with_hint(
            format!("output '{output}' deve terminar em .srt."),
            ErrorCode::InvalidArgument,
            "Ex: legendas/video.srt",
        ));
    }
    let content = build_srt(segments)?;
    let write = target
        .parent()
        .map(std::fs::create_dir_all)
        .transpose()
        .and_then(|_| std::fs::write(&target, content));
    write.map_err(|error| {
        ToolError::new(
            format!("Não foi possível gravar '{output}': {error}"),
            ErrorCode::InvalidArgument,
        )
    })?;
    let max_end = segments.iter().map(|s| s.end).fold(f64::MIN, f64::max);
    Ok(CreateSubtitlesResult {
        output: runtime.workspace.relative(&target),
        segments: segments.len(),
        duration: round_to(max_end, 3),
    })
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Lista de {start, end, text}, tempos em segundos.
    pub segments: Vec<SubtitleSegment>,
    /// Caminho do .srt a criar, relativo ao workspace.
    pub output: String,
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "create_subtitles",
        "Cria um arquivo de legendas .srt a partir de trechos com início, fim e texto.\n\n\
         Use com os segments de transcribe_audio (corrigindo ou traduzindo o texto), \
         ou escreva as legendas você mesmo. Depois aplique no vídeo com burn_subtitles.",
        move |params: Params| guarded(create_subtitles(&runtime, &params.segments, &params.output)),
    );
}
