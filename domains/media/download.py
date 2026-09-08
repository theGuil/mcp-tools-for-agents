"""Tool ``download_video``: baixa um vídeo de qualquer URL para o workspace."""

from __future__ import annotations

from typing import TYPE_CHECKING, TypedDict

from core.downloader import VideoQuality
from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

_DEFAULT_FOLDER = "downloads"


class DownloadVideoResult(TypedDict):
    """Arquivo baixado e seus metadados."""

    output: str
    title: str
    duration: float
    quality: VideoQuality
    size_bytes: int


def _do_download(
    runtime: Runtime, url: str, quality: VideoQuality, folder: str
) -> DownloadVideoResult:
    target_dir = runtime.workspace.resolve(folder)
    if target_dir.exists() and not target_dir.is_dir():
        raise ToolError(
            f"'{folder}' existe e não é uma pasta.",
            code="invalid_argument",
            hint="Informe outra pasta em folder.",
        )
    path = runtime.downloader.download(url, target_dir, quality)
    info = runtime.ffmpeg.probe(path)
    return DownloadVideoResult(
        output=runtime.workspace.relative(path),
        title=path.stem,
        duration=info["duration"],
        quality=quality,
        size_bytes=info["size_bytes"],
    )


@guarded
def download_video(
    runtime: Runtime,
    url: str,
    *,
    quality: VideoQuality = "720p",
    folder: str = _DEFAULT_FOLDER,
    background: bool = False,
) -> DownloadVideoResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "download_video", lambda: _do_download(runtime, url, quality, folder)
        )
    return _do_download(runtime, url, quality, folder)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="download_video")
    def _tool(
        url: str,
        quality: VideoQuality = "720p",
        folder: str = _DEFAULT_FOLDER,
        background: bool = True,
    ) -> DownloadVideoResult | JobSubmitted | ErrorPayload:
        """Baixa para o workspace um vídeo de qualquer site, em MP4.

        Use sempre que o usuário der uma URL e pedir o vídeo, seja de onde for:
        YouTube, Vimeo, Twitch, X/Twitter, TikTok, Instagram, Facebook, portais
        de notícia, plataformas de aula (eaulas.usp.br e afins) ou uma página
        comum com player embutido. Quando nenhum site é reconhecido, a tool lê o
        HTML da página e procura sozinha a fonte do vídeo, inclusive streams HLS
        (.m3u8) e DASH (.mpd). Passe a URL da página onde o vídeo aparece; só use
        a URL direta do arquivo se a página falhar.

        Não há como baixar conteúdo com DRM (Netflix, Disney+, cursos com
        Widevine) nem páginas que exigem login — nesses casos o hint do erro
        avisa para não insistir.

        Ponto de partida do fluxo de edição: depois do download use probe_video,
        extract_frame ou transcribe_audio para "assistir" e escolher os trechos,
        cut_video para cortar e as demais tools para legendar e estilizar.
        Downloads são lentos, então roda em background por padrão: acompanhe com
        job_status e pegue o caminho do arquivo em job_result.

        Args:
            url: Endereço da página onde o vídeo está, começando com https://.
            quality: best, 1080p, 720p, 480p, 360p ou audio (só o áudio, em m4a).
            folder: Pasta do workspace onde salvar. Criada se não existir.
            background: Executa como job e devolve job_id.
        """
        return download_video(runtime, url, quality=quality, folder=folder, background=background)
