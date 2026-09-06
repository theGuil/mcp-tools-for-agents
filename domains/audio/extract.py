"""Tool ``extract_audio``: separa a trilha de áudio de um vídeo."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final, Literal, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

type AudioFormat = Literal["mp3", "wav", "aac", "flac"]

_CODECS: Final[dict[AudioFormat, list[str]]] = {
    "mp3": ["-c:a", "libmp3lame", "-q:a", "2"],
    "wav": ["-c:a", "pcm_s16le"],
    "aac": ["-c:a", "aac"],
    "flac": ["-c:a", "flac"],
}


class ExtractAudioResult(TypedDict):
    """Arquivo de áudio gerado."""

    output: str
    format: AudioFormat
    duration: float


def _do_extract(runtime: Runtime, path: str, audio_format: AudioFormat) -> ExtractAudioResult:
    source = runtime.workspace.existing(path)
    info = runtime.ffmpeg.probe(source)
    if not info["has_audio"]:
        raise ToolError(
            f"'{path}' não possui trilha de áudio.",
            code="invalid_argument",
            hint="Confira com probe_video antes de extrair.",
        )
    output = runtime.workspace.output_for(source, "audio", extension=audio_format)
    runtime.ffmpeg.run(["-i", str(source), "-vn", *_CODECS[audio_format], str(output)])
    return ExtractAudioResult(
        output=runtime.workspace.relative(output),
        format=audio_format,
        duration=info["duration"],
    )


@guarded
def extract_audio(
    runtime: Runtime,
    path: str,
    *,
    audio_format: AudioFormat = "mp3",
    background: bool = False,
) -> ExtractAudioResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "extract_audio", lambda: _do_extract(runtime, path, audio_format)
        )
    return _do_extract(runtime, path, audio_format)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="extract_audio")
    def _tool(
        path: str, audio_format: AudioFormat = "mp3", background: bool = False
    ) -> ExtractAudioResult | JobSubmitted | ErrorPayload:
        """Extrai a trilha de áudio de um vídeo para um arquivo separado.

        Args:
            path: Vídeo, relativo ao workspace.
            audio_format: mp3, wav, aac ou flac.
            background: Executa como job e devolve job_id.
        """
        return extract_audio(runtime, path, audio_format=audio_format, background=background)
