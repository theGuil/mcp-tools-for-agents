"""Wrapper tipado para o ``yt-dlp``.

Único ponto do projeto que conversa com o YouTube. O restante recebe apenas
metadados tipados e o caminho do arquivo baixado.
"""

from __future__ import annotations

import importlib
import re
import shutil
from dataclasses import dataclass
from pathlib import Path
from typing import Final, Literal, Protocol, Self, TypedDict, cast

from core.errors import ToolError

type VideoQuality = Literal["best", "1080p", "720p", "480p", "360p", "audio"]

_YOUTUBE_HOSTS: Final = re.compile(
    r"^https?://(www\.|m\.|music\.)?(youtube\.com|youtu\.be|youtube-nocookie\.com)/", re.IGNORECASE
)
_SAFE_TITLE: Final = re.compile(r"[^A-Za-z0-9._-]+")
_MAX_TITLE: Final = 60

_FORMATS: Final[dict[VideoQuality, str]] = {
    "best": "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best",
    "1080p": "bestvideo[height<=1080][ext=mp4]+bestaudio[ext=m4a]/best[height<=1080]/best",
    "720p": "bestvideo[height<=720][ext=mp4]+bestaudio[ext=m4a]/best[height<=720]/best",
    "480p": "bestvideo[height<=480][ext=mp4]+bestaudio[ext=m4a]/best[height<=480]/best",
    "360p": "bestvideo[height<=360][ext=mp4]+bestaudio[ext=m4a]/best[height<=360]/best",
    "audio": "bestaudio[ext=m4a]/bestaudio",
}


class VideoInfo(TypedDict):
    """Metadados de um vídeo do YouTube antes do download."""

    id: str
    title: str
    channel: str | None
    duration: float
    view_count: int | None
    upload_date: str | None
    description: str
    thumbnail: str | None
    chapters: list[Chapter]


class Chapter(TypedDict):
    """Capítulo declarado pelo autor do vídeo."""

    title: str
    start: float
    end: float


class _YtdlInstance(Protocol):
    def __enter__(self) -> Self: ...
    def __exit__(self, *args: object) -> None: ...
    def extract_info(self, url: str, download: bool) -> dict[str, object] | None: ...
    def prepare_filename(self, info: dict[str, object]) -> str: ...


class _YtdlModule(Protocol):
    def YoutubeDL(self, params: dict[str, object]) -> _YtdlInstance: ...  # noqa: N802

    class utils:  # noqa: N801
        class DownloadError(Exception): ...


def _load_ytdlp() -> _YtdlModule:
    try:
        return cast("_YtdlModule", importlib.import_module("yt_dlp"))
    except ModuleNotFoundError as exc:
        raise ToolError(
            "Download indisponível: yt-dlp não instalado.",
            code="unavailable",
            hint="Instale com: uv sync",
        ) from exc


def validate_url(url: str) -> str:
    """Garante que a URL é do YouTube.

    Raises:
        ToolError: Se a URL não for reconhecida.
    """
    cleaned = url.strip()
    if not _YOUTUBE_HOSTS.match(cleaned):
        raise ToolError(
            f"URL '{url}' não é um link do YouTube.",
            code="invalid_argument",
            hint="Use um link como https://www.youtube.com/watch?v=ID ou https://youtu.be/ID.",
        )
    return cleaned


def safe_title(title: str) -> str:
    """Converte o título do vídeo em um nome de arquivo seguro."""
    cleaned = _SAFE_TITLE.sub("_", title).strip("_")[:_MAX_TITLE].strip("_")
    return cleaned or "video"


@dataclass(frozen=True, slots=True)
class YouTube:
    """Consulta e baixa vídeos do YouTube com erros amigáveis."""

    ffmpeg_bin: str = "ffmpeg"
    timeout_seconds: float = 900.0

    def _base_params(self) -> dict[str, object]:
        params: dict[str, object] = {
            "quiet": True,
            "no_warnings": True,
            "noprogress": True,
            "socket_timeout": min(self.timeout_seconds, 60.0),
            "noplaylist": True,
        }
        # O yt-dlp exige um caminho real em ffmpeg_location. Um nome nu como
        # "ffmpeg" é tratado como inexistente e o merge de vídeo+áudio falha.
        ffmpeg = shutil.which(self.ffmpeg_bin)
        if ffmpeg is not None:
            params["ffmpeg_location"] = ffmpeg
        return params

    def info(self, url: str) -> VideoInfo:
        """Lê metadados do vídeo sem baixar.

        Raises:
            ToolError: Se o vídeo não puder ser acessado.
        """
        ytdl = _load_ytdlp()
        params = self._base_params()
        params["skip_download"] = True
        data = self._extract(ytdl, params, validate_url(url), download=False)
        return parse_info(data)

    def download(self, url: str, target_dir: Path, quality: VideoQuality = "720p") -> Path:
        """Baixa o vídeo para ``target_dir`` e devolve o caminho do arquivo final.

        Raises:
            ToolError: Se o download falhar.
        """
        ytdl = _load_ytdlp()
        target_dir.mkdir(parents=True, exist_ok=True)
        params = self._base_params()
        params["format"] = _FORMATS[quality]
        params["outtmpl"] = str(target_dir / "%(title).60B_%(id)s.%(ext)s")
        params["restrictfilenames"] = True
        if quality == "audio":
            params["postprocessors"] = [
                {"key": "FFmpegExtractAudio", "preferredcodec": "m4a"},
            ]
        else:
            params["merge_output_format"] = "mp4"
        data = self._extract(ytdl, params, validate_url(url), download=True)
        return _final_path(data, target_dir, quality)

    @staticmethod
    def _extract(
        ytdl: _YtdlModule, params: dict[str, object], url: str, *, download: bool
    ) -> dict[str, object]:
        try:
            with ytdl.YoutubeDL(params) as instance:
                data = instance.extract_info(url, download=download)
        except ytdl.utils.DownloadError as exc:
            raise ToolError(
                f"yt-dlp falhou: {_clean_error(str(exc))}",
                code="download_failed",
                hint="Confira se o vídeo é público e o link está correto. "
                "Vídeos privados, removidos ou com restrição de idade não podem ser baixados.",
            ) from exc
        if data is None:
            raise ToolError(
                "yt-dlp não devolveu informações do vídeo.",
                code="download_failed",
                hint="Tente novamente ou use outro link.",
            )
        return data


def _clean_error(raw: str) -> str:
    return raw.replace("ERROR: ", "").strip()[-500:]


def _final_path(data: dict[str, object], target_dir: Path, quality: VideoQuality) -> Path:
    downloads = data.get("requested_downloads")
    if isinstance(downloads, list) and downloads:
        first = downloads[0]
        if isinstance(first, dict):
            filepath = first.get("filepath")
            if isinstance(filepath, str):
                return Path(filepath)
    ext = "m4a" if quality == "audio" else "mp4"
    video_id = data.get("id")
    candidates = sorted(target_dir.glob(f"*_{video_id}.{ext}")) if video_id else []
    if candidates:
        return candidates[0]
    raise ToolError(
        "Download terminou mas o arquivo final não foi encontrado.",
        code="download_failed",
        hint="Use list_files para procurar o arquivo baixado.",
    )


def parse_info(data: dict[str, object]) -> VideoInfo:
    """Converte o dicionário bruto do yt-dlp em ``VideoInfo``."""
    raw_chapters = data.get("chapters")
    chapters: list[Chapter] = []
    if isinstance(raw_chapters, list):
        for item in raw_chapters:
            if not isinstance(item, dict):
                continue
            chapters.append(
                Chapter(
                    title=str(item.get("title") or ""),
                    start=float(_num(item.get("start_time"))),
                    end=float(_num(item.get("end_time"))),
                )
            )
    return VideoInfo(
        id=str(data.get("id") or ""),
        title=str(data.get("title") or "video"),
        channel=_opt_str(data.get("channel") or data.get("uploader")),
        duration=float(_num(data.get("duration"))),
        view_count=_opt_int(data.get("view_count")),
        upload_date=_opt_str(data.get("upload_date")),
        description=str(data.get("description") or ""),
        thumbnail=_opt_str(data.get("thumbnail")),
        chapters=chapters,
    )


def _num(value: object) -> float:
    return float(value) if isinstance(value, (int, float)) else 0.0


def _opt_str(value: object) -> str | None:
    return value if isinstance(value, str) and value else None


def _opt_int(value: object) -> int | None:
    return value if isinstance(value, int) else None
