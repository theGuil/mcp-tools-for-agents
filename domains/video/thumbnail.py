"""Tool ``create_thumbnail``: capa do vídeo com título em destaque."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final, Literal, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.fonts import escape_drawtext, escape_filter_path, require_font

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

type ThumbnailFormat = Literal["youtube", "vertical", "square", "original"]
type TextPosition = Literal["top", "center", "bottom"]

_SIZES: Final[dict[ThumbnailFormat, tuple[int, int]]] = {
    "youtube": (1280, 720),
    "vertical": (1080, 1920),
    "square": (1080, 1080),
}
_MAX_TITLE: Final = 80
_IMAGE_SUFFIXES: Final = {".png", ".jpg", ".jpeg", ".webp"}
_HEX_DIGITS: Final = set("0123456789abcdefABCDEF")
_HEX_LEN: Final = 6


class CreateThumbnailResult(TypedDict):
    """Imagem gerada."""

    output: str
    width: int
    height: int
    time: float | None
    title: str | None
    size_bytes: int


def _validate_color(name: str, value: str) -> str:
    raw = value.removeprefix("#")
    if len(raw) != _HEX_LEN or any(c not in _HEX_DIGITS for c in raw):
        raise ToolError(
            f"{name} deve ser uma cor hexadecimal como #FFFFFF ou #FFD700.",
            code="invalid_argument",
        )
    return f"0x{raw}"


def _text_y(position: TextPosition) -> str:
    if position == "top":
        return "h*0.08"
    if position == "center":
        return "(h-text_h)/2"
    return "h-text_h-h*0.08"


def _wrap_title(title: str, max_chars: int) -> str:
    """Quebra o título em linhas para não estourar a largura da imagem."""
    lines: list[str] = []
    current: list[str] = []
    for word in title.split():
        candidate = " ".join([*current, word])
        if current and len(candidate) > max_chars:
            lines.append(" ".join(current))
            current = [word]
        else:
            current.append(word)
    if current:
        lines.append(" ".join(current))
    return "\n".join(lines)


def _title_filter(
    title: str,
    *,
    height: int,
    position: TextPosition,
    text_color: str,
    border_color: str,
    darken: bool,
) -> str:
    font = require_font()
    font_size = max(int(height * 0.09), 28)
    steps: list[str] = []
    if darken:
        steps.append("eq=brightness=-0.12:contrast=1.1")
    y = _text_y(position)
    steps.append(
        f"drawtext=fontfile='{escape_filter_path(font)}':text={escape_drawtext(title)}"
        f":fontsize={font_size}:fontcolor={text_color}:borderw={max(font_size // 12, 3)}"
        f":bordercolor={border_color}:shadowcolor=black@0.6:shadowx=4:shadowy=4"
        f":x=(w-text_w)/2:y={y}:line_spacing={font_size // 6}"
    )
    return ",".join(steps)


def _canvas_filter(width: int, height: int) -> str:
    return f"scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height}"


@guarded
def create_thumbnail(
    runtime: Runtime,
    path: str,
    *,
    time: float | None = None,
    title: str | None = None,
    thumbnail_format: ThumbnailFormat = "youtube",
    position: TextPosition = "center",
    text_color: str = "#FFFFFF",
    border_color: str = "#000000",
    darken: bool = True,
    output: str | None = None,
) -> CreateThumbnailResult:
    """Implementação pura, testável sem MCP."""
    source = runtime.workspace.existing(path)
    if thumbnail_format not in {*_SIZES, "original"}:
        raise ToolError(
            f"thumbnail_format '{thumbnail_format}' não existe.",
            code="invalid_argument",
            hint="Use youtube (1280x720), vertical (1080x1920), square ou original.",
        )
    if position not in {"top", "center", "bottom"}:
        raise ToolError("position deve ser top, center ou bottom.", code="invalid_argument")
    title = title.strip() if title else None
    if title is not None and len(title) > _MAX_TITLE:
        raise ToolError(
            f"title deve ter até {_MAX_TITLE} caracteres.",
            code="invalid_argument",
            hint="Thumbnail boa tem no máximo 4 ou 5 palavras grandes.",
        )
    fg = _validate_color("text_color", text_color)
    border = _validate_color("border_color", border_color)
    is_image = source.suffix.lower() in _IMAGE_SUFFIXES
    info = runtime.ffmpeg.probe(source)
    if not info["has_video"] or info["width"] is None or info["height"] is None:
        raise ToolError(
            f"'{path}' não tem imagem.",
            code="invalid_argument",
            hint="Informe um vídeo ou uma imagem (png, jpg, webp).",
        )
    if is_image:
        time = None
    else:
        time = info["duration"] / 2 if time is None else time
        if time < 0 or time > info["duration"]:
            raise ToolError(
                f"time={time}s está fora da duração do vídeo ({info['duration']:.2f}s).",
                code="invalid_argument",
                hint="Use probe_video para conferir a duração ou omita time para o meio.",
            )
    if thumbnail_format == "original":
        width, height = info["width"], info["height"]
        steps = []
    else:
        width, height = _SIZES[thumbnail_format]
        steps = [_canvas_filter(width, height)]
    if title:
        wrapped = _wrap_title(title, max_chars=14 if width < height else 22)
        steps.append(
            _title_filter(
                wrapped,
                height=height,
                position=position,
                text_color=fg,
                border_color=border,
                darken=darken,
            )
        )
    target = (
        runtime.workspace.resolve(output)
        if output
        else runtime.workspace.output_for(source, f"thumb_{thumbnail_format}", extension="jpg")
    )
    if target.suffix.lower() not in {".jpg", ".jpeg", ".png"}:
        raise ToolError(
            f"output '{output}' deve terminar em .jpg ou .png.",
            code="invalid_argument",
        )
    target.parent.mkdir(parents=True, exist_ok=True)
    seek = [] if time is None else ["-ss", f"{time}"]
    vf = ["-vf", ",".join(steps)] if steps else []
    runtime.ffmpeg.run([*seek, "-i", str(source), "-frames:v", "1", *vf, "-q:v", "2", str(target)])
    return CreateThumbnailResult(
        output=runtime.workspace.relative(target),
        width=width,
        height=height,
        time=None if time is None else round(time, 3),
        title=title,
        size_bytes=target.stat().st_size,
    )


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="create_thumbnail")
    def _tool(  # noqa: PLR0917  # a assinatura da tool é a interface do agente
        path: str,
        time: float | None = None,
        title: str | None = None,
        thumbnail_format: ThumbnailFormat = "youtube",
        position: TextPosition = "center",
        text_color: str = "#FFFFFF",
        border_color: str = "#000000",
        darken: bool = True,
        output: str | None = None,
    ) -> CreateThumbnailResult | ErrorPayload:
        """Cria a capa (thumbnail) do vídeo: um frame ampliado com título grande por cima.

        Use no final da edição para gerar a imagem de capa do YouTube (1280x720),
        do TikTok/Shorts (1080x1920 vertical) ou do feed (quadrado). Escolha um
        frame expressivo com extract_frame ou detect_scenes antes, ou omita time
        para usar o meio do vídeo. O título é quebrado em linhas automaticamente,
        com contorno e sombra para ler bem em qualquer fundo; darken escurece a
        imagem levemente para o texto saltar. Também aceita uma imagem (png, jpg)
        como base em vez de vídeo. Devolve um .jpg.

        Args:
            path: Vídeo ou imagem, relativo ao workspace.
            time: Segundo do frame a usar. Omita para o meio do vídeo. Ignorado para imagem.
            title: Texto da capa, até 80 caracteres. Poucas palavras funcionam melhor.
            thumbnail_format: youtube (1280x720), vertical (1080x1920), square (1080x1080)
                ou original (mantém o tamanho do vídeo).
            position: Onde o título fica: top, center ou bottom.
            text_color: Cor do texto em hexadecimal, ex: #FFFFFF ou #FFD700.
            border_color: Cor do contorno do texto em hexadecimal.
            darken: Escurece a imagem de fundo para o título se destacar.
            output: Caminho do .jpg/.png a criar. Gerado ao lado do vídeo se omitido.
        """
        return create_thumbnail(
            runtime,
            path,
            time=time,
            title=title,
            thumbnail_format=thumbnail_format,
            position=position,
            text_color=text_color,
            border_color=border_color,
            darken=darken,
            output=output,
        )
