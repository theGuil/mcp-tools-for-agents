"""Tools de vídeo: inspeção, corte, silêncio, cenas, frames, texto, legenda e efeitos."""

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
    remove_silence,
    scenes,
    sound_effects,
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
    remove_silence.register(mcp, runtime)
    frame.register(mcp, runtime)
    text_overlay.register(mcp, runtime)
    subtitles.register(mcp, runtime)
    burn_subtitles.register(mcp, runtime)
    metadata.register(mcp, runtime)
    narration.register(mcp, runtime)
    sound_effects.register(mcp, runtime)
    template.register(mcp, runtime)
    banner.register(mcp, runtime)
