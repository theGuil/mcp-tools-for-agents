"""Tool ``download_youtube_video``: baixa um vídeo do YouTube para o workspace."""

from __future__ import annotations

from typing import TYPE_CHECKING, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted
from core.youtube import VideoQuality

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

_DEFAULT_FOLDER = "downloads"


class DownloadYouTubeResult(TypedDict):
    """Arquivo baixado e seus metadados."""

    output: str
    title: str
    duration: float
    quality: VideoQuality
    size_bytes: int


def _do_download(
    runtime: Runtime, url: str, quality: VideoQuality, folder: str
) -> DownloadYouTubeResult:
    target_dir = runtime.workspace.resolve(folder)
    if target_dir.exists() and not target_dir.is_dir():
        raise ToolError(
            f"'{folder}' existe e não é uma pasta.",
            code="invalid_argument",
            hint="Informe outra pasta em folder.",
        )
    path = runtime.youtube.download(url, target_dir, quality)
    info = runtime.ffmpeg.probe(path)
    return DownloadYouTubeResult(
        output=runtime.workspace.relative(path),
        title=path.stem,
        duration=info["duration"],
        quality=quality,
        size_bytes=info["size_bytes"],
    )


@guarded
def download_youtube_video(
    runtime: Runtime,
    url: str,
    *,
    quality: VideoQuality = "720p",
    folder: str = _DEFAULT_FOLDER,
    background: bool = False,
) -> DownloadYouTubeResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "download_youtube_video", lambda: _do_download(runtime, url, quality, folder)
        )
    return _do_download(runtime, url, quality, folder)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="download_youtube_video")
    def _tool(
        url: str,
        quality: VideoQuality = "720p",
        folder: str = _DEFAULT_FOLDER,
        background: bool = True,
    ) -> DownloadYouTubeResult | JobSubmitted | ErrorPayload:
        """Baixa um vídeo do YouTube para dentro do workspace, em MP4.

        Ponto de partida do fluxo de edição: depois do download use probe_video,
        extract_frame ou transcribe_audio para "assistir" e escolher os trechos,
        cut_video para cortar e as demais tools para legendar e estilizar.
        Downloads são lentos, então roda em background por padrão: acompanhe com
        job_status e pegue o caminho do arquivo em job_result.

        Args:
            url: Link do vídeo (youtube.com/watch?v=... ou youtu.be/...).
            quality: best, 1080p, 720p, 480p, 360p ou audio (só o áudio, em m4a).
            folder: Pasta do workspace onde salvar. Criada se não existir.
            background: Executa como job e devolve job_id.
        """
        return download_youtube_video(
            runtime, url, quality=quality, folder=folder, background=background
        )
