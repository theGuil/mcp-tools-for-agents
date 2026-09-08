"""Tool ``add_background_music``: trilha sonora em loop, com fade e ducking automático."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

_MAX_FADE: Final = 30.0


class AddBackgroundMusicResult(TypedDict):
    """Vídeo gerado com a trilha de fundo."""

    output: str
    music: str
    music_volume: float
    ducking: bool
    looped: bool
    fade_in: float
    fade_out: float
    duration: float


def _validate(
    *,
    music_volume: float,
    fade_in: float,
    fade_out: float,
    start: float,
    duration: float,
) -> None:
    if not 0.0 < music_volume <= 1.0:
        raise ToolError(
            "music_volume deve estar entre 0 (exclusivo) e 1.",
            code="invalid_argument",
            hint="0.15 a 0.3 é o usual para fundo de fala; 1 deixa a música no volume original.",
        )
    if fade_in < 0 or fade_out < 0 or fade_in > _MAX_FADE or fade_out > _MAX_FADE:
        raise ToolError(
            f"fade_in e fade_out devem estar entre 0 e {_MAX_FADE:g} segundos.",
            code="invalid_argument",
        )
    if start < 0:
        raise ToolError("start deve ser >= 0.", code="invalid_argument")
    if start >= duration:
        raise ToolError(
            f"start={start}s ultrapassa a duração do vídeo ({duration:.2f}s).",
            code="invalid_argument",
            hint="Use probe_video para conferir a duração.",
        )


def _music_chain(
    *,
    music_volume: float,
    fade_in: float,
    fade_out: float,
    start: float,
    duration: float,
    loop: bool,
) -> str:
    """Prepara a música: loop, corte na duração do vídeo, fades e volume."""
    music_len = duration - start
    steps = ["aloop=loop=-1:size=2e9"] if loop else []
    steps.append(f"atrim=0:{music_len:.3f}")
    steps.append("asetpts=PTS-STARTPTS")
    if fade_in > 0:
        steps.append(f"afade=t=in:st=0:d={fade_in:.3f}")
    if fade_out > 0:
        fade_start = max(music_len - fade_out, 0.0)
        steps.append(f"afade=t=out:st={fade_start:.3f}:d={fade_out:.3f}")
    steps.append(f"volume={music_volume}")
    if start > 0:
        steps.append(f"adelay={round(start * 1000)}:all=1")
    return ",".join(steps)


def _do_add_music(
    runtime: Runtime,
    path: str,
    music_path: str,
    *,
    music_volume: float,
    ducking: bool,
    fade_in: float,
    fade_out: float,
    start: float,
    loop: bool,
) -> AddBackgroundMusicResult:
    source = runtime.workspace.existing(path)
    music = runtime.workspace.existing(music_path)
    info = runtime.ffmpeg.probe(source)
    music_info = runtime.ffmpeg.probe(music)
    if not music_info["has_audio"]:
        raise ToolError(
            f"'{music_path}' não tem trilha de áudio.",
            code="invalid_argument",
            hint="Informe um arquivo de música (mp3, m4a, wav, ogg).",
        )
    duration = info["duration"]
    _validate(
        music_volume=music_volume,
        fade_in=fade_in,
        fade_out=fade_out,
        start=start,
        duration=duration,
    )
    music_chain = _music_chain(
        music_volume=music_volume,
        fade_in=fade_in,
        fade_out=fade_out,
        start=start,
        duration=duration,
        loop=loop,
    )
    has_voice = info["has_audio"]
    apply_ducking = ducking and has_voice
    if not has_voice:
        filter_expr = f"[1:a]{music_chain},apad[aout]"
    elif apply_ducking:
        # sidechaincompress abaixa a música sempre que a fala (sidechain) passa do threshold.
        filter_expr = (
            f"[1:a]{music_chain}[music];"
            "[0:a]asplit=2[voice][sc];"
            "[music][sc]sidechaincompress=threshold=0.03:ratio=8:attack=20:release=400"
            ":makeup=1[ducked];"
            "[voice][ducked]amix=inputs=2:duration=first:dropout_transition=0:normalize=0[aout]"
        )
    else:
        filter_expr = (
            f"[1:a]{music_chain}[music];"
            "[0:a][music]amix=inputs=2:duration=first:dropout_transition=0:normalize=0[aout]"
        )
    output = runtime.workspace.output_for(source, "music")
    runtime.ffmpeg.run(
        [
            "-i",
            str(source),
            "-i",
            str(music),
            "-filter_complex",
            filter_expr,
            "-map",
            "0:v:0",
            "-map",
            "[aout]",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-shortest",
            str(output),
        ]
    )
    return AddBackgroundMusicResult(
        output=runtime.workspace.relative(output),
        music=runtime.workspace.relative(music),
        music_volume=music_volume,
        ducking=apply_ducking,
        looped=loop,
        fade_in=fade_in,
        fade_out=fade_out,
        duration=round(duration, 3),
    )


@guarded
def add_background_music(
    runtime: Runtime,
    path: str,
    music_path: str,
    *,
    music_volume: float = 0.2,
    ducking: bool = True,
    fade_in: float = 1.0,
    fade_out: float = 2.0,
    start: float = 0.0,
    loop: bool = True,
    background: bool = False,
) -> AddBackgroundMusicResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "add_background_music",
            lambda: _do_add_music(
                runtime,
                path,
                music_path,
                music_volume=music_volume,
                ducking=ducking,
                fade_in=fade_in,
                fade_out=fade_out,
                start=start,
                loop=loop,
            ),
        )
    return _do_add_music(
        runtime,
        path,
        music_path,
        music_volume=music_volume,
        ducking=ducking,
        fade_in=fade_in,
        fade_out=fade_out,
        start=start,
        loop=loop,
    )


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="add_background_music")
    def _tool(  # noqa: PLR0917  # a assinatura da tool é a interface do agente
        path: str,
        music_path: str,
        music_volume: float = 0.2,
        ducking: bool = True,
        fade_in: float = 1.0,
        fade_out: float = 2.0,
        start: float = 0.0,
        loop: bool = True,
        background: bool = False,
    ) -> AddBackgroundMusicResult | JobSubmitted | ErrorPayload:
        """Coloca música de fundo no vídeo, em loop, com fade e volume que abaixa quando há fala.

        Use para dar clima a um corte de TikTok, Reels ou YouTube depois de já ter o
        vídeo cortado e legendado. A música é repetida até cobrir o vídeo inteiro
        (loop=true), entra com fade_in, sai com fade_out no final e toca no volume
        music_volume. Com ducking=true a música abaixa sozinha enquanto alguém fala e
        volta ao normal nas pausas (sidechain), o padrão dos editores profissionais.
        A fala original é preservada. O vídeo não é re-encodado, só o áudio.

        Diferença para add_narration: add_narration coloca uma VOZ por cima do vídeo;
        add_background_music coloca uma MÚSICA por baixo da voz que já existe.

        Fluxo típico: cut_video -> burn_subtitles -> add_background_music ->
        normalize_audio -> export_for_platform.

        Args:
            path: Vídeo, relativo ao workspace.
            music_path: Arquivo de música (mp3, m4a, wav, ogg), relativo ao workspace.
            music_volume: Volume da música de 0 a 1. 0.2 é fundo discreto para fala, 0.5
                bem presente, 1 volume original (só para vídeo sem fala).
            ducking: Abaixa a música automaticamente enquanto há fala no vídeo.
            fade_in: Segundos de entrada suave da música no início.
            fade_out: Segundos de saída suave da música no final do vídeo.
            start: Segundo do vídeo em que a música começa.
            loop: Repete a música até o fim do vídeo. Com false ela toca uma vez e para.
            background: Executa como job e devolve job_id.
        """
        return add_background_music(
            runtime,
            path,
            music_path,
            music_volume=music_volume,
            ducking=ducking,
            fade_in=fade_in,
            fade_out=fade_out,
            start=start,
            loop=loop,
            background=background,
        )
