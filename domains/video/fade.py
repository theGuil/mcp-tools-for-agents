"""Tool ``add_fade``: fade de entrada e saída na imagem e no som."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final, Literal, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

type FadeColor = Literal["black", "white"]

_MAX_FADE: Final = 30.0


class AddFadeResult(TypedDict):
    """Vídeo gerado com os fades."""

    output: str
    fade_in: float
    fade_out: float
    color: FadeColor
    audio_faded: bool
    duration: float


def _validate(fade_in: float, fade_out: float, color: FadeColor) -> None:
    if fade_in < 0 or fade_out < 0:
        raise ToolError("fade_in e fade_out devem ser >= 0.", code="invalid_argument")
    if fade_in == 0 and fade_out == 0:
        raise ToolError(
            "Informe fade_in ou fade_out maior que zero.",
            code="invalid_argument",
            hint="Ex: fade_in=0.5, fade_out=1.0.",
        )
    if fade_in > _MAX_FADE or fade_out > _MAX_FADE:
        raise ToolError(
            f"fade_in e fade_out devem ter no máximo {_MAX_FADE:g} segundos.",
            code="invalid_argument",
        )
    if color not in {"black", "white"}:
        raise ToolError("color deve ser 'black' ou 'white'.", code="invalid_argument")


def _audio_args(audio_steps: list[str], *, fade_audio: bool, has_audio: bool) -> list[str]:
    if fade_audio:
        return ["-af", ",".join(audio_steps), "-c:a", "aac"]
    if has_audio:
        return ["-c:a", "copy"]
    return []


def _do_fade(
    runtime: Runtime,
    path: str,
    *,
    fade_in: float,
    fade_out: float,
    color: FadeColor,
    audio: bool,
) -> AddFadeResult:
    source = runtime.workspace.existing(path)
    _validate(fade_in, fade_out, color)
    info = runtime.ffmpeg.probe(source)
    if not info["has_video"]:
        raise ToolError(
            f"'{path}' não tem trilha de vídeo.",
            code="invalid_argument",
            hint="Para áudio puro use normalize_audio ou add_background_music.",
        )
    duration = info["duration"]
    if fade_in + fade_out > duration:
        raise ToolError(
            f"fade_in + fade_out ({fade_in + fade_out:g}s) é maior que o vídeo ({duration:.2f}s).",
            code="invalid_argument",
            hint="Use probe_video para conferir a duração e reduza os fades.",
        )
    video_steps: list[str] = []
    audio_steps: list[str] = []
    if fade_in > 0:
        video_steps.append(f"fade=t=in:st=0:d={fade_in:.3f}:color={color}")
        audio_steps.append(f"afade=t=in:st=0:d={fade_in:.3f}")
    if fade_out > 0:
        out_start = max(duration - fade_out, 0.0)
        video_steps.append(f"fade=t=out:st={out_start:.3f}:d={fade_out:.3f}:color={color}")
        audio_steps.append(f"afade=t=out:st={out_start:.3f}:d={fade_out:.3f}")
    fade_audio = audio and info["has_audio"]
    audio_args = _audio_args(audio_steps, fade_audio=fade_audio, has_audio=info["has_audio"])
    output = runtime.workspace.output_for(source, "fade")
    runtime.ffmpeg.run(
        [
            "-i",
            str(source),
            "-vf",
            ",".join(video_steps),
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
    return AddFadeResult(
        output=runtime.workspace.relative(output),
        fade_in=fade_in,
        fade_out=fade_out,
        color=color,
        audio_faded=fade_audio,
        duration=round(duration, 3),
    )


@guarded
def add_fade(
    runtime: Runtime,
    path: str,
    *,
    fade_in: float = 0.5,
    fade_out: float = 1.0,
    color: FadeColor = "black",
    audio: bool = True,
    background: bool = False,
) -> AddFadeResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "add_fade",
            lambda: _do_fade(
                runtime, path, fade_in=fade_in, fade_out=fade_out, color=color, audio=audio
            ),
        )
    return _do_fade(runtime, path, fade_in=fade_in, fade_out=fade_out, color=color, audio=audio)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="add_fade")
    def _tool(  # noqa: PLR0917  # a assinatura da tool é a interface do agente
        path: str,
        fade_in: float = 0.5,
        fade_out: float = 1.0,
        color: FadeColor = "black",
        audio: bool = True,
        background: bool = False,
    ) -> AddFadeResult | JobSubmitted | ErrorPayload:
        """Aplica fade de entrada (do preto para a imagem) e de saída (da imagem para o preto).

        Use como último acabamento antes de exportar: evita começo e fim bruscos,
        que são a marca de um corte amador. O som acompanha o fade por padrão
        (audio=true). Passe 0 em fade_in ou fade_out para aplicar só um dos dois.
        O original não é modificado; o vídeo é re-encodado.

        Para transições ENTRE clipes use concat_videos com transition="fade".

        Args:
            path: Vídeo, relativo ao workspace.
            fade_in: Segundos do fade de entrada. 0 desativa.
            fade_out: Segundos do fade de saída. 0 desativa.
            color: Cor do fade: "black" (padrão) ou "white".
            audio: Aplica o mesmo fade no áudio.
            background: Executa como job e devolve job_id.
        """
        return add_fade(
            runtime,
            path,
            fade_in=fade_in,
            fade_out=fade_out,
            color=color,
            audio=audio,
            background=background,
        )
