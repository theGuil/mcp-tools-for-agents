//! Tool `apply_studio_background_light`: luz ambiente colorida de estúdio no vídeo.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::core::numbers::format_g;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

const MIN_INTENSITY: f64 = 0.05;
const MAX_INTENSITY: f64 = 1.0;
const MIN_SPREAD: f64 = 0.2;
const MAX_SPREAD: f64 = 2.0;
/// Usado quando o ffprobe não devolve o fps do vídeo.
const FALLBACK_FPS: f64 = 25.0;

/// Cor da luz de fundo. Presets pensados para fundo escuro ou neutro de estúdio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LightColor {
    CyanBlue,
    NeonPurple,
    WarmOrange,
    CyberpunkGreen,
    DeepRed,
    SunsetPink,
    ClassicWhite,
}

impl LightColor {
    /// Nome da cor como o agente informa e como entra no nome do arquivo.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CyanBlue => "cyan_blue",
            Self::NeonPurple => "neon_purple",
            Self::WarmOrange => "warm_orange",
            Self::CyberpunkGreen => "cyberpunk_green",
            Self::DeepRed => "deep_red",
            Self::SunsetPink => "sunset_pink",
            Self::ClassicWhite => "classic_white",
        }
    }

    /// Componentes RGB (0 a 255) do ponto mais forte da luz.
    pub fn rgb(self) -> (i64, i64, i64) {
        match self {
            Self::CyanBlue => (20, 140, 255),
            Self::NeonPurple => (170, 40, 255),
            Self::WarmOrange => (255, 130, 30),
            Self::CyberpunkGreen => (30, 255, 120),
            Self::DeepRed => (255, 40, 60),
            Self::SunsetPink => (255, 70, 160),
            Self::ClassicWhite => (235, 235, 235),
        }
    }
}

/// De onde a luz vem e como ela se distribui no quadro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LightShape {
    Radial,
    Top,
    Bottom,
    Sides,
}

impl LightShape {
    /// Nome do formato como o agente informa e como entra no nome do arquivo.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Radial => "radial",
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Sides => "sides",
        }
    }
}

/// Como a luz se mistura com a imagem original.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LightBlend {
    Screen,
    Softlight,
    Lighten,
}

impl LightBlend {
    /// Nome do modo como o filtro `blend` do ffmpeg espera.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Screen => "screen",
            Self::Softlight => "softlight",
            Self::Lighten => "lighten",
        }
    }
}

/// Vídeo gerado com a luz de fundo.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StudioBackgroundLightResult {
    pub output: String,
    pub color: LightColor,
    /// Cor de fato aplicada, em hexadecimal. `custom_color` vence o preset.
    pub hex: String,
    pub shape: LightShape,
    pub blend: LightBlend,
    pub intensity: f64,
    pub spread: f64,
    pub width: i64,
    pub height: i64,
}

/// Opções da luz, agrupadas para a chamada interna.
#[derive(Debug, Clone)]
struct LightOptions {
    color: LightColor,
    custom_color: Option<String>,
    shape: LightShape,
    blend: LightBlend,
    intensity: f64,
    spread: f64,
}

/// Converte `#RRGGBB` (ou `RRGGBB`) nos componentes 0 a 255.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] quando não são seis dígitos hexadecimais.
pub fn parse_hex(value: &str) -> ToolResult<(i64, i64, i64)> {
    let digits = value.trim().trim_start_matches('#');
    if digits.len() != 6 || !digits.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(ToolError::with_hint(
            format!("custom_color='{value}' não é um hexadecimal de seis dígitos."),
            ErrorCode::InvalidArgument,
            "Use #RRGGBB, como #1E90FF. Ou deixe custom_color de fora e escolha um preset em color.",
        ));
    }
    let channel = |start: usize| i64::from_str_radix(&digits[start..start + 2], 16).unwrap_or(0);
    Ok((channel(0), channel(2), channel(4)))
}

/// Resolve a cor final: `custom_color` em hexadecimal vence o preset de `color`.
fn resolve_color(options: &LightOptions) -> ToolResult<(i64, i64, i64)> {
    match options
        .custom_color
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => parse_hex(value),
        None => Ok(options.color.rgb()),
    }
}

fn validate(intensity: f64, spread: f64) -> ToolResult<()> {
    if !(MIN_INTENSITY..=MAX_INTENSITY).contains(&intensity) {
        return Err(ToolError::with_hint(
            format!(
                "intensity deve estar entre {} e {}.",
                format_g(MIN_INTENSITY),
                format_g(MAX_INTENSITY)
            ),
            ErrorCode::InvalidArgument,
            "0.4 é uma luz discreta, 0.6 o padrão, 0.9 bem marcada.",
        ));
    }
    if !(MIN_SPREAD..=MAX_SPREAD).contains(&spread) {
        return Err(ToolError::with_hint(
            format!(
                "spread deve estar entre {} e {}.",
                format_g(MIN_SPREAD),
                format_g(MAX_SPREAD)
            ),
            ErrorCode::InvalidArgument,
            "0.5 concentra a luz num ponto, 0.8 é o padrão, 1.5 espalha pelo quadro inteiro.",
        ));
    }
    Ok(())
}

/// Expressão do fator da luz (1 no ponto mais forte, perto de 0 na sombra) em
/// função da posição do pixel, para o filtro `geq`.
///
/// As vírgulas saem escapadas porque a expressão entra dentro de um filtro do
/// ffmpeg, onde a vírgula separa filtros.
pub fn falloff_expression(shape: LightShape, spread: f64) -> String {
    let spread = format!("{spread:.4}");
    match shape {
        LightShape::Radial => {
            format!("exp(-pow(hypot((X-W/2)/(W/2)\\,(Y-H/2)/(H/2))/{spread}\\,2))")
        }
        LightShape::Top => format!("exp(-pow(Y/H/{spread}\\,2))"),
        LightShape::Bottom => format!("exp(-pow((H-Y)/H/{spread}\\,2))"),
        LightShape::Sides => format!("exp(-pow(min(X\\,W-X)/(W/2)/{spread}\\,2))"),
    }
}

/// Monta o `geq` que desenha a luz: a cor cheia no ponto mais forte e preto no
/// resto, para o `blend` só acender o que interessa.
pub fn glow_expression(rgb: (i64, i64, i64), shape: LightShape, spread: f64) -> String {
    let (red, green, blue) = rgb;
    let falloff = falloff_expression(shape, spread);
    format!("geq=r='{red}*{falloff}':g='{green}*{falloff}':b='{blue}*{falloff}'")
}

/// Monta o filter_complex inteiro.
///
/// A luz é desenhada uma única vez num quadro parado e repetida pelo `loop`:
/// o `geq` é caro por pixel e rodaria em cada quadro do vídeo sem isso. A
/// mistura acontece em RGB (`gbrp`) porque `screen` em plano de croma daria cor
/// errada.
fn filter_graph(
    width: i64,
    height: i64,
    fps: f64,
    glow: &str,
    blend: LightBlend,
    intensity: f64,
) -> String {
    let fps = format_g(fps);
    let mode = blend.as_str();
    format!(
        "color=c=black:s={width}x{height}:r={fps}:d=1,format=gbrp,{glow},\
         loop=loop=-1:size=1:start=0,setpts=N/FRAME_RATE/TB[glow];\
         [0:v]format=gbrp[base];\
         [base][glow]blend=all_mode={mode}:all_opacity={intensity:.4}:shortest=1,\
         format=yuv420p[vout]"
    )
}

fn no_video(path: &str) -> ToolError {
    ToolError::with_hint(
        format!("'{path}' não tem trilha de vídeo."),
        ErrorCode::InvalidArgument,
        "apply_studio_background_light só se aplica a vídeos.",
    )
}

fn do_apply(
    runtime: &Runtime,
    path: &str,
    options: &LightOptions,
) -> ToolResult<StudioBackgroundLightResult> {
    let source = runtime.workspace.existing(path)?;
    let rgb = resolve_color(options)?;
    validate(options.intensity, options.spread)?;
    let info = runtime.ffmpeg.probe(&source)?;
    let (Some(width), Some(height)) = (info.width, info.height) else {
        return Err(no_video(path));
    };
    if !info.has_video {
        return Err(no_video(path));
    }
    let fps = info
        .fps
        .filter(|value| *value > 0.0)
        .unwrap_or(FALLBACK_FPS);
    let glow = glow_expression(rgb, options.shape, options.spread);
    let filter_expr = filter_graph(width, height, fps, &glow, options.blend, options.intensity);
    let tag = if options.custom_color.is_some() {
        "light_custom".to_string()
    } else {
        format!("light_{}", options.color.as_str())
    };
    let output = runtime.workspace.output_for(&source, &tag, None);
    let mut args = ffargs![
        "-i",
        source,
        "-filter_complex",
        filter_expr,
        "-map",
        "[vout]"
    ];
    if info.has_audio {
        args.extend(ffargs!["-map", "0:a?", "-c:a", "copy"]);
    } else {
        args.extend(ffargs!["-an"]);
    }
    args.extend(ffargs!["-c:v", "libx264", "-preset", "fast"]);
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    let (red, green, blue) = rgb;
    Ok(StudioBackgroundLightResult {
        output: runtime.workspace.relative(&output),
        color: options.color,
        hex: format!("#{red:02X}{green:02X}{blue:02X}"),
        shape: options.shape,
        blend: options.blend,
        intensity: options.intensity,
        spread: options.spread,
        width,
        height,
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Argumento inválido, arquivo inexistente ou falha do ffmpeg.
#[allow(clippy::too_many_arguments)] // os argumentos são a interface da tool
pub fn apply_studio_background_light(
    runtime: &Arc<Runtime>,
    path: &str,
    color: LightColor,
    custom_color: Option<String>,
    shape: LightShape,
    blend: LightBlend,
    intensity: f64,
    spread: f64,
    background: bool,
) -> ToolResult<MaybeJob<StudioBackgroundLightResult>> {
    let options = LightOptions {
        color,
        custom_color,
        shape,
        blend,
        intensity,
        spread,
    };
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        return Ok(MaybeJob::Job(
            runtime
                .jobs
                .submit("apply_studio_background_light", move || {
                    do_apply(&runtime_job, &path, &options)
                }),
        ));
    }
    Ok(MaybeJob::Done(do_apply(runtime, path, &options)?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// Cor da luz: "cyan_blue", "neon_purple", "warm_orange", "cyberpunk_green",
    /// "deep_red", "sunset_pink" ou "classic_white".
    #[serde(default = "default_color")]
    pub color: LightColor,
    /// Cor própria em hexadecimal (#RRGGBB); quando vem, ignora o preset de color.
    #[serde(default)]
    pub custom_color: Option<String>,
    /// De onde vem a luz: "radial" (halo atrás da pessoa), "top", "bottom" ou "sides".
    #[serde(default = "default_shape")]
    pub shape: LightShape,
    /// Mistura com a imagem: "screen", "softlight" ou "lighten".
    #[serde(default = "default_blend")]
    pub blend: LightBlend,
    /// Força da luz, de 0.05 a 1. 0.6 é o padrão.
    #[serde(default = "default_intensity")]
    pub intensity: f64,
    /// Quanto a luz se espalha, de 0.2 a 2. 0.8 é o padrão.
    #[serde(default = "default_spread")]
    pub spread: f64,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_color() -> LightColor {
    LightColor::CyanBlue
}

fn default_shape() -> LightShape {
    LightShape::Radial
}

fn default_blend() -> LightBlend {
    LightBlend::Screen
}

fn default_intensity() -> f64 {
    0.6
}

fn default_spread() -> f64 {
    0.8
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "apply_studio_background_light",
        "Acende uma luz colorida de estúdio no fundo do vídeo.\n\n\
         Serve para dar clima de set de gravação a quem gravou num fundo escuro, \
         cinza ou numa parede lisa: um halo colorido aparece atrás da pessoa, como \
         um softbox ou uma fita de LED apontada para o fundo. Funciona melhor \
         quando o fundo é escuro ou neutro e a pessoa está bem iluminada, porque o \
         modo \"screen\" acende o que está escuro e quase não mexe no que já está \
         claro. Fundo branco ou estourado quase não muda.\n\
         Formatos (shape):\n\
         - \"radial\": halo no centro, atrás de quem fala. É o visual mais comum.\n\
         - \"top\": luz descendo do topo, como um refletor no teto.\n\
         - \"bottom\": brilho subindo do rodapé.\n\
         - \"sides\": duas luzes nas laterais, estilo cyberpunk/gamer.\n\
         intensity (0.05 a 1) é a força e spread (0.2 a 2) é o tamanho da mancha \
         de luz: 0.5 concentra num ponto, 1.5 banha o quadro. Para uma cor de \
         marca, passe custom_color=\"#RRGGBB\" no lugar do preset.\n\
         Resolução, duração e fps não mudam, o áudio é copiado sem recompressão e \
         o arquivo original não é tocado: a saída é um arquivo novo. Vídeo longo \
         em alta resolução leva tempo, use background=true. Devolve o caminho da \
         saída e a cor aplicada em hexadecimal.",
        move |params: Params| {
            guarded(apply_studio_background_light(
                &runtime,
                &params.path,
                params.color,
                params.custom_color,
                params.shape,
                params.blend,
                params.intensity,
                params.spread,
                params.background,
            ))
        },
    );
}
