"""Tools de áudio: extração e transcrição."""

from __future__ import annotations

from typing import TYPE_CHECKING

from domains.audio import extract, transcribe

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Registra todas as tools do domínio."""
    extract.register(mcp, runtime)
    transcribe.register(mcp, runtime)
