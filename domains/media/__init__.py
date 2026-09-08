"""Tools de mídia da internet: vídeo de qualquer URL e efeitos sonoros do Freesound."""

from __future__ import annotations

from typing import TYPE_CHECKING

from domains.media import download, download_sfx, info, search_sfx

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Registra todas as tools do domínio."""
    info.register(mcp, runtime)
    download.register(mcp, runtime)
    search_sfx.register(mcp, runtime)
    download_sfx.register(mcp, runtime)
