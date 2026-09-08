"""Tools de áudio: extração, transcrição e normalização."""

from __future__ import annotations

from typing import TYPE_CHECKING

from domains.audio import extract, normalize, transcribe

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Registra todas as tools do domínio."""
    extract.register(mcp, runtime)
    transcribe.register(mcp, runtime)
    normalize.register(mcp, runtime)
