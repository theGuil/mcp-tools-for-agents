//! Tool `create_dynamic_subtitles`: legenda animada palavra por palavra (estilo TikTok).

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::numbers::round_to;
use crate::domains::{McpServer, Runtime};

/// Estilo da animação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SubtitleStyle {
    Highlight,
    Word,
    Block,
}

/// Posição vertical da legenda.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SubtitlePosition {
    Top,
    Center,
    Bottom,
}

impl SubtitlePosition {
    /// Código de alinhamento do ASS (numpad).
    fn alignment(self) -> u8 {
        match self {
            Self::Bottom => 2,
            Self::Center => 5,
            Self::Top => 8,
        }
    }
}

const MAX_WORDS: i64 = 12;
const MIN_WORD_GAP: f64 = 0.05;
const HEX_LEN: usize = 6;
const DEFAULT_WIDTH: i64 = 1080;
const DEFAULT_HEIGHT: i64 = 1920;

/// Uma palavra com seus tempos (mesmo formato de transcribe_audio).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct WordTiming {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

/// Arquivo .ass gerado.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CreateDynamicSubtitlesResult {
    pub output: String,
    pub style: SubtitleStyle,
    pub words: usize,
    pub groups: usize,
    pub duration: f64,
    pub play_res: String,
}

/// Converte `#RRGGBB` para o formato `&H00BBGGRR` do ASS.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] se a cor não for hexadecimal de seis dígitos.
pub fn ass_color(hex_color: &str, name: &str) -> ToolResult<String> {
    let raw = hex_color.strip_prefix('#').unwrap_or(hex_color);
    if raw.len() != HEX_LEN || !raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ToolError::new(
            format!("{name} deve ser uma cor hexadecimal como #FFFFFF ou #FFD700."),
            ErrorCode::InvalidArgument,
        ));
    }
    let (red, green, blue) = (&raw[0..2], &raw[2..4], &raw[4..6]);
    Ok(format!("&H00{blue}{green}{red}").to_uppercase())
}

/// Converte segundos para `H:MM:SS.cc` (centésimos) do ASS.
pub fn ass_time(seconds: f64) -> String {
    // `round` do Python arredonda para o par; a diferença só aparece em .5 exato.
    let total_cs = (seconds * 100.0).round_ties_even().max(0.0) as i64;
    let hours = total_cs / 360_000;
    let rest = total_cs % 360_000;
    let minutes = rest / 6_000;
    let rest = rest % 6_000;
    let secs = rest / 100;
    let cents = rest % 100;
    format!("{hours}:{minutes:02}:{secs:02}.{cents:02}")
}

fn escape_ass(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('{', "(")
        .replace('}', ")")
}

fn validate_words(words: &[WordTiming], uppercase: bool) -> ToolResult<Vec<WordTiming>> {
    if words.is_empty() {
        return Err(ToolError::with_hint(
            "words está vazio.",
            ErrorCode::InvalidArgument,
            "Passe a lista words de transcribe_audio (junte as words de todos os segments).",
        ));
    }
    let mut cleaned: Vec<WordTiming> = Vec::new();
    for (index, item) in words.iter().enumerate() {
        let index = index + 1;
        let text = item.word.trim();
        let (start, end) = (item.start, item.end);
        if text.is_empty() {
            continue;
        }
        if start < 0.0 || end < start {
            return Err(ToolError::with_hint(
                format!("Palavra {index} com intervalo inválido: start={start}, end={end}."),
                ErrorCode::InvalidArgument,
                "start deve ser >= 0 e end >= start, em segundos.",
            ));
        }
        if cleaned.last().is_some_and(|last| start < last.start) {
            return Err(ToolError::with_hint(
                format!("Palavra {index} ('{text}') começa antes da anterior."),
                ErrorCode::InvalidArgument,
                "Envie as palavras em ordem cronológica.",
            ));
        }
        cleaned.push(WordTiming {
            start,
            end,
            word: if uppercase {
                text.to_uppercase()
            } else {
                text.to_string()
            },
        });
    }
    if cleaned.is_empty() {
        return Err(ToolError::new(
            "Nenhuma palavra com texto em words.",
            ErrorCode::InvalidArgument,
        ));
    }
    Ok(cleaned)
}

/// Agrupa palavras em blocos curtos, quebrando em pausas longas.
fn group_words(words: &[WordTiming], max_words: usize, max_gap: f64) -> Vec<Vec<WordTiming>> {
    let mut groups: Vec<Vec<WordTiming>> = Vec::new();
    let mut current: Vec<WordTiming> = Vec::new();
    for word in words {
        if let Some(last) = current.last() {
            if current.len() >= max_words || word.start - last.end > max_gap {
                groups.push(std::mem::take(&mut current));
            }
        }
        current.push(word.clone());
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

/// Janela de exibição de cada palavra: do início dela até o início da próxima.
fn word_windows(group: &[WordTiming]) -> Vec<(f64, f64)> {
    let mut windows = Vec::with_capacity(group.len());
    for (index, word) in group.iter().enumerate() {
        let start = word.start;
        let mut end = group.get(index + 1).map_or(word.end, |next| next.start);
        if end - start < MIN_WORD_GAP {
            end = start + MIN_WORD_GAP;
        }
        windows.push((start, end));
    }
    windows
}

fn events(
    groups: &[Vec<WordTiming>],
    style: SubtitleStyle,
    highlight: &str,
    text_color: &str,
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for group in groups {
        let windows = word_windows(group);
        if style == SubtitleStyle::Block {
            let start = windows.first().map_or(0.0, |w| w.0);
            let end = group.iter().map(|w| w.end).fold(f64::MIN, f64::max);
            let text = group
                .iter()
                .map(|w| escape_ass(&w.word))
                .collect::<Vec<_>>()
                .join(" ");
            lines.push(dialogue(start, end, &format!("{{\\fad(80,80)}}{text}")));
            continue;
        }
        for (index, (start, end)) in windows.iter().enumerate() {
            let text = if style == SubtitleStyle::Word {
                format!(
                    "{{\\fscx85\\fscy85\\t(0,70,\\fscx100\\fscy100)}}{}",
                    escape_ass(&group[index].word)
                )
            } else {
                let parts: Vec<String> = group
                    .iter()
                    .enumerate()
                    .map(|(pos, word)| {
                        let escaped = escape_ass(&word.word);
                        if pos == index {
                            format!(
                                "{{\\c{highlight}&\\fscx108\\fscy108}}{escaped}\
                                 {{\\c{text_color}&\\fscx100\\fscy100}}"
                            )
                        } else {
                            escaped
                        }
                    })
                    .collect();
                parts.join(" ")
            };
            lines.push(dialogue(*start, *end, &text));
        }
    }
    lines
}

fn dialogue(start: f64, end: f64, text: &str) -> String {
    format!(
        "Dialogue: 0,{},{},Default,,0,0,0,,{text}",
        ass_time(start),
        ass_time(end)
    )
}

/// Opções de estilo do `.ass`, agrupadas para o [`build_ass`].
#[derive(Debug, Clone)]
pub struct AssOptions<'a> {
    pub style: SubtitleStyle,
    pub max_words: usize,
    pub max_gap: f64,
    pub font_size: i64,
    pub text_color: &'a str,
    pub highlight_color: &'a str,
    pub outline_color: &'a str,
    pub position: SubtitlePosition,
    pub uppercase: bool,
    pub width: i64,
    pub height: i64,
}

/// Monta o conteúdo do .ass. Devolve (conteúdo, palavras, grupos, duração).
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] se as palavras ou as cores forem inválidas.
pub fn build_ass(
    words: &[WordTiming],
    options: &AssOptions<'_>,
) -> ToolResult<(String, usize, usize, f64)> {
    let cleaned = validate_words(words, options.uppercase)?;
    let groups = group_words(&cleaned, options.max_words, options.max_gap);
    let primary = ass_color(options.text_color, "text_color")?;
    let highlight = ass_color(options.highlight_color, "highlight_color")?;
    let outline = ass_color(options.outline_color, "outline_color")?;
    let (width, height, font_size) = (options.width, options.height, options.font_size);
    let margin_v = (height as f64 * 0.18) as i64;
    let margin_h = (width as f64 * 0.06) as i64;
    let header = format!(
        "[Script Info]\n\
         ScriptType: v4.00+\n\
         PlayResX: {width}\n\
         PlayResY: {height}\n\
         WrapStyle: 0\n\
         ScaledBorderAndShadow: yes\n\n\
         [V4+ Styles]\n\
         Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, \
         BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, \
         BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Default,DejaVu Sans,{font_size},{primary},{primary},{outline},&H80000000,\
         -1,0,0,0,100,100,0,0,1,{},{},{},{margin_h},{margin_h},{margin_v},1\n\n\
         [Events]\n\
         Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        (font_size / 14).max(2),
        (font_size / 30).max(1),
        options.position.alignment(),
    );
    let lines = events(&groups, options.style, &highlight, &primary);
    let duration = cleaned.iter().map(|w| w.end).fold(f64::MIN, f64::max);
    Ok((
        format!("{header}{}\n", lines.join("\n")),
        cleaned.len(),
        groups.len(),
        duration,
    ))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Lista de {start, end, word}, em segundos, em ordem cronológica.
    pub words: Vec<WordTiming>,
    /// Caminho do .ass a criar, relativo ao workspace.
    pub output: String,
    /// Vídeo alvo, para ajustar tamanho e posição à resolução dele.
    #[serde(default)]
    pub video_path: Option<String>,
    /// highlight, word ou block.
    #[serde(default = "default_style")]
    pub style: SubtitleStyle,
    /// Máximo de palavras por bloco (3 a 5 é o usual).
    #[serde(default = "default_max_words")]
    pub max_words: i64,
    /// Pausa em segundos que força um bloco novo.
    #[serde(default = "default_max_gap")]
    pub max_gap: f64,
    /// Tamanho da fonte. Calculado pela altura do vídeo se omitido.
    #[serde(default)]
    pub font_size: Option<i64>,
    /// Cor do texto em hexadecimal, ex: #FFFFFF.
    #[serde(default = "default_text_color")]
    pub text_color: String,
    /// Cor da palavra em destaque, ex: #FFD700 (amarelo) ou #00FF88.
    #[serde(default = "default_highlight_color")]
    pub highlight_color: String,
    /// Cor do contorno, ex: #000000.
    #[serde(default = "default_outline_color")]
    pub outline_color: String,
    /// top, center ou bottom. center é o padrão em vídeo vertical.
    #[serde(default = "default_position")]
    pub position: SubtitlePosition,
    /// Converte o texto para maiúsculas, como nos cortes virais.
    #[serde(default = "default_true")]
    pub uppercase: bool,
}

fn default_style() -> SubtitleStyle {
    SubtitleStyle::Highlight
}

fn default_max_words() -> i64 {
    4
}

fn default_max_gap() -> f64 {
    1.0
}

fn default_text_color() -> String {
    "#FFFFFF".to_string()
}

fn default_highlight_color() -> String {
    "#FFD700".to_string()
}

fn default_outline_color() -> String {
    "#000000".to_string()
}

fn default_position() -> SubtitlePosition {
    SubtitlePosition::Center
}

fn default_true() -> bool {
    true
}

/// Opções da tool além de `words` e `output`, com os mesmos defaults do Python.
#[derive(Debug, Clone)]
pub struct DynamicSubtitlesOptions {
    pub video_path: Option<String>,
    pub style: SubtitleStyle,
    pub max_words: i64,
    pub max_gap: f64,
    pub font_size: Option<i64>,
    pub text_color: String,
    pub highlight_color: String,
    pub outline_color: String,
    pub position: SubtitlePosition,
    pub uppercase: bool,
}

impl Default for DynamicSubtitlesOptions {
    fn default() -> Self {
        Self {
            video_path: None,
            style: default_style(),
            max_words: default_max_words(),
            max_gap: default_max_gap(),
            font_size: None,
            text_color: default_text_color(),
            highlight_color: default_highlight_color(),
            outline_color: default_outline_color(),
            position: default_position(),
            uppercase: true,
        }
    }
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Argumento inválido, vídeo inexistente ou falha ao gravar o arquivo.
pub fn create_dynamic_subtitles(
    runtime: &Runtime,
    words: &[WordTiming],
    output: &str,
    options: &DynamicSubtitlesOptions,
) -> ToolResult<CreateDynamicSubtitlesResult> {
    let target = runtime.workspace.resolve(output)?;
    let is_ass = target
        .extension()
        .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case("ass"));
    if !is_ass {
        return Err(ToolError::with_hint(
            format!("output '{output}' deve terminar em .ass."),
            ErrorCode::InvalidArgument,
            "Ex: legendas/video.ass. Depois aplique com burn_subtitles.",
        ));
    }
    if !(1..=MAX_WORDS).contains(&options.max_words) {
        return Err(ToolError::with_hint(
            format!("max_words deve estar entre 1 e {MAX_WORDS}."),
            ErrorCode::InvalidArgument,
            "3 a 5 palavras por bloco é o padrão de TikTok e Reels.",
        ));
    }
    if options.max_gap <= 0.0 {
        return Err(ToolError::new(
            "max_gap deve ser maior que zero.",
            ErrorCode::InvalidArgument,
        ));
    }
    let (mut width, mut height) = (DEFAULT_WIDTH, DEFAULT_HEIGHT);
    if let Some(video_path) = &options.video_path {
        let info = runtime
            .ffmpeg
            .probe(&runtime.workspace.existing(video_path)?)?;
        if let (Some(w), Some(h)) = (info.width, info.height) {
            if w > 0 && h > 0 {
                width = w;
                height = h;
            }
        }
    }
    let size = options
        .font_size
        .unwrap_or_else(|| ((height as f64 * 0.045) as i64).max(20));
    if size <= 0 {
        return Err(ToolError::new(
            "font_size deve ser maior que zero.",
            ErrorCode::InvalidArgument,
        ));
    }
    let (content, count, groups, duration) = build_ass(
        words,
        &AssOptions {
            style: options.style,
            max_words: options.max_words as usize,
            max_gap: options.max_gap,
            font_size: size,
            text_color: &options.text_color,
            highlight_color: &options.highlight_color,
            outline_color: &options.outline_color,
            position: options.position,
            uppercase: options.uppercase,
            width,
            height,
        },
    )?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| io_error(output, &error))?;
    }
    std::fs::write(&target, content).map_err(|error| io_error(output, &error))?;
    Ok(CreateDynamicSubtitlesResult {
        output: runtime.workspace.relative(&target),
        style: options.style,
        words: count,
        groups,
        duration: round_to(duration, 3),
        play_res: format!("{width}x{height}"),
    })
}

fn io_error(output: &str, error: &std::io::Error) -> ToolError {
    ToolError::new(
        format!("Não foi possível gravar '{output}': {error}"),
        ErrorCode::InvalidArgument,
    )
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "create_dynamic_subtitles",
        "Cria legendas animadas palavra por palavra (estilo TikTok/Reels) em um arquivo .ass.\n\n\
         Fluxo: transcribe_audio (word_timestamps=true) -> junte as words de todos os \
         segments em uma lista -> create_dynamic_subtitles -> burn_subtitles. As \
         palavras são agrupadas em blocos curtos (max_words) que quebram nas pausas \
         (max_gap). Estilos:\n\
         - \"highlight\": o bloco fica visível e a palavra falada no momento muda de cor \
         e cresce um pouco (o mais usado em cortes de podcast).\n\
         - \"word\": aparece uma palavra de cada vez, grande, com efeito de pop.\n\
         - \"block\": o bloco inteiro aparece de uma vez, sem destaque, com fade.\n\
         Informe video_path para a legenda ser dimensionada para a resolução certa \
         (padrão 1080x1920). O tamanho da fonte é calculado pela altura do vídeo se \
         font_size for omitido. Depois grave no vídeo com burn_subtitles, que \
         respeita o estilo do .ass.",
        move |params: Params| {
            let options = DynamicSubtitlesOptions {
                video_path: params.video_path,
                style: params.style,
                max_words: params.max_words,
                max_gap: params.max_gap,
                font_size: params.font_size,
                text_color: params.text_color,
                highlight_color: params.highlight_color,
                outline_color: params.outline_color,
                position: params.position,
                uppercase: params.uppercase,
            };
            guarded(create_dynamic_subtitles(
                &runtime,
                &params.words,
                &params.output,
                &options,
            ))
        },
    );
}
