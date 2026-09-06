"""Wrapper tipado para ``ffmpeg`` e ``ffprobe``.

Toda operação de vídeo e áudio passa por aqui. O restante do código nunca
monta linha de comando nem interpreta saída bruta.
"""

from __future__ import annotations

import json
import shutil
import subprocess
from dataclasses import dataclass
from fractions import Fraction
from typing import TYPE_CHECKING, Final, TypedDict

from core.errors import ToolError

if TYPE_CHECKING:
    from collections.abc import Sequence
    from pathlib import Path

_STDERR_TAIL: Final = 1500


class ProbeResult(TypedDict):
    """Metadados essenciais de um arquivo de mídia."""

    duration: float
    width: int | None
    height: int | None
    fps: float | None
    video_codec: str | None
    audio_codec: str | None
    has_video: bool
    has_audio: bool
    size_bytes: int
    format_name: str


@dataclass(frozen=True, slots=True)
class FFmpeg:
    """Executa binários do FFmpeg com timeout e erros amigáveis."""

    ffmpeg_bin: str = "ffmpeg"
    ffprobe_bin: str = "ffprobe"
    timeout_seconds: float = 600.0

    def is_available(self) -> bool:
        """Indica se os dois binários estão no PATH."""
        return (
            shutil.which(self.ffmpeg_bin) is not None and shutil.which(self.ffprobe_bin) is not None
        )

    def require(self) -> None:
        """Falha com mensagem clara se o FFmpeg não estiver instalado.

        Raises:
            ToolError: Se ffmpeg ou ffprobe não forem encontrados.
        """
        if not self.is_available():
            raise ToolError(
                "ffmpeg/ffprobe não encontrados no PATH.",
                code="unavailable",
                hint="Instale o FFmpeg ou ajuste FFMPEG_BIN e FFPROBE_BIN.",
            )

    def run(self, args: Sequence[str]) -> str:
        """Roda ``ffmpeg`` com os argumentos dados e devolve o stderr.

        O stderr é onde o FFmpeg escreve progresso e informações de filtros.

        Raises:
            ToolError: Se o processo falhar ou exceder o timeout.
        """
        self.require()
        cmd = [self.ffmpeg_bin, "-hide_banner", "-nostdin", "-y", *args]
        try:
            proc = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=self.timeout_seconds,
                check=False,
            )
        except subprocess.TimeoutExpired as exc:
            raise ToolError(
                f"ffmpeg excedeu o timeout de {self.timeout_seconds:.0f}s.",
                code="timeout",
                hint="Use background=True para operações longas ou aumente FFMPEG_TIMEOUT.",
            ) from exc
        if proc.returncode != 0:
            raise ToolError(
                f"ffmpeg falhou (código {proc.returncode}): {proc.stderr[-_STDERR_TAIL:].strip()}",
                code="ffmpeg_failed",
                hint="Verifique se o arquivo de entrada é válido com probe_video.",
            )
        return proc.stderr

    def probe(self, path: Path) -> ProbeResult:
        """Lê metadados de um arquivo de mídia via ``ffprobe``.

        Raises:
            ToolError: Se o ffprobe falhar ou a saída não puder ser interpretada.
        """
        self.require()
        cmd = [
            self.ffprobe_bin,
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            str(path),
        ]
        try:
            proc = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=self.timeout_seconds,
                check=False,
            )
        except subprocess.TimeoutExpired as exc:
            raise ToolError("ffprobe excedeu o timeout.", code="timeout") from exc
        if proc.returncode != 0:
            raise ToolError(
                f"ffprobe falhou: {proc.stderr[-_STDERR_TAIL:].strip()}",
                code="ffmpeg_failed",
                hint="O arquivo pode estar corrompido ou não ser um vídeo/áudio.",
            )
        return _parse_probe(proc.stdout, path)


def _parse_probe(raw: str, path: Path) -> ProbeResult:
    try:
        data: dict[str, object] = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ToolError("Saída do ffprobe não é JSON válido.", code="ffmpeg_failed") from exc

    fmt = data.get("format")
    streams = data.get("streams")
    if not isinstance(fmt, dict) or not isinstance(streams, list):
        raise ToolError("Saída do ffprobe sem 'format' ou 'streams'.", code="ffmpeg_failed")

    video = next(
        (s for s in streams if isinstance(s, dict) and s.get("codec_type") == "video"), None
    )
    audio = next(
        (s for s in streams if isinstance(s, dict) and s.get("codec_type") == "audio"), None
    )

    return ProbeResult(
        duration=_as_float(fmt.get("duration")) or 0.0,
        width=_as_int(video.get("width")) if video else None,
        height=_as_int(video.get("height")) if video else None,
        fps=_as_fps(video.get("avg_frame_rate")) if video else None,
        video_codec=_as_str(video.get("codec_name")) if video else None,
        audio_codec=_as_str(audio.get("codec_name")) if audio else None,
        has_video=video is not None,
        has_audio=audio is not None,
        size_bytes=_as_int(fmt.get("size")) or path.stat().st_size,
        format_name=_as_str(fmt.get("format_name")) or "",
    )


def _as_float(value: object) -> float | None:
    try:
        return float(value) if isinstance(value, (str, int, float)) else None
    except ValueError:
        return None


def _as_int(value: object) -> int | None:
    try:
        return int(value) if isinstance(value, (str, int)) else None
    except ValueError:
        return None


def _as_str(value: object) -> str | None:
    return value if isinstance(value, str) else None


def _as_fps(value: object) -> float | None:
    if not isinstance(value, str) or value in {"", "0/0"}:
        return None
    try:
        return round(float(Fraction(value)), 3)
    except ValueError, ZeroDivisionError:
        return None
