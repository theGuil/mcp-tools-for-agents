"""Tool ``change_speed``: acelera ou desacelera o vídeo inteiro ou um trecho."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

_MIN_FACTOR: Final = 0.25
_MAX_FACTOR: Final = 8.0
# atempo aceita de 0.5 a 100 por instância; abaixo de 0.5 encadeamos várias.
_ATEMPO_MIN: Final = 0.5
_ATEMPO_MAX: Final = 100.0


class ChangeSpeedResult(TypedDict):
    """Vídeo gerado com a velocidade alterada."""

    output: str
    factor: float
    start: float | None
    end: float | None
    original_duration: float
    new_duration: float


def atempo_chain(factor: float) -> str:
    """Monta a cadeia ``atempo`` que mantém o tom da voz em qualquer fator."""
    parts: list[str] = []
    remaining = factor
    while remaining < _ATEMPO_MIN:
        parts.append(f"atempo={_ATEMPO_MIN}")
        remaining /= _ATEMPO_MIN
    while remaining > _ATEMPO_MAX:
        parts.append(f"atempo={_ATEMPO_MAX}")
        remaining /= _ATEMPO_MAX
    parts.append(f"atempo={remaining:.6g}")
    return ",".join(parts)


def _validate(factor: float, start: float | None, end: float | None, duration: float) -> None:
    if not _MIN_FACTOR <= factor <= _MAX_FACTOR:
        raise ToolError(
            f"factor deve estar entre {_MIN_FACTOR:g} e {_MAX_FACTOR:g}.",
            code="invalid_argument",
            hint="2 dobra a velocidade, 0.5 deixa em câmera lenta, 1.25 acelera levemente.",
        )
    if factor == 1.0:
        raise ToolError(
            "factor=1 não altera nada.",
            code="invalid_argument",
            hint="Informe um fator diferente de 1.",
        )
    if (start is None) != (end is None):
        raise ToolError(
            "Informe start e end juntos, ou nenhum dos dois para o vídeo inteiro.",
            code="invalid_argument",
        )
    if start is not None and end is not None:
        if start < 0 or end <= start:
            raise ToolError(
                f"Intervalo inválido: start={start}, end={end}.",
                code="invalid_argument",
                hint="start deve ser >= 0 e end maior que start, em segundos.",
            )
        if end > duration + 0.05:
            raise ToolError(
                f"end={end}s ultrapassa a duração do vídeo ({duration:.2f}s).",
                code="invalid_argument",
                hint="Use probe_video para conferir a duração.",
            )


def _whole_filter(factor: float, has_audio: bool) -> tuple[str, list[str]]:
    video = f"[0:v]setpts=PTS/{factor:.6g}[vout]"
    if not has_audio:
        return video, ["-map", "[vout]"]
    return f"{video};[0:a]{atempo_chain(factor)}[aout]", ["-map", "[vout]", "-map", "[aout]"]


def _segment_filter(
    factor: float, start: float, end: float, duration: float, has_audio: bool
) -> tuple[str, list[str]]:
    """Divide em antes / trecho / depois, altera só o trecho e junta de novo."""
    pieces: list[tuple[float, float, float]] = []
    if start > 0:
        pieces.append((0.0, start, 1.0))
    pieces.append((start, end, factor))
    if end < duration:
        pieces.append((end, duration, 1.0))
    chains: list[str] = []
    labels: list[str] = []
    for index, (seg_start, seg_end, seg_factor) in enumerate(pieces):
        speed = "" if seg_factor == 1.0 else f",setpts=PTS/{seg_factor:.6g}"
        chains.append(
            f"[0:v]trim=start={seg_start:.3f}:end={seg_end:.3f},setpts=PTS-STARTPTS{speed}[v{index}]"
        )
        labels.append(f"[v{index}]")
        if has_audio:
            tempo = "" if seg_factor == 1.0 else f",{atempo_chain(seg_factor)}"
            chains.append(
                f"[0:a]atrim=start={seg_start:.3f}:end={seg_end:.3f},asetpts=PTS-STARTPTS"
                f"{tempo}[a{index}]"
            )
            labels.append(f"[a{index}]")
    if has_audio:
        chains.append(f"{''.join(labels)}concat=n={len(pieces)}:v=1:a=1[vout][aout]")
        return ";".join(chains), ["-map", "[vout]", "-map", "[aout]"]
    chains.append(f"{''.join(labels)}concat=n={len(pieces)}:v=1:a=0[vout]")
    return ";".join(chains), ["-map", "[vout]"]


def _do_change_speed(
    runtime: Runtime,
    path: str,
    factor: float,
    *,
    start: float | None,
    end: float | None,
) -> ChangeSpeedResult:
    source = runtime.workspace.existing(path)
    info = runtime.ffmpeg.probe(source)
    if not info["has_video"]:
        raise ToolError(
            f"'{path}' não tem trilha de vídeo.",
            code="invalid_argument",
            hint="change_speed só se aplica a vídeos.",
        )
    duration = info["duration"]
    _validate(factor, start, end, duration)
    has_audio = info["has_audio"]
    if start is None or end is None:
        filter_expr, map_args = _whole_filter(factor, has_audio)
        new_duration = duration / factor
    else:
        end = min(end, duration)
        filter_expr, map_args = _segment_filter(factor, start, end, duration, has_audio)
        new_duration = duration - (end - start) + (end - start) / factor
    output = runtime.workspace.output_for(source, f"speed_{factor:g}x")
    audio_args = ["-c:a", "aac"] if has_audio else []
    runtime.ffmpeg.run(
        [
            "-i",
            str(source),
            "-filter_complex",
            filter_expr,
            *map_args,
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-pix_fmt",
            "yuv420p",
            *audio_args,
            str(output),
        ]
    )
    return ChangeSpeedResult(
        output=runtime.workspace.relative(output),
        factor=factor,
        start=start,
        end=end,
        original_duration=round(duration, 3),
        new_duration=round(new_duration, 3),
    )


@guarded
def change_speed(
    runtime: Runtime,
    path: str,
    factor: float,
    *,
    start: float | None = None,
    end: float | None = None,
    background: bool = False,
) -> ChangeSpeedResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "change_speed",
            lambda: _do_change_speed(runtime, path, factor, start=start, end=end),
        )
    return _do_change_speed(runtime, path, factor, start=start, end=end)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="change_speed")
    def _tool(
        path: str,
        factor: float,
        start: float | None = None,
        end: float | None = None,
        background: bool = False,
    ) -> ChangeSpeedResult | JobSubmitted | ErrorPayload:
        """Acelera ou desacelera o vídeo inteiro ou só um trecho, mantendo o tom da voz.

        Use para dar ritmo a um corte: acelere partes lentas (factor=1.5), faça
        um time-lapse (factor=4) ou uma câmera lenta de impacto (factor=0.5).
        Com start e end só aquele trecho muda de velocidade e o resto fica
        normal, tudo em um único arquivo. O áudio é ajustado sem virar "voz de
        esquilo". O original não é modificado; o vídeo é re-encodado.

        Devolve a duração antes e depois, útil para recalcular tempos de legenda.

        Args:
            path: Vídeo, relativo ao workspace.
            factor: Multiplicador de velocidade entre 0.25 e 8. 2 = duas vezes mais
                rápido, 0.5 = metade da velocidade.
            start: Início do trecho a alterar, em segundos. Omita para o vídeo todo.
            end: Fim do trecho a alterar, em segundos. Obrigatório junto com start.
            background: Executa como job e devolve job_id.
        """
        return change_speed(runtime, path, factor, start=start, end=end, background=background)
