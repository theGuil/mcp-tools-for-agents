//! Tool `burn_subtitles`: grava legendas de um .srt ou .ass na imagem do vídeo.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::fonts::{escape_filter_path, find_font};
use crate::core::jobs::MaybeJob;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

const HEX_LEN: usize = 6;

/// Posição vertical da legenda.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SubtitlePosition {
    Top,
    Center,
    Bottom,
}

impl SubtitlePosition {
    /// Nome da posição.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Center => "center",
            Self::Bottom => "bottom",
        }
    }

    fn alignment(self) -> u8 {
        match self {
            Self::Bottom => 2,
            Self::Center => 5,
            Self::Top => 8,
        }
    }
}

/// Vídeo gerado com as legendas.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BurnSubtitlesResult {
    pub output: String,
    pub subtitles: String,
    pub font_size: i64,
    pub position: SubtitlePosition,
    pub styled_by_file: bool,
}

/// Opções de estilo da legenda (só .srt/.vtt).
#[derive(Debug, Clone, PartialEq)]
pub struct BurnOptions {
    pub font_size: i64,
    pub position: SubtitlePosition,
    pub text_color: String,
    pub outline_color: String,
}

impl Default for BurnOptions {
    fn default() -> Self {
        Self {
            font_size: 24,
            position: SubtitlePosition::Bottom,
            text_color: "#FFFFFF".to_string(),
            outline_color: "#000000".to_string(),
        }
    }
}

fn ass_color(hex_color: &str, name: &str) -> ToolResult<String> {
    let raw = hex_color.strip_prefix('#').unwrap_or(hex_color);
    if raw.len() != HEX_LEN || !raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ToolError::new(
            format!("{name} deve ser uma cor hexadecimal como #FFFFFF."),
            ErrorCode::InvalidArgument,
        ));
    }
    Ok(format!("&H00{}{}{}", &raw[4..6], &raw[2..4], &raw[0..2]).to_uppercase())
}

fn do_burn(
    runtime: &Runtime,
    path: &str,
    subtitles_path: &str,
    options: &BurnOptions,
) -> ToolResult<BurnSubtitlesResult> {
    let source = runtime.workspace.existing(path)?;
    let subs = runtime.workspace.existing(subtitles_path)?;
    let suffix = subs
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !matches!(suffix.as_str(), "srt" | "vtt" | "ass") {
        return Err(ToolError::with_hint(
            format!("'{subtitles_path}' não é um arquivo de legenda (.srt, .vtt ou .ass)."),
            ErrorCode::InvalidArgument,
            "Gere um .srt com create_subtitles ou um .ass com create_dynamic_subtitles.",
        ));
    }
    if options.font_size <= 0 {
        return Err(ToolError::new(
            "font_size deve ser maior que zero.",
            ErrorCode::InvalidArgument,
        ));
    }
    let primary = ass_color(&options.text_color, "text_color")?;
    let outline = ass_color(&options.outline_color, "outline_color")?;
    let info = runtime.ffmpeg.probe(&source)?;
    let styled_by_file = suffix == "ass";
    let mut filter_expr = format!("subtitles='{}'", escape_filter_path(&subs));
    if !styled_by_file {
        let style = format!(
            "FontSize={},PrimaryColour={primary},OutlineColour={outline},\
             Outline=2,Shadow=0,Alignment={},MarginV=30",
            options.font_size,
            options.position.alignment()
        );
        filter_expr.push_str(&format!(":force_style='{style}'"));
    }
    if let Some(font) = find_font() {
        if let Some(dir) = font.parent() {
            filter_expr.push_str(&format!(":fontsdir='{}'", escape_filter_path(dir)));
        }
    }
    let output = runtime.workspace.output_for(&source, "subtitled", None);
    let mut args = ffargs![
        "-i",
        source,
        "-vf",
        filter_expr,
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
    Ok(BurnSubtitlesResult {
        output: runtime.workspace.relative(&output),
        subtitles: runtime.workspace.relative(&subs),
        font_size: options.font_size,
        position: options.position,
        styled_by_file,
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Legenda com extensão errada, cor inválida, arquivo inexistente ou falha do ffmpeg.
pub fn burn_subtitles(
    runtime: &Arc<Runtime>,
    path: &str,
    subtitles_path: &str,
    options: BurnOptions,
    background: bool,
) -> ToolResult<MaybeJob<BurnSubtitlesResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        let subtitles_path = subtitles_path.to_string();
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("burn_subtitles", move || {
                do_burn(&runtime_job, &path, &subtitles_path, &options)
            }),
        ));
    }
    Ok(MaybeJob::Done(do_burn(
        runtime,
        path,
        subtitles_path,
        &options,
    )?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// Arquivo .srt, .vtt ou .ass, relativo ao workspace.
    pub subtitles_path: String,
    /// Tamanho da fonte (só .srt/.vtt).
    #[serde(default = "default_font_size")]
    pub font_size: i64,
    /// top, center ou bottom (só .srt/.vtt).
    #[serde(default = "default_position")]
    pub position: SubtitlePosition,
    /// Cor do texto em hexadecimal, ex: #FFFFFF (só .srt/.vtt).
    #[serde(default = "default_text_color")]
    pub text_color: String,
    /// Cor do contorno em hexadecimal, ex: #000000 (só .srt/.vtt).
    #[serde(default = "default_outline_color")]
    pub outline_color: String,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_font_size() -> i64 {
    24
}

fn default_position() -> SubtitlePosition {
    SubtitlePosition::Bottom
}

fn default_text_color() -> String {
    "#FFFFFF".to_string()
}

fn default_outline_color() -> String {
    "#000000".to_string()
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "burn_subtitles",
        "Grava as legendas de um .srt ou .ass direto na imagem do vídeo (legenda fixa).\n\n\
         Fluxos: transcribe_audio -> create_subtitles (.srt) -> burn_subtitles para \
         legenda comum; transcribe_audio -> create_dynamic_subtitles (.ass) -> \
         burn_subtitles para legenda animada palavra por palavra. Com .srt/.vtt os \
         parâmetros de estilo (font_size, position, cores) são aplicados; com .ass o \
         estilo já vem do arquivo e esses parâmetros são ignorados. O original não \
         é modificado; o vídeo é re-encodado. Para vídeos longos use background=true.",
        move |params: Params| {
            guarded(burn_subtitles(
                &runtime,
                &params.path,
                &params.subtitles_path,
                BurnOptions {
                    font_size: params.font_size,
                    position: params.position,
                    text_color: params.text_color,
                    outline_color: params.outline_color,
                },
                params.background,
            ))
        },
    );
}
