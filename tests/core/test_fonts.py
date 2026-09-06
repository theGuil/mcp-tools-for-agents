from core.fonts import escape_drawtext

BS = "\\"


def test_escape_drawtext_plain() -> None:
    assert escape_drawtext("Olá mundo") == "Olá mundo"


def test_escape_drawtext_percent() -> None:
    # % vira \% para o drawtext, e cada camada acima dobra a barra.
    assert escape_drawtext("100%") == "100" + BS * 4 + "%"


def test_escape_drawtext_colon() -> None:
    assert escape_drawtext("a:b") == "a" + BS * 2 + ":b"


def test_escape_drawtext_quote() -> None:
    assert escape_drawtext("d'agua") == "d" + BS * 3 + "'agua"


def test_escape_drawtext_graph_separators() -> None:
    assert escape_drawtext("a,b;[c]") == "a" + BS + ",b" + BS + ";" + BS + "[c" + BS + "]"


def test_escape_drawtext_backslash() -> None:
    assert escape_drawtext("x" + BS + "y") == "x" + BS * 8 + "y"


def test_escape_drawtext_keeps_newline() -> None:
    assert escape_drawtext("linha1\nlinha2") == "linha1\nlinha2"
