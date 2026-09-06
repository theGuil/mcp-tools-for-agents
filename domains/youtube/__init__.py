"""Tools de YouTube: consulta de metadados e download para o workspace."""

from __future__ import annotations

from typing import TYPE_CHECKING

from domains.youtube import download, info

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Registra todas as tools do domínio."""
    info.register(mcp, runtime)
    download.register(mcp, runtime)
