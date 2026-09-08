//! Tool `add_text_overlay`: escreve um texto (título, descrição) sobre o vídeo.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::fonts::{escape_drawtext, escape_filter_path, require_font};
use crate::core::jobs::MaybeJob;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

const MAX_TEXT: usize = 500;

/// Posição vertical do texto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TextPosition {
    Top,
    Center,
    Bottom,
}

impl TextPosition {
    /// Nome da posição.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Center => "center",
            Self::Bottom => "bottom",
        }
    }

    fn y_expr(self) -> &'static str {
        match self {
            Self::Top => "h*0.08",
            Self::Center => "(h-text_h)/2",
            Self::Bottom => "h-text_h-h*0.08",
        }
    }
}

/// Vídeo gerado com o texto.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AddTextOverlayResult {
    pub output: String,
    pub text: String,
    pub position: TextPosition,
    pub start: f64,
    pub end: Option<f64>,
}

/// Opções de estilo e tempo do texto.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayOptions {
    pub position: TextPosition,
    pub start: f64,
    pub end: Option<f64>,
    pub font_size: i64,
    pub color: String,
}

impl Default for OverlayOptions {
    fn default() -> Self {
        Self {
            position: TextPosition::Bottom,
            start: 0.0,
            end: None,
            font_size: 42,
            color: "white".to_string(),
        }
    }
}

fn do_overlay(
    runtime: &Runtime,
    path: &str,
    text: &str,
    options: &OverlayOptions,
) -> ToolResult<AddTextOverlayResult> {
    let source = runtime.workspace.existing(path)?;
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_TEXT {
        return Err(ToolError::with_hint(
            format!("text deve ter entre 1 e {MAX_TEXT} caracteres."),
            ErrorCode::InvalidArgument,
            "Quebre textos longos em linhas com \\n ou use burn_subtitles.",
        ));
    }
    if options.font_size <= 0 {
        return Err(ToolError::new(
            "font_size deve ser maior que zero.",
            ErrorCode::InvalidArgument,
        ));
    }
    let (start, end) = (options.start, options.end);
    if start < 0.0 || end.is_some_and(|e| e <= start) {
        return Err(ToolError::with_hint(
            format!(
                "Intervalo inválido: start={start}, end={}.",
                end.map_or("None".to_string(), |e| e.to_string())
            ),
            ErrorCode::InvalidArgument,
            "start deve ser >= 0 e end maior que start, ou omitido para ir até o fim.",
        ));
    }
    let info = runtime.ffmpeg.probe(&source)?;
    if start >= info.duration {
        return Err(ToolError::with_hint(
            format!(
                "start={start}s ultrapassa a duração do vídeo ({:.2}s).",
                info.duration
            ),
            ErrorCode::InvalidArgument,
            "Use probe_video para conferir a duração.",
        ));
    }
    let font = require_font()?;
    let enable = match end {
        Some(end) => format!("between(t\\,{start}\\,{end})"),
        None => format!("gte(t\\,{start})"),
    };
    let drawtext = format!(
        "drawtext=fontfile='{}':text={}:fontsize={}:fontcolor={}:borderw=3:bordercolor=black@0.8\
         :x=(w-text_w)/2:y={}:line_spacing=8:enable='{enable}'",
        escape_filter_path(&font),
        escape_drawtext(text),
        options.font_size,
        options.color,
        options.position.y_expr()
    );
    let output = runtime.workspace.output_for(&source, "text", None);
    let mut args = ffargs!["-i", source, "-vf", drawtext, "-c:v", "libx264", "-pix_fmt", "yuv420p"];
    if info.has_audio {
        args.extend(ffargs!["-c:a", "copy"]);
    }
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(AddTextOverlayResult {
        output: runtime.workspace.relative(&output),
        text: text.to_string(),
        position: options.position,
        start,
        end,
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Texto vazio ou longo demais, intervalo inválido, arquivo inexistente ou falha do ffmpeg.
pub fn add_text_overlay(
    runtime: &Arc<Runtime>,
    path: &str,
    text: &str,
    options: OverlayOptions,
    background: bool,
) -> ToolResult<MaybeJob<AddTextOverlayResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        let text = text.to_string();
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("add_text_overlay", move || {
                do_overlay(&runtime_job, &path, &text, &options)
            }),
        ));
    }
    Ok(MaybeJob::Done(do_overlay(runtime, path, text, &options)?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// Texto a exibir. Até 500 caracteres.
    pub text: String,
    /// top, center ou bottom.
    #[serde(default = "default_position")]
    pub position: TextPosition,
    /// Segundo em que o texto aparece.
    #[serde(default)]
    pub start: f64,
    /// Segundo em que o texto some. Omitido = até o fim.
    #[serde(default)]
    pub end: Option<f64>,
    /// Tamanho da fonte em pixels.
    #[serde(default = "default_font_size")]
    pub font_size: i64,
    /// Cor do texto (white, yellow, #FF0000...).
    #[serde(default = "default_color")]
    pub color: String,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_position() -> TextPosition {
    TextPosition::Bottom
}

fn default_font_size() -> i64 {
    42
}

fn default_color() -> String {
    "white".to_string()
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "add_text_overlay",
        "Escreve um texto fixo sobre o vídeo (título, descrição, chamada) e salva novo arquivo.\n\n\
         Serve para colocar a descrição ou o título que você escreveu direto na imagem. \
         O texto fica centralizado horizontalmente, com contorno preto para leitura. \
         Use \\n para quebrar linhas. Para falas sincronizadas use burn_subtitles. \
         O original não é modificado; o vídeo é re-encodado.",
        move |params: Params| {
            guarded(add_text_overlay(
                &runtime,
                &params.path,
                &params.text,
                OverlayOptions {
                    position: params.position,
                    start: params.start,
                    end: params.end,
                    font_size: params.font_size,
                    color: params.color,
                },
                params.background,
            ))
        },
    );
}
