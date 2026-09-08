"""Tool ``remove_silence``: corta os trechos em que ninguém fala."""

from __future__ import annotations

import re
import tempfile
from pathlib import Path
from typing import TYPE_CHECKING, Final, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

_SILENCE_START: Final = re.compile(r"silence_start:\s*(?P<t>-?[0-9]+(?:\.[0-9]+)?)")
_SILENCE_END: Final = re.compile(r"silence_end:\s*(?P<t>-?[0-9]+(?:\.[0-9]+)?)")

_DEFAULT_THRESHOLD_DB: Final = -30.0
_DEFAULT_MIN_SILENCE: Final = 0.5
_DEFAULT_MARGIN: Final = 0.2
_MIN_SEGMENT: Final = 0.05


class Segment(TypedDict):
    """Trecho com fala mantido no resultado."""

    start: float
    end: float
    duration: float


class RemoveSilenceResult(TypedDict):
    """Arquivo gerado sem os silêncios."""

    output: str
    threshold_db: float
    min_silence: float
    margin: float
    original_duration: float
    duration: float
    removed_duration: float
    silences_removed: int
    segments: list[Segment]


def _validate(threshold_db: float, min_silence: float, margin: float) -> None:
    if threshold_db >= 0:
        raise ToolError(
            f"threshold_db={threshold_db} deve ser negativo.",
            code="invalid_argument",
            hint="Use valores em dBFS como -30 (padrão) ou -40 para ambientes silenciosos.",
        )
    if min_silence <= 0:
        raise ToolError(
            f"min_silence={min_silence} deve ser maior que zero.",
            code="invalid_argument",
            hint="Valores típicos: 0.3 a 1.0 segundos.",
        )
    if margin < 0:
        raise ToolError(
            f"margin={margin} não pode ser negativa.",
            code="invalid_argument",
            hint="Use 0 para nenhuma margem ou algo como 0.2 segundos.",
        )


def _parse_silences(stderr: str, duration: float) -> list[tuple[float, float]]:
    """Extrai os intervalos de silêncio reportados pelo ``silencedetect``."""
    starts = [float(m.group("t")) for m in _SILENCE_START.finditer(stderr)]
    ends = [float(m.group("t")) for m in _SILENCE_END.finditer(stderr)]
    if len(ends) < len(starts):
        ends.append(duration)
    return [(max(s, 0.0), min(e, duration)) for s, e in zip(starts, ends, strict=True) if e > s]


def _speech_segments(
    silences: list[tuple[float, float]], duration: float, margin: float
) -> list[Segment]:
    """Inverte os silêncios, aplica a margem e funde trechos que se sobrepõem."""
    raw: list[tuple[float, float]] = []
    cursor = 0.0
    for s_start, s_end in silences:
        if s_start > cursor:
            raw.append((cursor, s_start))
        cursor = max(cursor, s_end)
    if cursor < duration:
        raw.append((cursor, duration))

    merged: list[list[float]] = []
    for start, end in raw:
        a, b = max(start - margin, 0.0), min(end + margin, duration)
        if merged and a <= merged[-1][1]:
            merged[-1][1] = max(merged[-1][1], b)
        else:
            merged.append([a, b])

    return [
        Segment(start=round(a, 3), end=round(b, 3), duration=round(b - a, 3))
        for a, b in merged
        if b - a >= _MIN_SEGMENT
    ]


def _filter_script(segments: list[Segment], *, has_video: bool) -> str:
    """Monta o filter_complex que recorta e concatena os trechos com fala."""
    lines: list[str] = []
    labels: list[str] = []
    for i, seg in enumerate(segments):
        start, end = seg["start"], seg["end"]
        if has_video:
            lines.append(f"[0:v]trim=start={start}:end={end},setpts=PTS-STARTPTS[v{i}];")
            labels.append(f"[v{i}]")
        lines.append(f"[0:a]atrim=start={start}:end={end},asetpts=PTS-STARTPTS[a{i}];")
        labels.append(f"[a{i}]")
    outputs = "[v][a]" if has_video else "[a]"
    lines.append(f"{''.join(labels)}concat=n={len(segments)}:v={int(has_video)}:a=1{outputs}")
    return "\n".join(lines)


def _do_remove(
    runtime: Runtime,
    path: str,
    *,
    threshold_db: float,
    min_silence: float,
    margin: float,
) -> RemoveSilenceResult:
    _validate(threshold_db, min_silence, margin)
    source = runtime.workspace.existing(path)
    info = runtime.ffmpeg.probe(source)
    if not info["has_audio"]:
        raise ToolError(
            "O arquivo não tem trilha de áudio, então não há silêncio a detectar.",
            code="invalid_argument",
            hint="Use probe_video para conferir os streams do arquivo.",
        )
    duration = info["duration"]

    stderr = runtime.ffmpeg.run(
        [
            "-i",
            str(source),
            "-af",
            f"silencedetect=noise={threshold_db}dB:d={min_silence}",
            "-vn",
            "-f",
            "null",
            "-",
        ]
    )
    silences = _parse_silences(stderr, duration)
    segments = _speech_segments(silences, duration, margin)
    if not segments:
        raise ToolError(
            "Nenhum trecho com fala foi encontrado: o arquivo inteiro está abaixo do limiar.",
            code="invalid_argument",
            hint="Aumente threshold_db (ex.: -40) ou confira o áudio com extract_audio.",
        )

    output = runtime.workspace.output_for(source, "nosilence")
    codec_args = ["-c:v", "libx264", "-c:a", "aac"] if info["has_video"] else ["-c:a", "aac"]
    maps = ["-map", "[v]", "-map", "[a]"] if info["has_video"] else ["-map", "[a]"]
    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False, encoding="utf-8") as script:
        script.write(_filter_script(segments, has_video=info["has_video"]))
        script_path = Path(script.name)
    try:
        runtime.ffmpeg.run(
            [
                "-i",
                str(source),
                "-filter_complex_script",
                str(script_path),
                *maps,
                *codec_args,
                str(output),
            ]
        )
    finally:
        script_path.unlink(missing_ok=True)

    kept = sum(seg["duration"] for seg in segments)
    return RemoveSilenceResult(
        output=runtime.workspace.relative(output),
        threshold_db=threshold_db,
        min_silence=min_silence,
        margin=margin,
        original_duration=round(duration, 3),
        duration=round(kept, 3),
        removed_duration=round(max(duration - kept, 0.0), 3),
        silences_removed=len(silences),
        segments=segments,
    )


@guarded
def remove_silence(
    runtime: Runtime,
    path: str,
    *,
    threshold_db: float = _DEFAULT_THRESHOLD_DB,
    min_silence: float = _DEFAULT_MIN_SILENCE,
    margin: float = _DEFAULT_MARGIN,
    background: bool = False,
) -> RemoveSilenceResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "remove_silence",
            lambda: _do_remove(
                runtime, path, threshold_db=threshold_db, min_silence=min_silence, margin=margin
            ),
        )
    return _do_remove(
        runtime, path, threshold_db=threshold_db, min_silence=min_silence, margin=margin
    )


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="remove_silence")
    def _tool(
        path: str,
        threshold_db: float = _DEFAULT_THRESHOLD_DB,
        min_silence: float = _DEFAULT_MIN_SILENCE,
        margin: float = _DEFAULT_MARGIN,
        background: bool = False,
    ) -> RemoveSilenceResult | JobSubmitted | ErrorPayload:
        """Remove as pausas sem fala de um vídeo ou áudio e salva um novo arquivo.

        Use em aulas, palestras e gravações de tela para tirar os trechos em que
        o locutor fica em silêncio. O original não é modificado. O resultado é
        re-encodado para que os cortes caiam exatamente onde a fala começa e
        termina, então prefira background=true em vídeos longos e acompanhe com
        job_status. Devolve o arquivo gerado, quanto tempo foi removido e a lista
        dos trechos mantidos.

        Args:
            path: Vídeo ou áudio de origem, relativo ao workspace.
            threshold_db: Nível abaixo do qual o áudio conta como silêncio, em dB.
                -30 serve para a maioria das gravações; use -40 se cortar fala baixa.
            min_silence: Duração mínima, em segundos, para uma pausa ser removida.
            margin: Segundos preservados antes e depois de cada fala para não
                cortar o início ou o fim das palavras.
            background: Executa como job e devolve job_id.
        """
        return remove_silence(
            runtime,
            path,
            threshold_db=threshold_db,
            min_silence=min_silence,
            margin=margin,
            background=background,
        )
