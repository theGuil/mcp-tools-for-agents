"""Tool ``add_text_overlay``: escreve um texto (título, descrição) sobre o vídeo."""

from __future__ import annotations

from typing import TYPE_CHECKING, Literal, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.fonts import escape_drawtext, escape_filter_path, require_font
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

type TextPosition = Literal["top", "center", "bottom"]

_POSITIONS: dict[TextPosition, str] = {
    "top": "h*0.08",
    "center": "(h-text_h)/2",
    "bottom": "h-text_h-h*0.08",
}
_MAX_TEXT = 500


class AddTextOverlayResult(TypedDict):
    """Vídeo gerado com o texto."""

    output: str
    text: str
    position: TextPosition
    start: float
    end: float | None


def _do_overlay(
    runtime: Runtime,
    path: str,
    text: str,
    *,
    position: TextPosition,
    start: float,
    end: float | None,
    font_size: int,
    color: str,
) -> AddTextOverlayResult:
    source = runtime.workspace.existing(path)
    text = text.strip()
    if not text or len(text) > _MAX_TEXT:
        raise ToolError(
            f"text deve ter entre 1 e {_MAX_TEXT} caracteres.",
            code="invalid_argument",
            hint="Quebre textos longos em linhas com \\n ou use burn_subtitles.",
        )
    if font_size <= 0:
        raise ToolError("font_size deve ser maior que zero.", code="invalid_argument")
    if start < 0 or (end is not None and end <= start):
        raise ToolError(
            f"Intervalo inválido: start={start}, end={end}.",
            code="invalid_argument",
            hint="start deve ser >= 0 e end maior que start, ou omitido para ir até o fim.",
        )
    info = runtime.ffmpeg.probe(source)
    if start >= info["duration"]:
        raise ToolError(
            f"start={start}s ultrapassa a duração do vídeo ({info['duration']:.2f}s).",
            code="invalid_argument",
            hint="Use probe_video para conferir a duração.",
        )
    font = require_font()
    enable = f"between(t\\,{start}\\,{end})" if end is not None else f"gte(t\\,{start})"
    drawtext = (
        f"drawtext=fontfile='{escape_filter_path(font)}':text='{escape_drawtext(text)}'"
        f":fontsize={font_size}:fontcolor={color}:borderw=3:bordercolor=black@0.8"
        f":x=(w-text_w)/2:y={_POSITIONS[position]}:line_spacing=8:enable='{enable}'"
    )
    output = runtime.workspace.output_for(source, "text")
    audio_args = ["-c:a", "copy"] if info["has_audio"] else []
    runtime.ffmpeg.run(
        [
            "-i",
            str(source),
            "-vf",
            drawtext,
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            *audio_args,
            str(output),
        ]
    )
    return AddTextOverlayResult(
        output=runtime.workspace.relative(output),
        text=text,
        position=position,
        start=start,
        end=end,
    )


@guarded
def add_text_overlay(
    runtime: Runtime,
    path: str,
    text: str,
    *,
    position: TextPosition = "bottom",
    start: float = 0.0,
    end: float | None = None,
    font_size: int = 42,
    color: str = "white",
    background: bool = False,
) -> AddTextOverlayResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "add_text_overlay",
            lambda: _do_overlay(
                runtime,
                path,
                text,
                position=position,
                start=start,
                end=end,
                font_size=font_size,
                color=color,
            ),
        )
    return _do_overlay(
        runtime,
        path,
        text,
        position=position,
        start=start,
        end=end,
        font_size=font_size,
        color=color,
    )


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="add_text_overlay")
    def _tool(  # noqa: PLR0917  # a assinatura da tool é a interface do agente
        path: str,
        text: str,
        position: TextPosition = "bottom",
        start: float = 0.0,
        end: float | None = None,
        font_size: int = 42,
        color: str = "white",
        background: bool = False,
    ) -> AddTextOverlayResult | JobSubmitted | ErrorPayload:
        r"""Escreve um texto fixo sobre o vídeo (título, descrição, chamada) e salva novo arquivo.

        Serve para colocar a descrição ou o título que você escreveu direto na imagem.
        O texto fica centralizado horizontalmente, com contorno preto para leitura.
        Use \\n para quebrar linhas. Para falas sincronizadas use burn_subtitles.
        O original não é modificado; o vídeo é re-encodado.

        Args:
            path: Vídeo, relativo ao workspace.
            text: Texto a exibir. Até 500 caracteres.
            position: top, center ou bottom.
            start: Segundo em que o texto aparece.
            end: Segundo em que o texto some. Omitido = até o fim.
            font_size: Tamanho da fonte em pixels.
            color: Cor do texto (white, yellow, #FF0000...).
            background: Executa como job e devolve job_id.
        """
        return add_text_overlay(
            runtime,
            path,
            text,
            position=position,
            start=start,
            end=end,
            font_size=font_size,
            color=color,
            background=background,
        )
