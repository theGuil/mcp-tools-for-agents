"""Monta o servidor MCP e registra os domínios ativos."""

from __future__ import annotations

import sys
from typing import TYPE_CHECKING

from mcp.server.mcpserver import MCPServer

from config import Settings
from core.downloader import Downloader
from core.errors import ToolError
from core.ffmpeg import FFmpeg
from core.jobs import JobManager
from core.paths import Workspace
from domains import Runtime, register_domains

if TYPE_CHECKING:
    from collections.abc import Mapping

_INSTRUCTIONS = (
    "Servidor de tools para edição de vídeo e áudio. Fluxo típico: "
    "download_video (qualquer URL) -> probe_video/extract_frame para assistir -> cut_video -> "
    "add_text_overlay/burn_subtitles/apply_template -> set_video_metadata. Todos os caminhos são "
    "relativos ao workspace. Comece com list_files e probe_video. Operações "
    "longas aceitam background=true e devolvem job_id; acompanhe com job_status "
    "e busque a saída com job_result. Erros vêm como {error, code, hint}."
)


def build_runtime(settings: Settings) -> Runtime:
    """Instancia as dependências compartilhadas a partir das configurações."""
    return Runtime(
        settings=settings,
        workspace=Workspace.at(settings.workspace_dir),
        ffmpeg=FFmpeg(
            ffmpeg_bin=settings.ffmpeg_bin,
            ffprobe_bin=settings.ffprobe_bin,
            timeout_seconds=settings.ffmpeg_timeout,
        ),
        jobs=JobManager(workers=settings.job_workers),
        downloader=Downloader(
            ffmpeg_bin=settings.ffmpeg_bin, timeout_seconds=settings.download_timeout
        ),
    )


def build_server(
    settings: Settings | None = None, *, runtime: Runtime | None = None
) -> MCPServer[None]:
    """Cria o servidor com os domínios ativos registrados."""
    settings = settings if settings is not None else Settings.from_env()
    runtime = runtime if runtime is not None else build_runtime(settings)
    mcp: MCPServer[None] = MCPServer(name=settings.server_name, instructions=_INSTRUCTIONS)
    register_domains(mcp, runtime, settings.domains)
    return mcp


def main(env: Mapping[str, str] | None = None) -> int:
    """Ponto de entrada: sobe o servidor via stdio."""
    try:
        settings = Settings.from_env(env)
    except ToolError as exc:
        print(f"Configuração inválida: {exc.message} {exc.hint or ''}".strip(), file=sys.stderr)  # noqa: T201
        return 2
    build_server(settings).run(transport="stdio")
    return 0


if __name__ == "__main__":
    sys.exit(main())
