//! Localiza uma fonte TrueType para os filtros `drawtext` e `subtitles`.
//!
//! Sem fonte o FFmpeg depende do fontconfig, que nem sempre existe. Aqui
//! procuramos uma fonte comum do sistema e, se não houver nenhuma, gravamos
//! a DejaVu Sans Bold embutida no binário (licença Bitstream Vera, que
//! permite redistribuição) na pasta de cache. É o que garante que texto e
//! legenda funcionem em qualquer máquina, sem instalar nada.

use std::path::{Path, PathBuf};

use crate::core::binaries::{default_cache_dir, exe_dir};
use crate::core::errors::{ErrorCode, ToolError, ToolResult};

const CANDIDATES: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
    "/usr/share/fonts/truetype/freefont/FreeSansBold.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/Library/Fonts/Arial Bold.ttf",
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "/System/Library/Fonts/Helvetica.ttc",
    "C:/Windows/Fonts/arialbd.ttf",
    "C:/Windows/Fonts/arial.ttf",
];

const EMBEDDED_NAME: &str = "DejaVuSans-Bold.ttf";
const EMBEDDED_FONT: &[u8] = include_bytes!("fonts/DejaVuSans-Bold.ttf");

/// Primeira fonte conhecida encontrada no sistema, ou `None`.
///
/// Procura, nesta ordem: a pasta `fonts/` ao lado do executável, as fontes
/// comuns do sistema e a cópia embutida já gravada no cache.
pub fn find_font() -> Option<PathBuf> {
    if let Some(dir) = exe_dir() {
        let bundled = dir.join("fonts").join(EMBEDDED_NAME);
        if bundled.is_file() {
            return Some(bundled);
        }
    }
    for candidate in CANDIDATES {
        let path = Path::new(candidate);
        if path.is_file() {
            return Some(path.to_path_buf());
        }
    }
    let cached = cached_font_path();
    cached.is_file().then_some(cached)
}

/// Fonte obrigatória para desenhar texto.
///
/// Sem fonte no sistema, grava a DejaVu embutida no cache e devolve o caminho.
///
/// # Errors
///
/// [`ErrorCode::Unavailable`] se nenhuma fonte for encontrada nem puder ser gravada.
pub fn require_font() -> ToolResult<PathBuf> {
    if let Some(font) = find_font() {
        return Ok(font);
    }
    let target = cached_font_path();
    let written = target
        .parent()
        .map(std::fs::create_dir_all)
        .transpose()
        .and_then(|_| std::fs::write(&target, EMBEDDED_FONT));
    match written {
        Ok(()) => Ok(target),
        Err(error) => Err(ToolError::with_hint(
            format!("Nenhuma fonte TrueType encontrada no sistema ({error})."),
            ErrorCode::Unavailable,
            "Instale a fonte DejaVu (ex: apt install fonts-dejavu-core) ou coloque um .ttf \
             em fonts/DejaVuSans-Bold.ttf ao lado do mcp-tools.",
        )),
    }
}

fn cached_font_path() -> PathBuf {
    default_cache_dir().join("fonts").join(EMBEDDED_NAME)
}

// O FFmpeg lê o valor de `text=` em três camadas, cada uma removendo um nível de
// escape: a expansão do próprio drawtext, o parser de opções do filtro e o parser
// do filtergraph. O texto é escapado de dentro para fora, uma camada por vez.
const DRAWTEXT_SPECIAL: &[char] = &['\\', '%'];
const OPTION_SPECIAL: &[char] = &['\\', '\'', ':'];
const GRAPH_SPECIAL: &[char] = &['\\', '\'', '[', ']', ',', ';'];

fn escape_layer(text: &str, special: &[char]) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        if special.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Escapa um texto para o valor de `text=` do filtro `drawtext`.
///
/// O resultado deve ir direto após `text=`, sem aspas: envolver em aspas
/// quebra quando o texto contém `'`. Quebras de linha reais são preservadas.
pub fn escape_drawtext(text: &str) -> String {
    let escaped = escape_layer(text, DRAWTEXT_SPECIAL);
    let escaped = escape_layer(&escaped, OPTION_SPECIAL);
    escape_layer(&escaped, GRAPH_SPECIAL)
}

/// Escapa um caminho para uso como valor dentro de um filtergraph.
pub fn escape_filter_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .replace(':', "\\:")
        .replace('\'', "\\'")
}
