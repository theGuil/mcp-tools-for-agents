//! Tool `create_thumbnail`: capa do vídeo com título em destaque.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::fonts::{escape_drawtext, escape_filter_path, require_font};
use crate::core::numbers::round_to;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

/// Formato da capa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ThumbnailFormat {
    Youtube,
    Vertical,
    Square,
    Original,
}

impl ThumbnailFormat {
    /// Nome do formato como aparece no nome do arquivo.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Youtube => "youtube",
            Self::Vertical => "vertical",
            Self::Square => "square",
            Self::Original => "original",
        }
    }

    /// Tamanho fixo do formato, ou `None` para manter o original.
    fn size(self) -> Option<(i64, i64)> {
        match self {
            Self::Youtube => Some((1280, 720)),
            Self::Vertical => Some((1080, 1920)),
            Self::Square => Some((1080, 1080)),
            Self::Original => None,
        }
    }
}

/// Posição vertical do título.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TextPosition {
    Top,
    Center,
    Bottom,
}

const MAX_TITLE: usize = 80;
const IMAGE_SUFFIXES: &[&str] = &["png", "jpg", "jpeg", "webp"];
const HEX_LEN: usize = 6;

/// Imagem gerada.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CreateThumbnailResult {
    pub output: String,
    pub width: i64,
    pub height: i64,
    pub time: Option<f64>,
    pub title: Option<String>,
    pub size_bytes: u64,
}

fn validate_color(name: &str, value: &str) -> ToolResult<String> {
    let raw = value.strip_prefix('#').unwrap_or(value);
    if raw.len() != HEX_LEN || !raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ToolError::new(
            format!("{name} deve ser uma cor hexadecimal como #FFFFFF ou #FFD700."),
            ErrorCode::InvalidArgument,
        ));
    }
    Ok(format!("0x{raw}"))
}

fn text_y(position: TextPosition) -> &'static str {
    match position {
        TextPosition::Top => "h*0.08",
        TextPosition::Center => "(h-text_h)/2",
        TextPosition::Bottom => "h-text_h-h*0.08",
    }
}

/// Quebra o título em linhas para não estourar a largura da imagem.
fn wrap_title(title: &str, max_chars: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for word in title.split_whitespace() {
        let mut candidate = current.clone();
        candidate.push(word);
        let candidate = candidate.join(" ");
        if !current.is_empty() && candidate.chars().count() > max_chars {
            lines.push(current.join(" "));
            current = vec![word];
        } else {
            current.push(word);
        }
    }
    if !current.is_empty() {
        lines.push(current.join(" "));
    }
    lines.join("\n")
}

fn title_filter(
    title: &str,
    height: i64,
    position: TextPosition,
    text_color: &str,
    border_color: &str,
    darken: bool,
) -> ToolResult<String> {
    let font = require_font()?;
    let font_size = ((height as f64 * 0.09) as i64).max(28);
    let mut steps: Vec<String> = Vec::new();
    if darken {
        steps.push("eq=brightness=-0.12:contrast=1.1".to_string());
    }
    let y = text_y(position);
    steps.push(format!(
        "drawtext=fontfile='{}':text={}\
         :fontsize={font_size}:fontcolor={text_color}:borderw={}\
         :bordercolor={border_color}:shadowcolor=black@0.6:shadowx=4:shadowy=4\
         :x=(w-text_w)/2:y={y}:line_spacing={}",
        escape_filter_path(&font),
        escape_drawtext(title),
        (font_size / 12).max(3),
        font_size / 6,
    ));
    Ok(steps.join(","))
}

fn canvas_filter(width: i64, height: i64) -> String {
    format!("scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height}")
}

/// Opções da tool além de `path`, com os mesmos defaults do Python.
#[derive(Debug, Clone)]
pub struct ThumbnailOptions {
    pub time: Option<f64>,
    pub title: Option<String>,
    pub thumbnail_format: ThumbnailFormat,
    pub position: TextPosition,
    pub text_color: String,
    pub border_color: String,
    pub darken: bool,
    pub output: Option<String>,
}

impl Default for ThumbnailOptions {
    fn default() -> Self {
        Self {
            time: None,
            title: None,
            thumbnail_format: ThumbnailFormat::Youtube,
            position: TextPosition::Center,
            text_color: default_text_color(),
            border_color: default_border_color(),
            darken: true,
            output: None,
        }
    }
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Argumento inválido, arquivo inexistente ou falha do ffmpeg.
pub fn create_thumbnail(
    runtime: &Runtime,
    path: &str,
    options: &ThumbnailOptions,
) -> ToolResult<CreateThumbnailResult> {
    let source = runtime.workspace.existing(path)?;
    let title = options
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    if title
        .as_ref()
        .is_some_and(|t| t.chars().count() > MAX_TITLE)
    {
        return Err(ToolError::with_hint(
            format!("title deve ter até {MAX_TITLE} caracteres."),
            ErrorCode::InvalidArgument,
            "Thumbnail boa tem no máximo 4 ou 5 palavras grandes.",
        ));
    }
    let fg = validate_color("text_color", &options.text_color)?;
    let border = validate_color("border_color", &options.border_color)?;
    let is_image = source
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|ext| IMAGE_SUFFIXES.contains(&ext.as_str()));
    let info = runtime.ffmpeg.probe(&source)?;
    let (Some(src_width), Some(src_height)) = (info.width, info.height) else {
        return Err(no_image(path));
    };
    if !info.has_video {
        return Err(no_image(path));
    }
    let time = if is_image {
        None
    } else {
        let time = options.time.unwrap_or(info.duration / 2.0);
        if time < 0.0 || time > info.duration {
            return Err(ToolError::with_hint(
                format!(
                    "time={time}s está fora da duração do vídeo ({:.2}s).",
                    info.duration
                ),
                ErrorCode::InvalidArgument,
                "Use probe_video para conferir a duração ou omita time para o meio.",
            ));
        }
        Some(time)
    };
    let (width, height, mut steps) = match options.thumbnail_format.size() {
        None => (src_width, src_height, Vec::new()),
        Some((w, h)) => (w, h, vec![canvas_filter(w, h)]),
    };
    if let Some(title) = &title {
        let wrapped = wrap_title(title, if width < height { 14 } else { 22 });
        steps.push(title_filter(
            &wrapped,
            height,
            options.position,
            &fg,
            &border,
            options.darken,
        )?);
    }
    let target = match &options.output {
        Some(output) => runtime.workspace.resolve(output)?,
        None => runtime.workspace.output_for(
            &source,
            &format!("thumb_{}", options.thumbnail_format.as_str()),
            Some("jpg"),
        ),
    };
    let ext = target
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !matches!(ext.as_str(), "jpg" | "jpeg" | "png") {
        return Err(ToolError::new(
            format!(
                "output '{}' deve terminar em .jpg ou .png.",
                options.output.as_deref().unwrap_or_default()
            ),
            ErrorCode::InvalidArgument,
        ));
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            ToolError::new(
                format!("Não foi possível criar a pasta de saída: {error}"),
                ErrorCode::InvalidArgument,
            )
        })?;
    }
    let mut args: Vec<std::ffi::OsString> = Vec::new();
    if let Some(time) = time {
        args.extend(ffargs!["-ss", time.to_string()]);
    }
    args.extend(ffargs!["-i", source, "-frames:v", "1"]);
    if !steps.is_empty() {
        args.extend(ffargs!["-vf", steps.join(",")]);
    }
    args.extend(ffargs!["-q:v", "2"]);
    args.push(target.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    let size_bytes = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
    Ok(CreateThumbnailResult {
        output: runtime.workspace.relative(&target),
        width,
        height,
        time: time.map(|t| round_to(t, 3)),
        title,
        size_bytes,
    })
}

fn no_image(path: &str) -> ToolError {
    ToolError::with_hint(
        format!("'{path}' não tem imagem."),
        ErrorCode::InvalidArgument,
        "Informe um vídeo ou uma imagem (png, jpg, webp).",
    )
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo ou imagem, relativo ao workspace.
    pub path: String,
    /// Segundo do frame a usar. Omita para o meio do vídeo. Ignorado para imagem.
    #[serde(default)]
    pub time: Option<f64>,
    /// Texto da capa, até 80 caracteres. Poucas palavras funcionam melhor.
    #[serde(default)]
    pub title: Option<String>,
    /// youtube (1280x720), vertical (1080x1920), square (1080x1080)
    /// ou original (mantém o tamanho do vídeo).
    #[serde(default = "default_format")]
    pub thumbnail_format: ThumbnailFormat,
    /// Onde o título fica: top, center ou bottom.
    #[serde(default = "default_position")]
    pub position: TextPosition,
    /// Cor do texto em hexadecimal, ex: #FFFFFF ou #FFD700.
    #[serde(default = "default_text_color")]
    pub text_color: String,
    /// Cor do contorno do texto em hexadecimal.
    #[serde(default = "default_border_color")]
    pub border_color: String,
    /// Escurece a imagem de fundo para o título se destacar.
    #[serde(default = "default_true")]
    pub darken: bool,
    /// Caminho do .jpg/.png a criar. Gerado ao lado do vídeo se omitido.
    #[serde(default)]
    pub output: Option<String>,
}

fn default_format() -> ThumbnailFormat {
    ThumbnailFormat::Youtube
}

fn default_position() -> TextPosition {
    TextPosition::Center
}

fn default_text_color() -> String {
    "#FFFFFF".to_string()
}

fn default_border_color() -> String {
    "#000000".to_string()
}

fn default_true() -> bool {
    true
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "create_thumbnail",
        "Cria a capa (thumbnail) do vídeo: um frame ampliado com título grande por cima.\n\n\
         Use no final da edição para gerar a imagem de capa do YouTube (1280x720), \
         do TikTok/Shorts (1080x1920 vertical) ou do feed (quadrado). Escolha um \
         frame expressivo com extract_frame ou detect_scenes antes, ou omita time \
         para usar o meio do vídeo. O título é quebrado em linhas automaticamente, \
         com contorno e sombra para ler bem em qualquer fundo; darken escurece a \
         imagem levemente para o texto saltar. Também aceita uma imagem (png, jpg) \
         como base em vez de vídeo. Devolve um .jpg.",
        move |params: Params| {
            let options = ThumbnailOptions {
                time: params.time,
                title: params.title,
                thumbnail_format: params.thumbnail_format,
                position: params.position,
                text_color: params.text_color,
                border_color: params.border_color,
                darken: params.darken,
                output: params.output,
            };
            guarded(create_thumbnail(&runtime, &params.path, &options))
        },
    );
}
