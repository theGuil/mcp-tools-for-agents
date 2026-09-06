"""Tools de vídeo: inspeção, corte, concatenação, cenas, frames, texto, legenda e templates."""

from __future__ import annotations

from typing import TYPE_CHECKING

from domains.video import (
    banner,
    burn_subtitles,
    concat,
    cut,
    frame,
    metadata,
    narration,
    probe,
    scenes,
    subtitles,
    template,
    text_overlay,
)

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
    text_overlay.register(mcp, runtime)
    subtitles.register(mcp, runtime)
    burn_subtitles.register(mcp, runtime)
    metadata.register(mcp, runtime)
    narration.register(mcp, runtime)
    template.register(mcp, runtime)
    banner.register(mcp, runtime)
