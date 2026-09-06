"""Tool ``get_youtube_info``: lê título, duração, descrição e capítulos sem baixar."""

from __future__ import annotations

from typing import TYPE_CHECKING, TypedDict

from core.errors import ErrorPayload, guarded
from core.youtube import Chapter

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


class YouTubeInfoResult(TypedDict):
    """Metadados do vídeo no YouTube."""

    url: str
    id: str
    title: str
    channel: str | None
    duration: float
    view_count: int | None
    upload_date: str | None
    description: str
    thumbnail: str | None
    chapters: list[Chapter]


@guarded
def get_youtube_info(runtime: Runtime, url: str) -> YouTubeInfoResult:
    """Implementação pura, testável sem MCP."""
    data = runtime.youtube.info(url)
    return YouTubeInfoResult(
        url=url.strip(),
        id=data["id"],
        title=data["title"],
        channel=data["channel"],
        duration=data["duration"],
        view_count=data["view_count"],
        upload_date=data["upload_date"],
        description=data["description"],
        thumbnail=data["thumbnail"],
        chapters=data["chapters"],
    )


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="get_youtube_info")
    def _tool(url: str) -> YouTubeInfoResult | ErrorPayload:
        """Consulta título, duração, descrição e capítulos de um vídeo do YouTube sem baixar.

        Use antes de download_youtube_video para decidir se vale baixar e em que
        qualidade. Os capítulos, quando existem, já indicam bons pontos de corte.

        Args:
            url: Link do vídeo (youtube.com/watch?v=... ou youtu.be/...).
        """
        return get_youtube_info(runtime, url)
