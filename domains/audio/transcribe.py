"""Tool ``transcribe_audio``: transcreve fala com timestamps.

Depende do extra opcional ``transcribe`` (faster-whisper). Sem ele a tool
existe, mas devolve um erro explicando como habilitar.
"""

from __future__ import annotations

import importlib
from typing import TYPE_CHECKING, Protocol, TypedDict, cast

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from collections.abc import Iterable

    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


class _Word(Protocol):
    start: float
    end: float
    word: str


class _Segment(Protocol):
    start: float
    end: float
    text: str
    words: Iterable[_Word] | None


class _WhisperModel(Protocol):
    def transcribe(
        self, audio: str, *, language: str | None, word_timestamps: bool
    ) -> tuple[Iterable[_Segment], object]: ...


class _WhisperModule(Protocol):
    def WhisperModel(self, model_size: str, *, device: str, compute_type: str) -> _WhisperModel: ...  # noqa: N802


class TranscriptWord(TypedDict):
    """Uma palavra com seus tempos, para legendas dinâmicas."""

    start: float
    end: float
    word: str


class TranscriptSegment(TypedDict):
    """Um trecho de fala com seus tempos."""

    start: float
    end: float
    text: str
    words: list[TranscriptWord]


class TranscribeResult(TypedDict):
    """Transcrição completa."""

    path: str
    model: str
    language: str | None
    segments: list[TranscriptSegment]
    text: str


def _load_whisper() -> _WhisperModule:
    try:
        return cast("_WhisperModule", importlib.import_module("faster_whisper"))
    except ModuleNotFoundError as exc:
        raise ToolError(
            "Transcrição indisponível: faster-whisper não instalado.",
            code="unavailable",
            hint="Instale com: uv sync --extra transcribe",
        ) from exc


def _words_of(segment: _Segment) -> list[TranscriptWord]:
    if segment.words is None:
        return []
    return [
        TranscriptWord(start=round(w.start, 3), end=round(w.end, 3), word=w.word.strip())
        for w in segment.words
        if w.word.strip()
    ]


def _do_transcribe(
    runtime: Runtime,
    path: str,
    model_size: str,
    language: str | None,
    *,
    word_timestamps: bool,
) -> TranscribeResult:
    source = runtime.workspace.existing(path)
    whisper = _load_whisper()
    model = whisper.WhisperModel(model_size, device="cpu", compute_type="int8")
    raw_segments, _ = model.transcribe(
        str(source), language=language, word_timestamps=word_timestamps
    )
    segments = [
        TranscriptSegment(
            start=round(s.start, 3),
            end=round(s.end, 3),
            text=s.text.strip(),
            words=_words_of(s) if word_timestamps else [],
        )
        for s in raw_segments
    ]
    return TranscribeResult(
        path=runtime.workspace.relative(source),
        model=model_size,
        language=language,
        segments=segments,
        text=" ".join(s["text"] for s in segments),
    )


@guarded
def transcribe_audio(
    runtime: Runtime,
    path: str,
    *,
    model_size: str = "base",
    language: str | None = None,
    word_timestamps: bool = True,
    background: bool = False,
) -> TranscribeResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "transcribe_audio",
            lambda: _do_transcribe(
                runtime, path, model_size, language, word_timestamps=word_timestamps
            ),
        )
    return _do_transcribe(runtime, path, model_size, language, word_timestamps=word_timestamps)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="transcribe_audio")
    def _tool(
        path: str,
        model_size: str = "base",
        language: str | None = None,
        word_timestamps: bool = True,
        background: bool = True,
    ) -> TranscribeResult | JobSubmitted | ErrorPayload:
        """Transcreve a fala de um áudio ou vídeo, com tempos de cada trecho e de cada palavra.

        Combine com cut_video para cortar por conteúdo falado, com create_subtitles
        para legenda comum e com create_dynamic_subtitles para legenda animada
        palavra por palavra (estilo TikTok). Cada segment traz words, uma lista
        {start, end, word}, quando word_timestamps=true. É lento, por isso roda em
        background por padrão: pegue o resultado com job_result.

        Args:
            path: Áudio ou vídeo, relativo ao workspace.
            model_size: Modelo whisper: tiny, base, small, medium ou large-v3.
            language: Código do idioma (pt, en). Detecta automaticamente se omitido.
            word_timestamps: Inclui o tempo de cada palavra dentro de cada segment.
            background: Executa como job e devolve job_id.
        """
        return transcribe_audio(
            runtime,
            path,
            model_size=model_size,
            language=language,
            word_timestamps=word_timestamps,
            background=background,
        )
