"""Tool ``burn_subtitles``: grava legendas de um .srt na imagem do vídeo."""

from __future__ import annotations

from typing import TYPE_CHECKING, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.fonts import escape_filter_path, find_font
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


class BurnSubtitlesResult(TypedDict):
    """Vídeo gerado com as legendas."""

    output: str
    subtitles: str
    font_size: int


def _do_burn(
    runtime: Runtime, path: str, subtitles_path: str, *, font_size: int
) -> BurnSubtitlesResult:
    source = runtime.workspace.existing(path)
    subs = runtime.workspace.existing(subtitles_path)
    if subs.suffix.lower() not in {".srt", ".vtt", ".ass"}:
        raise ToolError(
            f"'{subtitles_path}' não é um arquivo de legenda (.srt, .vtt ou .ass).",
            code="invalid_argument",
            hint="Gere um .srt com create_subtitles.",
        )
    if font_size <= 0:
        raise ToolError("font_size deve ser maior que zero.", code="invalid_argument")
    info = runtime.ffmpeg.probe(source)
    style = f"FontSize={font_size},Outline=2,Shadow=0,MarginV=30"
    font = find_font()
    filter_expr = f"subtitles='{escape_filter_path(subs)}':force_style='{style}'"
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
    )


@guarded
def burn_subtitles(
    runtime: Runtime,
    path: str,
    subtitles_path: str,
    *,
    font_size: int = 24,
    background: bool = False,
) -> BurnSubtitlesResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "burn_subtitles",
            lambda: _do_burn(runtime, path, subtitles_path, font_size=font_size),
        )
    return _do_burn(runtime, path, subtitles_path, font_size=font_size)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="burn_subtitles")
    def _tool(
        path: str,
        subtitles_path: str,
        font_size: int = 24,
        background: bool = False,
    ) -> BurnSubtitlesResult | JobSubmitted | ErrorPayload:
        """Grava as legendas de um arquivo .srt direto na imagem do vídeo (legenda fixa).

        Fluxo: transcribe_audio -> create_subtitles -> burn_subtitles. O original não
        é modificado; o vídeo é re-encodado. Para vídeos longos use background=true.

        Args:
            path: Vídeo, relativo ao workspace.
            subtitles_path: Arquivo .srt (ou .vtt/.ass), relativo ao workspace.
            font_size: Tamanho da fonte da legenda.
            background: Executa como job e devolve job_id.
        """
        return burn_subtitles(
            runtime, path, subtitles_path, font_size=font_size, background=background
        )
