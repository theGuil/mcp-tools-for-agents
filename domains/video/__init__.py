"""Tools de vídeo: inspeção, corte, concatenação, cenas e frames."""

from __future__ import annotations

from typing import TYPE_CHECKING

from domains.video import concat, cut, frame, probe, scenes

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Registra todas as tools do domínio."""
    probe.register(mcp, runtime)
    cut.register(mcp, runtime)
    concat.register(mcp, runtime)
    scenes.register(mcp, runtime)
    frame.register(mcp, runtime)
