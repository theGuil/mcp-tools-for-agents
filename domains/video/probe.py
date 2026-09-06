"""Tool ``probe_video``: lê metadados de um vídeo sem alterá-lo."""

from __future__ import annotations

from typing import TYPE_CHECKING, TypedDict

from core.errors import ErrorPayload, guarded
from core.ffmpeg import ProbeResult

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


class ProbeVideoResult(TypedDict):
    """Metadados do vídeo mais o caminho consultado."""

    path: str
    info: ProbeResult


@guarded
def probe_video(runtime: Runtime, path: str) -> ProbeVideoResult:
    """Implementação pura, testável sem MCP."""
    source = runtime.workspace.existing(path)
    return ProbeVideoResult(
        path=runtime.workspace.relative(source), info=runtime.ffmpeg.probe(source)
    )


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="probe_video")
    def _tool(path: str) -> ProbeVideoResult | ErrorPayload:
        """Lê duração, resolução, fps, codecs e tamanho de um vídeo.

        Use antes de cortar ou concatenar para conhecer o arquivo. Não altera nada.

        Args:
            path: Caminho do vídeo, relativo ao workspace.
        """
        return probe_video(runtime, path)
