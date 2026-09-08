"""Tool ``normalize_audio``: normaliza o volume para o padrão de loudness das plataformas."""

from __future__ import annotations

import json
import math
import re
from typing import TYPE_CHECKING, Final, Literal, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

type LoudnessPreset = Literal["tiktok", "youtube", "instagram", "podcast", "broadcast"]

# Loudness integrada (LUFS) recomendada por cada plataforma.
PRESETS: Final[dict[LoudnessPreset, float]] = {
    "tiktok": -14.0,
    "youtube": -14.0,
    "instagram": -14.0,
    "podcast": -16.0,
    "broadcast": -23.0,
}
_TRUE_PEAK: Final = -1.5
_LRA: Final = 11.0
_MIN_LUFS: Final = -70.0
_MAX_LUFS: Final = -5.0
_JSON_BLOCK: Final = re.compile(r"\{[^{}]*\"input_i\"[^{}]*\}", re.DOTALL)


class NormalizeAudioResult(TypedDict):
    """Arquivo gerado com o áudio normalizado."""

    output: str
    target_lufs: float
    measured_lufs: float | None
    measured_true_peak: float | None
    gain_db: float | None
    is_video: bool


class _Measurement(TypedDict):
    input_i: str
    input_tp: str
    input_lra: str
    input_thresh: str
    target_offset: str


def _resolve_target(preset: LoudnessPreset | None, target_lufs: float | None) -> float:
    if target_lufs is not None:
        if not _MIN_LUFS <= target_lufs <= _MAX_LUFS:
            raise ToolError(
                f"target_lufs deve estar entre {_MIN_LUFS:g} e {_MAX_LUFS:g}.",
                code="invalid_argument",
                hint="-14 é o padrão de TikTok, YouTube e Instagram.",
            )
        return target_lufs
    if preset is None:
        return PRESETS["youtube"]
    if preset not in PRESETS:
        raise ToolError(
            f"preset '{preset}' não existe.",
            code="invalid_argument",
            hint=f"Use um de: {', '.join(PRESETS)}.",
        )
    return PRESETS[preset]


def _measure(runtime: Runtime, source_args: list[str], target: float) -> _Measurement:
    """Primeira passada do loudnorm: só mede, sem gravar nada."""
    stderr = runtime.ffmpeg.run(
        [
            *source_args,
            "-af",
            f"loudnorm=I={target}:TP={_TRUE_PEAK}:LRA={_LRA}:print_format=json",
            "-vn",
            "-f",
            "null",
            "-",
        ]
    )
    match = _JSON_BLOCK.search(stderr)
    if match is None:
        raise ToolError(
            "Não foi possível medir o loudness do áudio.",
            code="ffmpeg_failed",
            hint="Confira com probe_video se o arquivo tem trilha de áudio válida.",
        )
    data = json.loads(match.group(0))
    return _Measurement(
        input_i=str(data["input_i"]),
        input_tp=str(data["input_tp"]),
        input_lra=str(data["input_lra"]),
        input_thresh=str(data["input_thresh"]),
        target_offset=str(data["target_offset"]),
    )


def _as_float(value: str) -> float | None:
    try:
        number = float(value)
    except ValueError:
        return None
    return None if math.isinf(number) or math.isnan(number) else number


def _do_normalize(
    runtime: Runtime,
    path: str,
    *,
    preset: LoudnessPreset | None,
    target_lufs: float | None,
) -> NormalizeAudioResult:
    source = runtime.workspace.existing(path)
    target = _resolve_target(preset, target_lufs)
    info = runtime.ffmpeg.probe(source)
    if not info["has_audio"]:
        raise ToolError(
            f"'{path}' não tem trilha de áudio.",
            code="invalid_argument",
            hint="Só arquivos com som podem ser normalizados.",
        )
    measured = _measure(runtime, ["-i", str(source)], target)
    measured_i = _as_float(measured["input_i"])
    if measured_i is None:
        raise ToolError(
            "O áudio é silêncio total, não há o que normalizar.",
            code="invalid_argument",
            hint="Confira o volume original com probe_video ou extraia o áudio com extract_audio.",
        )
    second_pass = (
        f"loudnorm=I={target}:TP={_TRUE_PEAK}:LRA={_LRA}"
        f":measured_I={measured['input_i']}:measured_TP={measured['input_tp']}"
        f":measured_LRA={measured['input_lra']}:measured_thresh={measured['input_thresh']}"
        f":offset={measured['target_offset']}:linear=true:print_format=summary"
    )
    is_video = info["has_video"]
    output = runtime.workspace.output_for(source, "normalized")
    if is_video:
        codec_args = [
            "-map",
            "0:v:0",
            "-map",
            "0:a:0",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
        ]
    else:
        codec_args = ["-map", "0:a:0"]
    runtime.ffmpeg.run(
        [
            "-i",
            str(source),
            "-af",
            second_pass,
            *codec_args,
            str(output),
        ]
    )
    return NormalizeAudioResult(
        output=runtime.workspace.relative(output),
        target_lufs=target,
        measured_lufs=measured_i,
        measured_true_peak=_as_float(measured["input_tp"]),
        gain_db=round(target - measured_i, 2),
        is_video=is_video,
    )


@guarded
def normalize_audio(
    runtime: Runtime,
    path: str,
    *,
    preset: LoudnessPreset | None = None,
    target_lufs: float | None = None,
    background: bool = False,
) -> NormalizeAudioResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "normalize_audio",
            lambda: _do_normalize(runtime, path, preset=preset, target_lufs=target_lufs),
        )
    return _do_normalize(runtime, path, preset=preset, target_lufs=target_lufs)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="normalize_audio")
    def _tool(
        path: str,
        preset: LoudnessPreset | None = None,
        target_lufs: float | None = None,
        background: bool = False,
    ) -> NormalizeAudioResult | JobSubmitted | ErrorPayload:
        """Normaliza o volume do áudio para o padrão de loudness da plataforma (EBU R128).

        Use antes de exportar: garante que o vídeo não toque baixo demais nem
        estoure em relação aos outros vídeos do feed. Faz duas passadas do
        loudnorm (mede e depois corrige), o mesmo processo de mastering usado
        em estúdio. Funciona em vídeo (só o áudio é re-encodado, a imagem é
        copiada) e em áudio puro (mp3, wav, m4a).

        Presets: tiktok, youtube e instagram (-14 LUFS), podcast (-16 LUFS),
        broadcast (-23 LUFS). Sem preset usa -14 LUFS. target_lufs sobrescreve.
        Devolve o loudness medido e o ganho aplicado em dB.

        Args:
            path: Vídeo ou áudio, relativo ao workspace.
            preset: Plataforma alvo: tiktok, youtube, instagram, podcast ou broadcast.
            target_lufs: Loudness alvo em LUFS, ex: -14. Ignora o preset se informado.
            background: Executa como job e devolve job_id.
        """
        return normalize_audio(
            runtime, path, preset=preset, target_lufs=target_lufs, background=background
        )
