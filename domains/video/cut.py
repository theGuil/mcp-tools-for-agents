"""Tool ``cut_video``: recorta um trecho de um vídeo."""

from __future__ import annotations

from typing import TYPE_CHECKING, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


class CutVideoResult(TypedDict):
    """Arquivo gerado pelo corte."""

    output: str
    start: float
    end: float
    duration: float
    reencoded: bool


def _do_cut(
    runtime: Runtime, path: str, start: float, end: float, *, reencode: bool
) -> CutVideoResult:
    source = runtime.workspace.existing(path)
    if start < 0 or end <= start:
        raise ToolError(
            f"Intervalo inválido: start={start}, end={end}.",
            code="invalid_argument",
            hint="start deve ser >= 0 e end maior que start, em segundos.",
        )
    info = runtime.ffmpeg.probe(source)
    if end > info["duration"] + 0.05:
        raise ToolError(
            f"end={end}s ultrapassa a duração do vídeo ({info['duration']:.2f}s).",
            code="invalid_argument",
            hint="Use probe_video para conferir a duração.",
        )
    output = runtime.workspace.output_for(source, f"cut_{start:g}-{end:g}")
    codec_args = ["-c:v", "libx264", "-c:a", "aac"] if reencode else ["-c", "copy"]
    runtime.ffmpeg.run(
        ["-ss", f"{start}", "-to", f"{end}", "-i", str(source), *codec_args, str(output)]
    )
    return CutVideoResult(
        output=runtime.workspace.relative(output),
        start=start,
        end=end,
        duration=round(end - start, 3),
        reencoded=reencode,
    )


@guarded
def cut_video(
    runtime: Runtime,
    path: str,
    start: float,
    end: float,
    *,
    reencode: bool = False,
    background: bool = False,
) -> CutVideoResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "cut_video", lambda: _do_cut(runtime, path, start, end, reencode=reencode)
        )
    return _do_cut(runtime, path, start, end, reencode=reencode)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="cut_video")
    def _tool(
        path: str,
        start: float,
        end: float,
        reencode: bool = False,
        background: bool = False,
    ) -> CutVideoResult | JobSubmitted | ErrorPayload:
        """Corta o trecho entre start e end (segundos) e salva um novo arquivo.

        O original não é modificado. Por padrão copia os streams sem re-encodar,
        o que é rápido mas corta no keyframe mais próximo. Use reencode=true para
        corte exato no frame. Para vídeos longos use background=true e acompanhe
        com job_status.

        Args:
            path: Vídeo de origem, relativo ao workspace.
            start: Início do trecho em segundos.
            end: Fim do trecho em segundos.
            reencode: Re-encoda para corte exato (mais lento).
            background: Executa como job e devolve job_id.
        """
        return cut_video(runtime, path, start, end, reencode=reencode, background=background)
