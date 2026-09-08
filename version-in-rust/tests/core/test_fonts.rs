use mcp_tools::core::fonts::{escape_drawtext, require_font};

const BS: &str = "\\";

#[test]
fn test_escape_drawtext_plain() {
    assert_eq!(escape_drawtext("Olá mundo"), "Olá mundo");
}

#[test]
fn test_escape_drawtext_percent() {
    // % vira \% para o drawtext, e cada camada acima dobra a barra.
    assert_eq!(escape_drawtext("100%"), format!("100{}%", BS.repeat(4)));
}

#[test]
fn test_escape_drawtext_colon() {
    assert_eq!(escape_drawtext("a:b"), format!("a{}:b", BS.repeat(2)));
}

#[test]
fn test_escape_drawtext_quote() {
    assert_eq!(escape_drawtext("d'agua"), format!("d{}'agua", BS.repeat(3)));
}

#[test]
fn test_escape_drawtext_graph_separators() {
    assert_eq!(
        escape_drawtext("a,b;[c]"),
        format!("a{BS},b{BS};{BS}[c{BS}]")
    );
}

#[test]
fn test_escape_drawtext_backslash() {
    assert_eq!(
        escape_drawtext(&format!("x{BS}y")),
        format!("x{}y", BS.repeat(8))
    );
}

#[test]
fn test_escape_drawtext_keeps_newline() {
    assert_eq!(escape_drawtext("linha1\nlinha2"), "linha1\nlinha2");
}

#[test]
fn test_require_font_always_finds_one() {
    // Sem fonte no sistema, a DejaVu embutida é gravada no cache.
    let font = require_font().unwrap();
    assert!(font.is_file());
}
