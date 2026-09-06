"""Tools para acompanhar tarefas em background."""

from __future__ import annotations

from typing import TYPE_CHECKING

from domains.jobs import result, status

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Registra todas as tools do domínio."""
    status.register(mcp, runtime)
    result.register(mcp, runtime)
