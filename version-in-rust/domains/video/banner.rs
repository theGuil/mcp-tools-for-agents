//! Tool `add_banner`: faixa com fundo colorido e texto no topo ou no rodapé do vídeo.

use std::sync::{Arc, OnceLock};

use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::fonts::{escape_drawtext, escape_filter_path, require_font};
use crate::core::jobs::MaybeJob;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

const MAX_TEXT: usize = 300;
const MIN_HEIGHT_RATIO: f64 = 0.03;
const MAX_HEIGHT_RATIO: f64 = 0.4;
const MAX_OFFSET_RATIO: f64 = 0.6;
const MIN_BAND_PX: i64 = 24;
const FONT_RATIO: f64 = 0.4;
const MIN_FONT_PX: i64 = 12;
const LINE_SPACING: i64 = 8;

fn color_regex() -> &'static Regex {
    static COLOR: OnceLock<Regex> = OnceLock::new();
    COLOR.get_or_init(|| Regex::new(r"^[A-Za-z0-9#@.]+$").expect("regex fixa válida"))
}

/// Posição da faixa no vídeo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BannerPosition {
    Top,
    Bottom,
}

impl BannerPosition {
    /// Nome da posição como no Python.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
}

/// Vídeo gerado com a faixa.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AddBannerResult {
    pub output: String,
    pub text: String,
    pub position: BannerPosition,
    pub color: String,
    pub band_height_px: i64,
    pub offset_px: i64,
    pub font_size: i64,
    pub start: f64,
    pub end: Option<f64>,
}

/// Opções da faixa, agrupadas para a chamada interna.
#[derive(Debug, Clone)]
struct BannerOptions {
    position: BannerPosition,
    color: String,
    text_color: String,
    height_ratio: f64,
    offset_ratio: f64,
    font_size: Option<i64>,
    start: f64,
    end: Option<f64>,
}

/// Valida uma cor aceita pelo ffmpeg (nome, hexadecimal ou com opacidade).
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] se a cor tiver caracteres inesperados.
pub fn validate_color(name: &str, value: &str) -> ToolResult<String> {
    let value = value.trim();
    if value.is_empty() || !color_regex().is_match(value) {
        return Err(ToolError::with_hint(
            format!("{name}='{value}' não é uma cor válida."),
            ErrorCode::InvalidArgument,
            "Use um nome (blue, white), hexadecimal (#1E40AF) ou com opacidade (blue@0.8).",
        ));
    }
    Ok(value.to_string())
}

fn validate(text: &str, options: &BannerOptions) -> ToolResult<String> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_TEXT {
        return Err(ToolError::with_hint(
            format!("text deve ter entre 1 e {MAX_TEXT} caracteres."),
            ErrorCode::InvalidArgument,
            "Encurte o texto ou quebre em linhas com \\n.",
        ));
    }
    if !(MIN_HEIGHT_RATIO..=MAX_HEIGHT_RATIO).contains(&options.height_ratio) {
        return Err(ToolError::with_hint(
            format!("height_ratio deve estar entre {MIN_HEIGHT_RATIO} e {MAX_HEIGHT_RATIO}."),
            ErrorCode::InvalidArgument,
            "Use algo como 0.06 para uma faixa fina ou 0.12 para uma faixa grossa.",
        ));
    }
    if !(0.0..=MAX_OFFSET_RATIO).contains(&options.offset_ratio) {
        return Err(ToolError::with_hint(
            format!("offset_ratio deve estar entre 0 e {MAX_OFFSET_RATIO}."),
            ErrorCode::InvalidArgument,
            "Use 0 para colar na borda ou 0.15 para escapar da interface do app.",
        ));
    }
    if options.font_size.is_some_and(|size| size <= 0) {
        return Err(ToolError::new(
            "font_size deve ser maior que zero.",
            ErrorCode::InvalidArgument,
        ));
    }
    if options.start < 0.0 || options.end.is_some_and(|end| end <= options.start) {
        return Err(ToolError::with_hint(
            format!(
                "Intervalo inválido: start={}, end={}.",
                options.start,
                options.end.map_or("None".to_string(), |e| e.to_string())
            ),
            ErrorCode::InvalidArgument,
            "start deve ser >= 0 e end maior que start, ou omitido para ir até o fim.",
        ));
    }
    Ok(text.to_string())
}

/// Expressão `enable` do filtro para o intervalo pedido (vazia quando é o vídeo todo).
pub fn enable(start: f64, end: Option<f64>) -> String {
    if start == 0.0 && end.is_none() {
        return String::new();
    }
    let expr = match end {
        Some(end) => format!("between(t\\,{start}\\,{end})"),
        None => format!("gte(t\\,{start})"),
    };
    format!(":enable='{expr}'")
}

fn do_banner(
    runtime: &Runtime,
    path: &str,
    text: &str,
    options: &BannerOptions,
) -> ToolResult<AddBannerResult> {
    let source = runtime.workspace.existing(path)?;
    let text = validate(text, options)?;
    let color = validate_color("color", &options.color)?;
    let text_color = validate_color("text_color", &options.text_color)?;
    let info = runtime.ffmpeg.probe(&source)?;
    let Some(height) = info.height.filter(|_| info.has_video) else {
        return Err(ToolError::with_hint(
            format!("'{path}' não tem trilha de vídeo."),
            ErrorCode::InvalidArgument,
            "A faixa só se aplica a vídeos.",
        ));
    };
    let start = options.start;
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
    let band = ((height as f64 * options.height_ratio) as i64).max(MIN_BAND_PX);
    let offset = (height as f64 * options.offset_ratio) as i64;
    let size = options
        .font_size
        .unwrap_or_else(|| ((band as f64 * FONT_RATIO) as i64).max(MIN_FONT_PX));
    let enable = enable(start, options.end);
    let (box_y, band_top) = match options.position {
        BannerPosition::Top => (format!("{offset}"), format!("{offset}")),
        BannerPosition::Bottom => (format!("ih-{band}-{offset}"), format!("h-{band}-{offset}")),
    };
    let mut filters = vec![format!(
        "drawbox=x=0:y={box_y}:w=iw:h={band}:color={color}:t=fill{enable}"
    )];
    // Uma chamada de drawtext por linha: o drawtext centraliza o bloco inteiro,
    // mas alinha as linhas pela esquerda. Assim cada linha fica centralizada.
    let lines: Vec<&str> = text
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let count = lines.len() as i64;
    let block = count * size + (count - 1) * LINE_SPACING;
    let font = escape_filter_path(&require_font()?);
    for (index, line) in lines.iter().enumerate() {
        let y = format!(
            "{band_top}+({band}-{block})/2+{}+({size}-text_h)/2",
            index as i64 * (size + LINE_SPACING)
        );
        filters.push(format!(
            "drawtext=fontfile='{font}':text={}:fontsize={size}:fontcolor={text_color}\
             :x=(w-text_w)/2:y={y}{enable}",
            escape_drawtext(line)
        ));
    }
    let output = runtime.workspace.output_for(&source, "banner", None);
    let mut args = ffargs![
        "-i",
        source,
        "-vf",
        filters.join(","),
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p"
    ];
    if info.has_audio {
        args.extend(ffargs!["-c:a", "copy"]);
    }
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(AddBannerResult {
        output: runtime.workspace.relative(&output),
        text,
        position: options.position,
        color,
        band_height_px: band,
        offset_px: offset,
        font_size: size,
        start,
        end: options.end,
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Argumento inválido, arquivo inexistente ou falha do ffmpeg.
#[allow(clippy::too_many_arguments)] // os argumentos são a interface da tool
pub fn add_banner(
    runtime: &Arc<Runtime>,
    path: &str,
    text: &str,
    position: BannerPosition,
    color: &str,
    text_color: &str,
    height_ratio: f64,
    offset_ratio: f64,
    font_size: Option<i64>,
    start: f64,
    end: Option<f64>,
    background: bool,
) -> ToolResult<MaybeJob<AddBannerResult>> {
    let options = BannerOptions {
        position,
        color: color.to_string(),
        text_color: text_color.to_string(),
        height_ratio,
        offset_ratio,
        font_size,
        start,
        end,
    };
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        let text = text.to_string();
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("add_banner", move || {
                do_banner(&runtime_job, &path, &text, &options)
            }),
        ));
    }
    Ok(MaybeJob::Done(do_banner(runtime, path, text, &options)?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// Texto da faixa. Até 300 caracteres; use \n para quebrar linhas.
    pub text: String,
    /// top ou bottom.
    #[serde(default = "default_position")]
    pub position: BannerPosition,
    /// Cor de fundo: nome (blue), hexadecimal (#1E40AF) ou com opacidade (blue@0.8).
    #[serde(default = "default_color")]
    pub color: String,
    /// Cor do texto, no mesmo formato.
    #[serde(default = "default_text_color")]
    pub text_color: String,
    /// Altura da faixa como fração da altura do vídeo (0.03 a 0.4).
    #[serde(default = "default_height_ratio")]
    pub height_ratio: f64,
    /// Distância da borda até a faixa, como fração da altura (0 a 0.6).
    /// Use 0.15 no topo para ficar abaixo da interface do TikTok.
    #[serde(default)]
    pub offset_ratio: f64,
    /// Tamanho da fonte em pixels. Omitido = 40% da altura da faixa.
    #[serde(default)]
    pub font_size: Option<i64>,
    /// Segundo em que a faixa aparece.
    #[serde(default)]
    pub start: f64,
    /// Segundo em que a faixa some. Omitido = até o fim.
    #[serde(default)]
    pub end: Option<f64>,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_position() -> BannerPosition {
    BannerPosition::Top
}

fn default_color() -> String {
    "blue".to_string()
}

fn default_text_color() -> String {
    "white".to_string()
}

fn default_height_ratio() -> f64 {
    0.07
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "add_banner",
        "Desenha uma faixa de fundo colorido com texto centralizado no topo ou no rodapé.\n\n\
         Use para avisos fixos, chamadas de página ou créditos que precisam de \
         fundo sólido para leitura, como \"Só artistas sem auto-tune\". A faixa \
         ocupa toda a largura, com altura proporcional ao vídeo (height_ratio). \
         Em vídeos verticais para TikTok, Reels e Shorts, a interface do app cobre \
         cerca de 12% do topo e 25% do rodapé: use offset_ratio para afastar a \
         faixa da borda e mantê-la visível. \
         Para texto sem fundo use add_text_overlay; para falas use burn_subtitles. \
         O original não é modificado; o vídeo é re-encodado e ganha o sufixo _banner.",
        move |params: Params| {
            guarded(add_banner(
                &runtime,
                &params.path,
                &params.text,
                params.position,
                &params.color,
                &params.text_color,
                params.height_ratio,
                params.offset_ratio,
                params.font_size,
                params.start,
                params.end,
                params.background,
            ))
        },
    );
}
