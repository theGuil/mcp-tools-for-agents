"""Tools de arquivos do workspace."""

from __future__ import annotations

from typing import TYPE_CHECKING

from domains.files import delete, listing

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Registra todas as tools do domínio."""
    listing.register(mcp, runtime)
    delete.register(mcp, runtime)
