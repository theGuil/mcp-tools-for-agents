"""Tool ``get_video_info``: lê título, duração, descrição e capítulos de uma URL sem baixar."""

from __future__ import annotations

from typing import TYPE_CHECKING, TypedDict

from core.downloader import Chapter
from core.errors import ErrorPayload, guarded

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


class VideoInfoResult(TypedDict):
    """Metadados do vídeo na origem."""

    url: str
    id: str
    title: str
    extractor: str
    channel: str | None
    duration: float
    view_count: int | None
    upload_date: str | None
    description: str
    thumbnail: str | None
    chapters: list[Chapter]


@guarded
def get_video_info(runtime: Runtime, url: str) -> VideoInfoResult:
    """Implementação pura, testável sem MCP."""
    data = runtime.downloader.info(url)
    return VideoInfoResult(
        url=url.strip(),
        id=data["id"],
        title=data["title"],
        extractor=data["extractor"],
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

    @mcp.tool(name="get_video_info")
    def _tool(url: str) -> VideoInfoResult | ErrorPayload:
        """Consulta título, duração, descrição e capítulos de um vídeo na internet, sem baixar.

        Aceita a URL de qualquer site, não só do YouTube. Use antes de
        download_video para confirmar que a página tem mesmo um vídeo e decidir
        a qualidade, sem gastar um download inteiro. O campo extractor diz quem
        reconheceu a página ("Youtube", "Vimeo", "Generic" quando o vídeo foi
        achado lendo o HTML). Os capítulos, quando existem, já indicam bons
        pontos de corte. Campos como channel e view_count vêm nulos em sites que
        não os publicam.

        Args:
            url: Endereço da página onde o vídeo está, começando com https://.
        """
        return get_video_info(runtime, url)
