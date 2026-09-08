"""Tool ``concat_videos``: junta vários vídeos em sequência, com ou sem transição."""

from __future__ import annotations

import tempfile
from pathlib import Path
from typing import TYPE_CHECKING, Final, Literal, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from core.ffmpeg import ProbeResult
    from domains import Runtime

type TransitionName = Literal[
    "fade",
    "fadeblack",
    "fadewhite",
    "dissolve",
    "wipeleft",
    "wiperight",
    "wipeup",
    "wipedown",
    "slideleft",
    "slideright",
    "slideup",
    "slidedown",
    "smoothleft",
    "smoothright",
    "circleopen",
    "circleclose",
    "radial",
    "zoomin",
    "pixelize",
    "hblur",
]

TRANSITIONS: Final[tuple[TransitionName, ...]] = (
    "fade",
    "fadeblack",
    "fadewhite",
    "dissolve",
    "wipeleft",
    "wiperight",
    "wipeup",
    "wipedown",
    "slideleft",
    "slideright",
    "slideup",
    "slidedown",
    "smoothleft",
    "smoothright",
    "circleopen",
    "circleclose",
    "radial",
    "zoomin",
    "pixelize",
    "hblur",
)
_MIN_INPUTS: Final = 2
_MAX_TRANSITION: Final = 5.0
_DEFAULT_FPS: Final = 30.0


class ConcatVideosResult(TypedDict):
    """Arquivo gerado pela concatenação."""

    output: str
    inputs: list[str]
    count: int
    transition: TransitionName | None
    transition_duration: float | None
    has_audio: bool


def _escape_for_concat(path: Path) -> str:
    return str(path).replace("'", r"'\''")


def _resolve_output(runtime: Runtime, sources: list[Path], output_name: str | None) -> Path:
    output = (
        runtime.workspace.resolve(output_name)
        if output_name
        else runtime.workspace.output_for(sources[0], f"concat_{len(sources)}")
    )
    output.parent.mkdir(parents=True, exist_ok=True)
    return output


def _concat_copy(runtime: Runtime, sources: list[Path], output: Path) -> None:
    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False, encoding="utf-8") as listing:
        listing.writelines(f"file '{_escape_for_concat(s)}'\n" for s in sources)
        list_path = Path(listing.name)
    try:
        runtime.ffmpeg.run(
            ["-f", "concat", "-safe", "0", "-i", str(list_path), "-c", "copy", str(output)]
        )
    finally:
        list_path.unlink(missing_ok=True)


def transition_offsets(durations: list[float], transition_duration: float) -> list[float]:
    """Instante em que cada transição começa, já descontando as sobreposições anteriores."""
    offsets: list[float] = []
    elapsed = 0.0
    for duration in durations[:-1]:
        elapsed += duration - transition_duration
        offsets.append(round(elapsed, 3))
    return offsets


def _transition_filter(
    infos: list[ProbeResult],
    transition: TransitionName,
    transition_duration: float,
    *,
    width: int,
    height: int,
    fps: float,
    with_audio: bool,
) -> str:
    chains: list[str] = []
    for index in range(len(infos)):
        chains.append(
            f"[{index}:v]scale={width}:{height}:force_original_aspect_ratio=decrease,"
            f"pad={width}:{height}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={fps:g},"
            f"format=yuv420p,settb=AVTB[v{index}]"
        )
        if with_audio:
            chains.append(
                f"[{index}:a]aformat=sample_rates=48000:channel_layouts=stereo,"
                f"asetpts=PTS-STARTPTS[a{index}]"
            )
    offsets = transition_offsets([i["duration"] for i in infos], transition_duration)
    previous = "[v0]"
    for index, offset in enumerate(offsets, start=1):
        label = "[vout]" if index == len(offsets) else f"[x{index}]"
        chains.append(
            f"{previous}[v{index}]xfade=transition={transition}"
            f":duration={transition_duration:.3f}:offset={offset:.3f}{label}"
        )
        previous = label
    if with_audio:
        previous = "[a0]"
        for index in range(1, len(infos)):
            label = "[aout]" if index == len(infos) - 1 else f"[ax{index}]"
            chains.append(
                f"{previous}[a{index}]acrossfade=d={transition_duration:.3f}:c1=tri:c2=tri{label}"
            )
            previous = label
    return ";".join(chains)


def _concat_transition(
    runtime: Runtime,
    sources: list[Path],
    output: Path,
    transition: TransitionName,
    transition_duration: float,
) -> bool:
    if transition not in TRANSITIONS:
        raise ToolError(
            f"transition '{transition}' não existe.",
            code="invalid_argument",
            hint=f"Use uma de: {', '.join(TRANSITIONS)}.",
        )
    if not 0 < transition_duration <= _MAX_TRANSITION:
        raise ToolError(
            f"transition_duration deve estar entre 0 e {_MAX_TRANSITION:g} segundos.",
            code="invalid_argument",
            hint="0.5 é o padrão; 1 fica mais lento e cinematográfico.",
        )
    infos = [runtime.ffmpeg.probe(s) for s in sources]
    for source, info in zip(sources, infos, strict=True):
        if not info["has_video"] or info["width"] is None or info["height"] is None:
            raise ToolError(
                f"'{runtime.workspace.relative(source)}' não tem trilha de vídeo.",
                code="invalid_argument",
            )
        if info["duration"] <= transition_duration:
            raise ToolError(
                f"'{runtime.workspace.relative(source)}' dura {info['duration']:.2f}s, "
                f"menos que a transição de {transition_duration:g}s.",
                code="invalid_argument",
                hint="Reduza transition_duration ou use clipes mais longos.",
            )
    first = infos[0]
    width, height = first["width"] or 0, first["height"] or 0
    fps = first["fps"] or _DEFAULT_FPS
    with_audio = all(i["has_audio"] for i in infos)
    filter_expr = _transition_filter(
        infos,
        transition,
        transition_duration,
        width=width,
        height=height,
        fps=fps,
        with_audio=with_audio,
    )
    inputs = [arg for s in sources for arg in ("-i", str(s))]
    audio_args = ["-map", "[aout]", "-c:a", "aac", "-b:a", "192k"] if with_audio else []
    runtime.ffmpeg.run(
        [
            *inputs,
            "-filter_complex",
            filter_expr,
            "-map",
            "[vout]",
            *audio_args,
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-pix_fmt",
            "yuv420p",
            str(output),
        ]
    )
    return with_audio


def _do_concat(
    runtime: Runtime,
    paths: list[str],
    output_name: str | None,
    transition: TransitionName | None,
    transition_duration: float,
) -> ConcatVideosResult:
    if len(paths) < _MIN_INPUTS:
        raise ToolError(
            "Informe pelo menos dois vídeos para concatenar.",
            code="invalid_argument",
        )
    sources = [runtime.workspace.existing(p) for p in paths]
    output = _resolve_output(runtime, sources, output_name)
    if transition is None:
        _concat_copy(runtime, sources, output)
        has_audio = runtime.ffmpeg.probe(output)["has_audio"]
        applied_duration = None
    else:
        has_audio = _concat_transition(runtime, sources, output, transition, transition_duration)
        applied_duration = transition_duration
    return ConcatVideosResult(
        output=runtime.workspace.relative(output),
        inputs=[runtime.workspace.relative(s) for s in sources],
        count=len(sources),
        transition=transition,
        transition_duration=applied_duration,
        has_audio=has_audio,
    )


@guarded
def concat_videos(
    runtime: Runtime,
    paths: list[str],
    *,
    output_name: str | None = None,
    transition: TransitionName | None = None,
    transition_duration: float = 0.5,
    background: bool = False,
) -> ConcatVideosResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "concat_videos",
            lambda: _do_concat(runtime, paths, output_name, transition, transition_duration),
        )
    return _do_concat(runtime, paths, output_name, transition, transition_duration)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="concat_videos")
    def _tool(
        paths: list[str],
        output_name: str | None = None,
        transition: TransitionName | None = None,
        transition_duration: float = 0.5,
        background: bool = False,
    ) -> ConcatVideosResult | JobSubmitted | ErrorPayload:
        """Junta dois ou mais vídeos na ordem informada, com corte seco ou com transição.

        Sem transition: emenda direta e sem re-encode, os vídeos precisam ter o
        mesmo codec e resolução (por exemplo, cortes do mesmo original). Com
        transition: os clipes são redimensionados para o tamanho do primeiro e
        emendados com o efeito escolhido, o áudio faz crossfade e o resultado é
        re-encodado (use background=true para muitos clipes).

        Transições disponíveis: fade (dissolve suave, a mais usada), fadeblack,
        fadewhite, dissolve, wipeleft, wiperight, wipeup, wipedown, slideleft,
        slideright, slideup, slidedown, smoothleft, smoothright, circleopen,
        circleclose, radial, zoomin, pixelize, hblur. Cada transição consome
        transition_duration segundos de cada clipe, então o resultado fica um pouco
        mais curto que a soma. Os arquivos de entrada não são alterados.

        Args:
            paths: Lista de vídeos, relativa ao workspace, na ordem desejada.
            output_name: Nome do arquivo de saída. Gerado automaticamente se omitido.
            transition: Nome da transição entre os clipes. Omita para corte seco.
            transition_duration: Duração de cada transição em segundos (0.3 a 1 é o usual).
            background: Executa como job e devolve job_id.
        """
        return concat_videos(
            runtime,
            paths,
            output_name=output_name,
            transition=transition,
            transition_duration=transition_duration,
            background=background,
        )
