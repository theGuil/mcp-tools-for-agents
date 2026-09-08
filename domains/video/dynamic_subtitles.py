"""Tool ``create_dynamic_subtitles``: legenda animada palavra por palavra (estilo TikTok)."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final, Literal, TypedDict

from core.errors import ErrorPayload, ToolError, guarded

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

type SubtitleStyle = Literal["highlight", "word", "block"]
type SubtitlePosition = Literal["top", "center", "bottom"]

_MAX_WORDS: Final = 12
_MIN_WORD_GAP: Final = 0.05
_HEX_DIGITS: Final = set("0123456789abcdefABCDEF")
_HEX_LEN: Final = 6
_DEFAULT_WIDTH: Final = 1080
_DEFAULT_HEIGHT: Final = 1920
_ALIGNMENT: Final[dict[SubtitlePosition, int]] = {"bottom": 2, "center": 5, "top": 8}


class WordTiming(TypedDict):
    """Uma palavra com seus tempos (mesmo formato de transcribe_audio)."""

    start: float
    end: float
    word: str


class CreateDynamicSubtitlesResult(TypedDict):
    """Arquivo .ass gerado."""

    output: str
    style: SubtitleStyle
    words: int
    groups: int
    duration: float
    play_res: str


def ass_color(hex_color: str, name: str) -> str:
    """Converte ``#RRGGBB`` para o formato ``&H00BBGGRR`` do ASS."""
    raw = hex_color.removeprefix("#")
    if len(raw) != _HEX_LEN or any(c not in _HEX_DIGITS for c in raw):
        raise ToolError(
            f"{name} deve ser uma cor hexadecimal como #FFFFFF ou #FFD700.",
            code="invalid_argument",
        )
    red, green, blue = raw[0:2], raw[2:4], raw[4:6]
    return f"&H00{blue}{green}{red}".upper()


def ass_time(seconds: float) -> str:
    """Converte segundos para ``H:MM:SS.cc`` (centésimos) do ASS."""
    total_cs = round(seconds * 100)
    hours, rest = divmod(total_cs, 360_000)
    minutes, rest = divmod(rest, 6_000)
    secs, cents = divmod(rest, 100)
    return f"{hours}:{minutes:02d}:{secs:02d}.{cents:02d}"


def _escape_ass(text: str) -> str:
    return text.replace("\\", "\\\\").replace("{", "(").replace("}", ")")


def _validate_words(words: list[WordTiming], *, uppercase: bool) -> list[WordTiming]:
    if not words:
        raise ToolError(
            "words está vazio.",
            code="invalid_argument",
            hint="Passe a lista words de transcribe_audio (junte as words de todos os segments).",
        )
    cleaned: list[WordTiming] = []
    for index, item in enumerate(words, start=1):
        text = item["word"].strip()
        start, end = item["start"], item["end"]
        if not text:
            continue
        if start < 0 or end < start:
            raise ToolError(
                f"Palavra {index} com intervalo inválido: start={start}, end={end}.",
                code="invalid_argument",
                hint="start deve ser >= 0 e end >= start, em segundos.",
            )
        if cleaned and start < cleaned[-1]["start"]:
            raise ToolError(
                f"Palavra {index} ('{text}') começa antes da anterior.",
                code="invalid_argument",
                hint="Envie as palavras em ordem cronológica.",
            )
        cleaned.append(WordTiming(start=start, end=end, word=text.upper() if uppercase else text))
    if not cleaned:
        raise ToolError("Nenhuma palavra com texto em words.", code="invalid_argument")
    return cleaned


def _group_words(words: list[WordTiming], max_words: int, max_gap: float) -> list[list[WordTiming]]:
    """Agrupa palavras em blocos curtos, quebrando em pausas longas."""
    groups: list[list[WordTiming]] = []
    current: list[WordTiming] = []
    for word in words:
        if current and (len(current) >= max_words or word["start"] - current[-1]["end"] > max_gap):
            groups.append(current)
            current = []
        current.append(word)
    if current:
        groups.append(current)
    return groups


def _word_windows(group: list[WordTiming]) -> list[tuple[float, float]]:
    """Janela de exibição de cada palavra: do início dela até o início da próxima."""
    windows: list[tuple[float, float]] = []
    for index, word in enumerate(group):
        start = word["start"]
        end = group[index + 1]["start"] if index + 1 < len(group) else word["end"]
        if end - start < _MIN_WORD_GAP:
            end = start + _MIN_WORD_GAP
        windows.append((start, end))
    return windows


def _events(
    groups: list[list[WordTiming]],
    *,
    style: SubtitleStyle,
    highlight: str,
    text_color: str,
) -> list[str]:
    lines: list[str] = []
    for group in groups:
        windows = _word_windows(group)
        if style == "block":
            start, end = windows[0][0], max(w["end"] for w in group)
            text = " ".join(_escape_ass(w["word"]) for w in group)
            lines.append(_dialogue(start, end, "{\\fad(80,80)}" + text))
            continue
        for index, (start, end) in enumerate(windows):
            if style == "word":
                text = "{\\fscx85\\fscy85\\t(0,70,\\fscx100\\fscy100)}" + _escape_ass(
                    group[index]["word"]
                )
            else:
                parts = []
                for pos, word in enumerate(group):
                    escaped = _escape_ass(word["word"])
                    if pos == index:
                        parts.append(
                            f"{{\\c{highlight}&\\fscx108\\fscy108}}{escaped}"
                            f"{{\\c{text_color}&\\fscx100\\fscy100}}"
                        )
                    else:
                        parts.append(escaped)
                text = " ".join(parts)
            lines.append(_dialogue(start, end, text))
    return lines


def _dialogue(start: float, end: float, text: str) -> str:
    return f"Dialogue: 0,{ass_time(start)},{ass_time(end)},Default,,0,0,0,,{text}"


def build_ass(
    words: list[WordTiming],
    *,
    style: SubtitleStyle,
    max_words: int,
    max_gap: float,
    font_size: int,
    text_color: str,
    highlight_color: str,
    outline_color: str,
    position: SubtitlePosition,
    uppercase: bool,
    width: int,
    height: int,
) -> tuple[str, int, int, float]:
    """Monta o conteúdo do .ass. Devolve (conteúdo, palavras, grupos, duração)."""
    cleaned = _validate_words(words, uppercase=uppercase)
    groups = _group_words(cleaned, max_words, max_gap)
    primary = ass_color(text_color, "text_color")
    highlight = ass_color(highlight_color, "highlight_color")
    outline = ass_color(outline_color, "outline_color")
    margin_v = int(height * 0.18)
    header = (
        "[Script Info]\n"
        "ScriptType: v4.00+\n"
        f"PlayResX: {width}\n"
        f"PlayResY: {height}\n"
        "WrapStyle: 0\n"
        "ScaledBorderAndShadow: yes\n\n"
        "[V4+ Styles]\n"
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, "
        "BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, "
        "BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n"
        f"Style: Default,DejaVu Sans,{font_size},{primary},{primary},{outline},&H80000000,"
        f"-1,0,0,0,100,100,0,0,1,{max(font_size // 14, 2)},{max(font_size // 30, 1)},"
        f"{_ALIGNMENT[position]},{int(width * 0.06)},{int(width * 0.06)},{margin_v},1\n\n"
        "[Events]\n"
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n"
    )
    events = _events(groups, style=style, highlight=highlight, text_color=primary)
    duration = max(w["end"] for w in cleaned)
    return header + "\n".join(events) + "\n", len(cleaned), len(groups), duration


@guarded
def create_dynamic_subtitles(
    runtime: Runtime,
    words: list[WordTiming],
    output: str,
    *,
    video_path: str | None = None,
    style: SubtitleStyle = "highlight",
    max_words: int = 4,
    max_gap: float = 1.0,
    font_size: int | None = None,
    text_color: str = "#FFFFFF",
    highlight_color: str = "#FFD700",
    outline_color: str = "#000000",
    position: SubtitlePosition = "center",
    uppercase: bool = True,
) -> CreateDynamicSubtitlesResult:
    """Implementação pura, testável sem MCP."""
    target = runtime.workspace.resolve(output)
    if target.suffix.lower() != ".ass":
        raise ToolError(
            f"output '{output}' deve terminar em .ass.",
            code="invalid_argument",
            hint="Ex: legendas/video.ass. Depois aplique com burn_subtitles.",
        )
    if style not in {"highlight", "word", "block"}:
        raise ToolError(
            f"style '{style}' não existe.",
            code="invalid_argument",
            hint="Use highlight, word ou block.",
        )
    if position not in _ALIGNMENT:
        raise ToolError("position deve ser top, center ou bottom.", code="invalid_argument")
    if not 1 <= max_words <= _MAX_WORDS:
        raise ToolError(
            f"max_words deve estar entre 1 e {_MAX_WORDS}.",
            code="invalid_argument",
            hint="3 a 5 palavras por bloco é o padrão de TikTok e Reels.",
        )
    if max_gap <= 0:
        raise ToolError("max_gap deve ser maior que zero.", code="invalid_argument")
    width, height = _DEFAULT_WIDTH, _DEFAULT_HEIGHT
    if video_path is not None:
        info = runtime.ffmpeg.probe(runtime.workspace.existing(video_path))
        if info["width"] and info["height"]:
            width, height = info["width"], info["height"]
    size = font_size if font_size is not None else max(int(height * 0.045), 20)
    if size <= 0:
        raise ToolError("font_size deve ser maior que zero.", code="invalid_argument")
    content, count, groups, duration = build_ass(
        words,
        style=style,
        max_words=max_words,
        max_gap=max_gap,
        font_size=size,
        text_color=text_color,
        highlight_color=highlight_color,
        outline_color=outline_color,
        position=position,
        uppercase=uppercase,
        width=width,
        height=height,
    )
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8")
    return CreateDynamicSubtitlesResult(
        output=runtime.workspace.relative(target),
        style=style,
        words=count,
        groups=groups,
        duration=round(duration, 3),
        play_res=f"{width}x{height}",
    )


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="create_dynamic_subtitles")
    def _tool(  # noqa: PLR0917  # a assinatura da tool é a interface do agente
        words: list[WordTiming],
        output: str,
        video_path: str | None = None,
        style: SubtitleStyle = "highlight",
        max_words: int = 4,
        max_gap: float = 1.0,
        font_size: int | None = None,
        text_color: str = "#FFFFFF",
        highlight_color: str = "#FFD700",
        outline_color: str = "#000000",
        position: SubtitlePosition = "center",
        uppercase: bool = True,
    ) -> CreateDynamicSubtitlesResult | ErrorPayload:
        """Cria legendas animadas palavra por palavra (estilo TikTok/Reels) em um arquivo .ass.

        Fluxo: transcribe_audio (word_timestamps=true) -> junte as words de todos os
        segments em uma lista -> create_dynamic_subtitles -> burn_subtitles. As
        palavras são agrupadas em blocos curtos (max_words) que quebram nas pausas
        (max_gap). Estilos:
        - "highlight": o bloco fica visível e a palavra falada no momento muda de cor
          e cresce um pouco (o mais usado em cortes de podcast).
        - "word": aparece uma palavra de cada vez, grande, com efeito de pop.
        - "block": o bloco inteiro aparece de uma vez, sem destaque, com fade.
        Informe video_path para a legenda ser dimensionada para a resolução certa
        (padrão 1080x1920). O tamanho da fonte é calculado pela altura do vídeo se
        font_size for omitido. Depois grave no vídeo com burn_subtitles, que
        respeita o estilo do .ass.

        Args:
            words: Lista de {start, end, word}, em segundos, em ordem cronológica.
            output: Caminho do .ass a criar, relativo ao workspace.
            video_path: Vídeo alvo, para ajustar tamanho e posição à resolução dele.
            style: highlight, word ou block.
            max_words: Máximo de palavras por bloco (3 a 5 é o usual).
            max_gap: Pausa em segundos que força um bloco novo.
            font_size: Tamanho da fonte. Calculado pela altura do vídeo se omitido.
            text_color: Cor do texto em hexadecimal, ex: #FFFFFF.
            highlight_color: Cor da palavra em destaque, ex: #FFD700 (amarelo) ou #00FF88.
            outline_color: Cor do contorno, ex: #000000.
            position: top, center ou bottom. center é o padrão em vídeo vertical.
            uppercase: Converte o texto para maiúsculas, como nos cortes virais.
        """
        return create_dynamic_subtitles(
            runtime,
            words,
            output,
            video_path=video_path,
            style=style,
            max_words=max_words,
            max_gap=max_gap,
            font_size=font_size,
            text_color=text_color,
            highlight_color=highlight_color,
            outline_color=outline_color,
            position=position,
            uppercase=uppercase,
        )
