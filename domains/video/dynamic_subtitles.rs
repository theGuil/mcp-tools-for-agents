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
    /// O bloco fica visível e a palavra falada muda de cor e cresce.
    Highlight,
    /// Uma palavra de cada vez, grande, com pop.
    Word,
    /// O bloco inteiro aparece de uma vez, com fade.
    Block,
    /// Karaokê: a cor preenche cada palavra da esquerda para a direita enquanto é dita.
    Karaoke,
    /// A palavra falada ganha uma tarja colorida atrás (estilo CapCut).
    Box,
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

/// Visual pronto, no padrão dos cortes que mais circulam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SubtitlePreset {
    /// Maiúsculas grandes, 3 palavras, destaque amarelo, contorno grosso e
    /// sombra (cortes de podcast e "dinheiro/negócios").
    Hormozi,
    /// Palavra falada com tarja colorida atrás, 4 palavras, centro (CapCut).
    Boxed,
    /// Preenchimento da esquerda para a direita, 5 palavras, rodapé (clipes musicais e reels).
    Karaoke,
    /// Uma palavra gigante por vez, com pop (ganchos e frases de impacto).
    Pop,
    /// Bloco discreto no rodapé, sem maiúsculas, contorno fino (documental e corporativo).
    Clean,
    /// Destaque verde-neon com contorno colorido e brilho (gaming e tech).
    Neon,
}

const MAX_WORDS: i64 = 12;
const MIN_WORD_GAP: f64 = 0.05;
const HEX_LEN: usize = 6;
const DEFAULT_WIDTH: i64 = 1080;
const DEFAULT_HEIGHT: i64 = 1920;
const DEFAULT_FONT: &str = "DejaVu Sans";

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
    pub preset: Option<SubtitlePreset>,
    pub words: usize,
    pub groups: usize,
    pub duration: f64,
    pub play_res: String,
    pub font_size: i64,
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

/// Cores e medidas já convertidas para o ASS, usadas ao montar os eventos.
struct Palette {
    text: String,
    highlight: String,
    outline: String,
    highlight_scale: i64,
    box_border: f64,
}

fn events(groups: &[Vec<WordTiming>], style: SubtitleStyle, palette: &Palette) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let Palette {
        text: text_color,
        highlight,
        outline: outline_color,
        highlight_scale: scale,
        box_border,
    } = palette;
    for group in groups {
        let windows = word_windows(group);
        let group_start = windows.first().map_or(0.0, |w| w.0);
        let group_end = group.iter().map(|w| w.end).fold(f64::MIN, f64::max);
        match style {
            SubtitleStyle::Block => {
                let text = group
                    .iter()
                    .map(|w| escape_ass(&w.word))
                    .collect::<Vec<_>>()
                    .join(" ");
                lines.push(dialogue(
                    group_start,
                    group_end,
                    &format!("{{\\fad(80,80)}}{text}"),
                ));
            }
            SubtitleStyle::Karaoke => {
                // \kf preenche da SecondaryColour para a PrimaryColour ao longo da
                // duração; aqui a primária vira a cor de destaque e a secundária o texto.
                let mut text = format!("{{\\1c{highlight}&\\2c{text_color}&}}");
                for (index, (start, end)) in windows.iter().enumerate() {
                    let centis = ((end - start) * 100.0).round().max(1.0) as i64;
                    if index > 0 {
                        text.push(' ');
                    }
                    text.push_str(&format!(
                        "{{\\kf{centis}}}{}",
                        escape_ass(&group[index].word)
                    ));
                }
                lines.push(dialogue(group_start, group_end, &text));
            }
            SubtitleStyle::Word => {
                for (index, (start, end)) in windows.iter().enumerate() {
                    let text = format!(
                        "{{\\fscx85\\fscy85\\t(0,70,\\fscx{scale}\\fscy{scale})\
                         \\t(70,140,\\fscx100\\fscy100)}}{}",
                        escape_ass(&group[index].word)
                    );
                    lines.push(dialogue(*start, *end, &text));
                }
            }
            SubtitleStyle::Highlight | SubtitleStyle::Box => {
                for (index, (start, end)) in windows.iter().enumerate() {
                    let parts: Vec<String> = group
                        .iter()
                        .enumerate()
                        .map(|(pos, word)| {
                            let escaped = escape_ass(&word.word);
                            if pos != index {
                                return escaped;
                            }
                            if style == SubtitleStyle::Box {
                                format!(
                                    "{{\\bord{box_border}\\3c{highlight}&\\shad0}}{escaped}\
                                     {{\\r}}"
                                )
                            } else {
                                format!(
                                    "{{\\c{highlight}&\\fscx{scale}\\fscy{scale}}}{escaped}\
                                     {{\\c{text_color}&\\3c{outline_color}&\\fscx100\\fscy100}}"
                                )
                            }
                        })
                        .collect();
                    lines.push(dialogue(*start, *end, &parts.join(" ")));
                }
            }
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
    pub font_name: &'a str,
    pub font_size: i64,
    pub bold: bool,
    pub text_color: &'a str,
    pub highlight_color: &'a str,
    pub outline_color: &'a str,
    /// Espessura do contorno, em pixels da resolução do vídeo.
    pub outline: f64,
    /// Deslocamento da sombra, em pixels (0 desliga).
    pub shadow: f64,
    /// Escala (%) da palavra em destaque nos estilos highlight e word.
    pub highlight_scale: i64,
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
    let margin_v = match options.position {
        SubtitlePosition::Center => 0,
        SubtitlePosition::Top | SubtitlePosition::Bottom => (height as f64 * 0.16) as i64,
    };
    let margin_h = (width as f64 * 0.06) as i64;
    // Sombra escura semitransparente: some com shadow=0.
    let back = "&H80000000";
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
         Style: Default,{font},{font_size},{primary},{primary},{outline},{back},\
         {bold},0,0,0,100,100,0,0,1,{outline_px},{shadow_px},{alignment},{margin_h},{margin_h},{margin_v},1\n\n\
         [Events]\n\
         Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        font = options.font_name,
        bold = if options.bold { -1 } else { 0 },
        outline_px = format_px(options.outline),
        shadow_px = format_px(options.shadow),
        alignment = options.position.alignment(),
    );
    let palette = Palette {
        text: primary,
        highlight,
        outline,
        highlight_scale: options.highlight_scale,
        box_border: round_to((f64::from(font_size as i32) * 0.32).max(4.0), 1),
    };
    let lines = events(&groups, options.style, &palette);
    let duration = cleaned.iter().map(|w| w.end).fold(f64::MIN, f64::max);
    Ok((
        format!("{header}{}\n", lines.join("\n")),
        cleaned.len(),
        groups.len(),
        duration,
    ))
}

fn format_px(value: f64) -> String {
    let rounded = round_to(value.max(0.0), 1);
    if rounded.fract() == 0.0 {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded}")
    }
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
    /// Visual pronto: hormozi, boxed, karaoke, pop, clean ou neon. Define
    /// estilo, cores, tamanho, contorno e posição de uma vez; qualquer outro
    /// parâmetro informado junto sobrepõe o valor do preset.
    #[serde(default)]
    pub preset: Option<SubtitlePreset>,
    /// highlight, word, block, karaoke ou box. Padrão highlight.
    #[serde(default)]
    pub style: Option<SubtitleStyle>,
    /// Máximo de palavras por bloco (3 a 5 é o usual).
    #[serde(default)]
    pub max_words: Option<i64>,
    /// Pausa em segundos que força um bloco novo.
    #[serde(default = "default_max_gap")]
    pub max_gap: f64,
    /// Nome da fonte instalada no sistema (ex: "Montserrat", "Arial Black").
    /// Padrão "DejaVu Sans", que vai embutida e existe em qualquer máquina.
    #[serde(default)]
    pub font_name: Option<String>,
    /// Tamanho da fonte em pixels. Calculado pela altura do vídeo se omitido.
    #[serde(default)]
    pub font_size: Option<i64>,
    /// Negrito. Padrão true.
    #[serde(default)]
    pub bold: Option<bool>,
    /// Cor do texto em hexadecimal, ex: #FFFFFF.
    #[serde(default)]
    pub text_color: Option<String>,
    /// Cor da palavra em destaque (ou da tarja no estilo box), ex: #FFD700 ou #00FF88.
    #[serde(default)]
    pub highlight_color: Option<String>,
    /// Cor do contorno, ex: #000000.
    #[serde(default)]
    pub outline_color: Option<String>,
    /// Espessura do contorno em pixels. Calculada pelo tamanho da fonte se omitida.
    #[serde(default)]
    pub outline: Option<f64>,
    /// Sombra em pixels (0 desliga). Calculada pelo preset se omitida.
    #[serde(default)]
    pub shadow: Option<f64>,
    /// top, center ou bottom. center é o padrão em vídeo vertical.
    #[serde(default)]
    pub position: Option<SubtitlePosition>,
    /// Converte o texto para maiúsculas, como nos cortes virais. Padrão true.
    #[serde(default)]
    pub uppercase: Option<bool>,
}

fn default_max_gap() -> f64 {
    1.0
}

/// Opções da tool além de `words` e `output`, já resolvidas.
///
/// `font_size`, `outline` e `shadow` em `None` são calculados pela altura do
/// vídeo (`font_scale`, `outline_scale` e `shadow_scale`, frações do tamanho).
#[derive(Debug, Clone)]
pub struct DynamicSubtitlesOptions {
    pub video_path: Option<String>,
    pub preset: Option<SubtitlePreset>,
    pub style: SubtitleStyle,
    pub max_words: i64,
    pub max_gap: f64,
    pub font_name: String,
    pub font_size: Option<i64>,
    /// Fração da altura do vídeo usada como tamanho da fonte quando `font_size` é None.
    pub font_scale: f64,
    pub bold: bool,
    pub text_color: String,
    pub highlight_color: String,
    pub outline_color: String,
    pub outline: Option<f64>,
    /// Fração do tamanho da fonte usada como contorno quando `outline` é None.
    pub outline_scale: f64,
    pub shadow: Option<f64>,
    /// Fração do tamanho da fonte usada como sombra quando `shadow` é None.
    pub shadow_scale: f64,
    pub highlight_scale: i64,
    pub position: SubtitlePosition,
    pub uppercase: bool,
}

impl Default for DynamicSubtitlesOptions {
    fn default() -> Self {
        Self {
            video_path: None,
            preset: None,
            style: SubtitleStyle::Highlight,
            max_words: 4,
            max_gap: default_max_gap(),
            font_name: DEFAULT_FONT.to_string(),
            font_size: None,
            font_scale: 0.045,
            bold: true,
            text_color: "#FFFFFF".to_string(),
            highlight_color: "#FFD700".to_string(),
            outline_color: "#000000".to_string(),
            outline: None,
            outline_scale: 0.07,
            shadow: None,
            shadow_scale: 0.035,
            highlight_scale: 108,
            position: SubtitlePosition::Center,
            uppercase: true,
        }
    }
}

impl DynamicSubtitlesOptions {
    /// Opções de um preset visual.
    pub fn preset(preset: SubtitlePreset) -> Self {
        let base = Self {
            preset: Some(preset),
            ..Self::default()
        };
        match preset {
            SubtitlePreset::Hormozi => Self {
                style: SubtitleStyle::Highlight,
                max_words: 3,
                font_scale: 0.058,
                highlight_color: "#FFD700".to_string(),
                outline_scale: 0.1,
                shadow_scale: 0.06,
                highlight_scale: 112,
                position: SubtitlePosition::Center,
                uppercase: true,
                ..base
            },
            SubtitlePreset::Boxed => Self {
                style: SubtitleStyle::Box,
                max_words: 4,
                font_scale: 0.05,
                highlight_color: "#7C3AED".to_string(),
                outline_scale: 0.06,
                shadow_scale: 0.0,
                position: SubtitlePosition::Center,
                uppercase: true,
                ..base
            },
            SubtitlePreset::Karaoke => Self {
                style: SubtitleStyle::Karaoke,
                max_words: 5,
                font_scale: 0.048,
                highlight_color: "#FF2D95".to_string(),
                outline_scale: 0.07,
                shadow_scale: 0.03,
                position: SubtitlePosition::Bottom,
                uppercase: false,
                ..base
            },
            SubtitlePreset::Pop => Self {
                style: SubtitleStyle::Word,
                max_words: 1,
                font_scale: 0.075,
                outline_scale: 0.09,
                shadow_scale: 0.05,
                highlight_scale: 115,
                position: SubtitlePosition::Center,
                uppercase: true,
                ..base
            },
            SubtitlePreset::Clean => Self {
                style: SubtitleStyle::Block,
                max_words: 6,
                font_scale: 0.04,
                bold: false,
                outline_scale: 0.05,
                shadow_scale: 0.0,
                position: SubtitlePosition::Bottom,
                uppercase: false,
                ..base
            },
            SubtitlePreset::Neon => Self {
                style: SubtitleStyle::Highlight,
                max_words: 4,
                font_scale: 0.052,
                highlight_color: "#39FF14".to_string(),
                outline_color: "#0A2A0A".to_string(),
                outline_scale: 0.09,
                shadow_scale: 0.0,
                highlight_scale: 110,
                position: SubtitlePosition::Center,
                uppercase: true,
                ..base
            },
        }
    }

    /// Parte do preset (ou do padrão) e aplica só o que o agente informou.
    pub fn from_params(params: &Params) -> Self {
        let mut options = params.preset.map_or_else(Self::default, Self::preset);
        options.video_path = params.video_path.clone();
        options.max_gap = params.max_gap;
        if let Some(style) = params.style {
            options.style = style;
        }
        if let Some(max_words) = params.max_words {
            options.max_words = max_words;
        }
        if let Some(font_name) = &params.font_name {
            options.font_name = font_name.clone();
        }
        if let Some(font_size) = params.font_size {
            options.font_size = Some(font_size);
        }
        if let Some(bold) = params.bold {
            options.bold = bold;
        }
        if let Some(color) = &params.text_color {
            options.text_color = color.clone();
        }
        if let Some(color) = &params.highlight_color {
            options.highlight_color = color.clone();
        }
        if let Some(color) = &params.outline_color {
            options.outline_color = color.clone();
        }
        if let Some(outline) = params.outline {
            options.outline = Some(outline);
        }
        if let Some(shadow) = params.shadow {
            options.shadow = Some(shadow);
        }
        if let Some(position) = params.position {
            options.position = position;
        }
        if let Some(uppercase) = params.uppercase {
            options.uppercase = uppercase;
        }
        options
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
    if options.font_name.trim().is_empty() {
        return Err(ToolError::new(
            "font_name não pode ser vazio.",
            ErrorCode::InvalidArgument,
        ));
    }
    if options.outline.is_some_and(|v| v < 0.0) || options.shadow.is_some_and(|v| v < 0.0) {
        return Err(ToolError::new(
            "outline e shadow não podem ser negativos.",
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
        .unwrap_or_else(|| ((height as f64 * options.font_scale) as i64).max(20));
    if size <= 0 {
        return Err(ToolError::new(
            "font_size deve ser maior que zero.",
            ErrorCode::InvalidArgument,
        ));
    }
    let outline = options
        .outline
        .unwrap_or_else(|| (size as f64 * options.outline_scale).max(1.0));
    let shadow = options.shadow.unwrap_or(size as f64 * options.shadow_scale);
    let (content, count, groups, duration) = build_ass(
        words,
        &AssOptions {
            style: options.style,
            max_words: options.max_words as usize,
            max_gap: options.max_gap,
            font_name: &options.font_name,
            font_size: size,
            bold: options.bold,
            text_color: &options.text_color,
            highlight_color: &options.highlight_color,
            outline_color: &options.outline_color,
            outline,
            shadow,
            highlight_scale: options.highlight_scale,
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
        preset: options.preset,
        words: count,
        groups,
        duration: round_to(duration, 3),
        play_res: format!("{width}x{height}"),
        font_size: size,
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
         (max_gap).\n\n\
         O jeito mais rápido de ter um resultado profissional é escolher um preset:\n\
         - \"hormozi\": maiúsculas grandes, 3 palavras, destaque amarelo, contorno grosso \
         e sombra (cortes de podcast, negócios, motivação).\n\
         - \"boxed\": a palavra falada ganha uma tarja roxa atrás (visual CapCut).\n\
         - \"karaoke\": a cor preenche cada palavra da esquerda para a direita, no rodapé.\n\
         - \"pop\": uma palavra gigante por vez, com pop (ganchos e frases de impacto).\n\
         - \"clean\": bloco discreto no rodapé, sem maiúsculas (documental, corporativo).\n\
         - \"neon\": destaque verde-neon (gaming e tech).\n\
         Qualquer parâmetro informado junto do preset sobrepõe o valor dele (ex.: \
         preset=hormozi com highlight_color=#00FF88). Sem preset, os estilos são \
         highlight (bloco visível, palavra falada muda de cor e cresce), word (uma \
         por vez), block (bloco inteiro com fade), karaoke e box.\n\n\
         Informe video_path para a legenda ser dimensionada para a resolução certa \
         (padrão 1080x1920): fonte, contorno e sombra são calculados em proporção à \
         altura do vídeo, então o mesmo preset fica igual em 720p e 4K. font_name \
         aceita qualquer fonte instalada na máquina. Depois grave no vídeo com \
         burn_subtitles, que respeita o estilo do .ass.",
        move |params: Params| {
            let options = DynamicSubtitlesOptions::from_params(&params);
            guarded(create_dynamic_subtitles(
                &runtime,
                &params.words,
                &params.output,
                &options,
            ))
        },
    );
}
