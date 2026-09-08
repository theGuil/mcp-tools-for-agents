//! Tool `extract_frame`: salva um frame do vídeo como imagem.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::numbers::format_g;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

/// Formato da imagem gerada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Png,
    Jpg,
}

impl ImageFormat {
    /// Extensão do arquivo, sem ponto.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpg => "jpg",
        }
    }
}

/// Imagem gerada.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExtractFrameResult {
    pub output: String,
    pub time: f64,
    pub format: ImageFormat,
    pub size_bytes: u64,
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Instante inválido, arquivo inexistente ou falha do ffmpeg.
pub fn extract_frame(
    runtime: &Runtime,
    path: &str,
    time: f64,
    image_format: ImageFormat,
) -> ToolResult<ExtractFrameResult> {
    let source = runtime.workspace.existing(path)?;
    if time < 0.0 {
        return Err(ToolError::new(
            "time deve ser >= 0.",
            ErrorCode::InvalidArgument,
        ));
    }
    let info = runtime.ffmpeg.probe(&source)?;
    if time > info.duration {
        return Err(ToolError::new(
            format!("time={time}s ultrapassa a duração ({:.2}s).", info.duration),
            ErrorCode::InvalidArgument,
        ));
    }
    let output = runtime.workspace.output_for(
        &source,
        &format!("frame_{}", format_g(time)),
        Some(image_format.as_str()),
    );
    let mut args = ffargs!["-ss", time.to_string(), "-i", source, "-frames:v", "1"];
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(ExtractFrameResult {
        output: runtime.workspace.relative(&output),
        time,
        format: image_format,
        size_bytes: std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0),
    })
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// Instante em segundos.
    pub time: f64,
    /// png ou jpg.
    #[serde(default = "default_format")]
    pub image_format: ImageFormat,
}

fn default_format() -> ImageFormat {
    ImageFormat::Png
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "extract_frame",
        "Extrai um único frame do vídeo no instante informado e salva como imagem.\n\n\
         Útil para conferir visualmente o conteúdo antes de cortar.",
        move |params: Params| {
            guarded(extract_frame(
                &runtime,
                &params.path,
                params.time,
                params.image_format,
            ))
        },
    );
}
