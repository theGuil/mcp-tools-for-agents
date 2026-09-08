"""Tools de vídeo: inspeção, corte, ritmo, legenda, som, enquadramento e exportação."""

from __future__ import annotations

from typing import TYPE_CHECKING

from domains.video import (
    background_music,
    banner,
    burn_subtitles,
    concat,
    cut,
    dynamic_subtitles,
    export,
    fade,
    frame,
    metadata,
    narration,
    probe,
    remove_silence,
    scenes,
    smart_crop,
    sound_effects,
    speed,
    subtitles,
    template,
    text_overlay,
    thumbnail,
    zoom,
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
    background_music.register(mcp, runtime)
    fade.register(mcp, runtime)
    speed.register(mcp, runtime)
    zoom.register(mcp, runtime)
    dynamic_subtitles.register(mcp, runtime)
    thumbnail.register(mcp, runtime)
    smart_crop.register(mcp, runtime)
    export.register(mcp, runtime)
