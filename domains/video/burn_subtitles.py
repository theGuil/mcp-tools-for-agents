"""Tool ``burn_subtitles``: grava legendas de um .srt ou .ass na imagem do vídeo."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final, Literal, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.fonts import escape_filter_path, find_font
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

type SubtitlePosition = Literal["top", "center", "bottom"]

_ALIGNMENT: Final[dict[SubtitlePosition, int]] = {"bottom": 2, "center": 5, "top": 8}
_HEX_DIGITS: Final = set("0123456789abcdefABCDEF")
_HEX_LEN: Final = 6


class BurnSubtitlesResult(TypedDict):
    """Vídeo gerado com as legendas."""

    output: str
    subtitles: str
    font_size: int
    position: SubtitlePosition
    styled_by_file: bool


def _ass_color(hex_color: str, name: str) -> str:
    raw = hex_color.removeprefix("#")
    if len(raw) != _HEX_LEN or any(c not in _HEX_DIGITS for c in raw):
        raise ToolError(
            f"{name} deve ser uma cor hexadecimal como #FFFFFF.",
            code="invalid_argument",
        )
    return f"&H00{raw[4:6]}{raw[2:4]}{raw[0:2]}".upper()


def _do_burn(
    runtime: Runtime,
    path: str,
    subtitles_path: str,
    *,
    font_size: int,
    position: SubtitlePosition,
    text_color: str,
    outline_color: str,
) -> BurnSubtitlesResult:
    source = runtime.workspace.existing(path)
    subs = runtime.workspace.existing(subtitles_path)
    if subs.suffix.lower() not in {".srt", ".vtt", ".ass"}:
        raise ToolError(
            f"'{subtitles_path}' não é um arquivo de legenda (.srt, .vtt ou .ass).",
            code="invalid_argument",
            hint="Gere um .srt com create_subtitles ou um .ass com create_dynamic_subtitles.",
        )
    if font_size <= 0:
        raise ToolError("font_size deve ser maior que zero.", code="invalid_argument")
    if position not in _ALIGNMENT:
        raise ToolError("position deve ser top, center ou bottom.", code="invalid_argument")
    primary = _ass_color(text_color, "text_color")
    outline = _ass_color(outline_color, "outline_color")
    info = runtime.ffmpeg.probe(source)
    styled_by_file = subs.suffix.lower() == ".ass"
    filter_expr = f"subtitles='{escape_filter_path(subs)}'"
    if not styled_by_file:
        style = (
            f"FontSize={font_size},PrimaryColour={primary},OutlineColour={outline},"
            f"Outline=2,Shadow=0,Alignment={_ALIGNMENT[position]},MarginV=30"
        )
        filter_expr += f":force_style='{style}'"
    font = find_font()
    if font is not None:
        filter_expr += f":fontsdir='{escape_filter_path(font.parent)}'"
    output = runtime.workspace.output_for(source, "subtitled")
    audio_args = ["-c:a", "copy"] if info["has_audio"] else []
    runtime.ffmpeg.run(
        [
            "-i",
            str(source),
            "-vf",
            filter_expr,
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            *audio_args,
            str(output),
        ]
    )
    return BurnSubtitlesResult(
        output=runtime.workspace.relative(output),
        subtitles=runtime.workspace.relative(subs),
        font_size=font_size,
        position=position,
        styled_by_file=styled_by_file,
    )


@guarded
def burn_subtitles(
    runtime: Runtime,
    path: str,
    subtitles_path: str,
    *,
    font_size: int = 24,
    position: SubtitlePosition = "bottom",
    text_color: str = "#FFFFFF",
    outline_color: str = "#000000",
    background: bool = False,
) -> BurnSubtitlesResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "burn_subtitles",
            lambda: _do_burn(
                runtime,
                path,
                subtitles_path,
                font_size=font_size,
                position=position,
                text_color=text_color,
                outline_color=outline_color,
            ),
        )
    return _do_burn(
        runtime,
        path,
        subtitles_path,
        font_size=font_size,
        position=position,
        text_color=text_color,
        outline_color=outline_color,
    )


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="burn_subtitles")
    def _tool(  # noqa: PLR0917  # a assinatura da tool é a interface do agente
        path: str,
        subtitles_path: str,
        font_size: int = 24,
        position: SubtitlePosition = "bottom",
        text_color: str = "#FFFFFF",
        outline_color: str = "#000000",
        background: bool = False,
    ) -> BurnSubtitlesResult | JobSubmitted | ErrorPayload:
        """Grava as legendas de um .srt ou .ass direto na imagem do vídeo (legenda fixa).

        Fluxos: transcribe_audio -> create_subtitles (.srt) -> burn_subtitles para
        legenda comum; transcribe_audio -> create_dynamic_subtitles (.ass) ->
        burn_subtitles para legenda animada palavra por palavra. Com .srt/.vtt os
        parâmetros de estilo (font_size, position, cores) são aplicados; com .ass o
        estilo já vem do arquivo e esses parâmetros são ignorados. O original não
        é modificado; o vídeo é re-encodado. Para vídeos longos use background=true.

        Args:
            path: Vídeo, relativo ao workspace.
            subtitles_path: Arquivo .srt, .vtt ou .ass, relativo ao workspace.
            font_size: Tamanho da fonte (só .srt/.vtt).
            position: top, center ou bottom (só .srt/.vtt).
            text_color: Cor do texto em hexadecimal, ex: #FFFFFF (só .srt/.vtt).
            outline_color: Cor do contorno em hexadecimal, ex: #000000 (só .srt/.vtt).
            background: Executa como job e devolve job_id.
        """
        return burn_subtitles(
            runtime,
            path,
            subtitles_path,
            font_size=font_size,
            position=position,
            text_color=text_color,
            outline_color=outline_color,
            background=background,
        )
