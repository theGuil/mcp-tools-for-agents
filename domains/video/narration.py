"""Tool ``add_narration``: mistura um áudio de narração no vídeo."""

from __future__ import annotations

from typing import TYPE_CHECKING, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


class AddNarrationResult(TypedDict):
    """Vídeo gerado com a narração."""

    output: str
    narration: str
    start: float
    original_volume: float
    replaced_audio: bool


def _do_narration(
    runtime: Runtime,
    path: str,
    audio_path: str,
    *,
    start: float,
    original_volume: float,
) -> AddNarrationResult:
    source = runtime.workspace.existing(path)
    narration = runtime.workspace.existing(audio_path)
    if start < 0:
        raise ToolError("start deve ser >= 0.", code="invalid_argument")
    if not 0.0 <= original_volume <= 1.0:
        raise ToolError(
            "original_volume deve estar entre 0 e 1.",
            code="invalid_argument",
            hint="0 silencia o áudio original, 1 mantém o volume, 0.2 deixa de fundo.",
        )
    info = runtime.ffmpeg.probe(source)
    narration_info = runtime.ffmpeg.probe(narration)
    if not narration_info["has_audio"]:
        raise ToolError(
            f"'{audio_path}' não tem trilha de áudio.",
            code="invalid_argument",
            hint="Informe um arquivo de áudio (mp3, m4a, wav) ou um vídeo com som.",
        )
    if start >= info["duration"]:
        raise ToolError(
            f"start={start}s ultrapassa a duração do vídeo ({info['duration']:.2f}s).",
            code="invalid_argument",
            hint="Use probe_video para conferir a duração.",
        )
    delay_ms = round(start * 1000)
    replace = original_volume == 0.0 or not info["has_audio"]
    if replace:
        filter_expr = f"[1:a]adelay={delay_ms}:all=1,apad[aout]"
    else:
        filter_expr = (
            f"[0:a]volume={original_volume}[bg];"
            f"[1:a]adelay={delay_ms}:all=1[nar];"
            "[bg][nar]amix=inputs=2:duration=first:dropout_transition=0:normalize=0[aout]"
        )
    output = runtime.workspace.output_for(source, "narrated")
    runtime.ffmpeg.run(
        [
            "-i",
            str(source),
            "-i",
            str(narration),
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
            "-shortest",
            str(output),
        ]
    )
    return AddNarrationResult(
        output=runtime.workspace.relative(output),
        narration=runtime.workspace.relative(narration),
        start=start,
        original_volume=original_volume,
        replaced_audio=replace,
    )


@guarded
def add_narration(
    runtime: Runtime,
    path: str,
    audio_path: str,
    *,
    start: float = 0.0,
    original_volume: float = 0.2,
    background: bool = False,
) -> AddNarrationResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "add_narration",
            lambda: _do_narration(
                runtime, path, audio_path, start=start, original_volume=original_volume
            ),
        )
    return _do_narration(runtime, path, audio_path, start=start, original_volume=original_volume)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="add_narration")
    def _tool(
        path: str,
        audio_path: str,
        start: float = 0.0,
        original_volume: float = 0.2,
        background: bool = False,
    ) -> AddNarrationResult | JobSubmitted | ErrorPayload:
        """Mistura um áudio de narração (voz, locução) por cima do som do vídeo.

        Use quando você gerou ou recebeu um áudio com a descrição falada e quer
        colocá-lo no vídeo. O áudio original fica de fundo no volume indicado;
        original_volume=0 substitui o som por completo. O vídeo não é re-encodado.

        Args:
            path: Vídeo, relativo ao workspace.
            audio_path: Áudio da narração (mp3, m4a, wav), relativo ao workspace.
            start: Segundo em que a narração começa.
            original_volume: Volume do áudio original, de 0 (mudo) a 1 (igual).
            background: Executa como job e devolve job_id.
        """
        return add_narration(
            runtime,
            path,
            audio_path,
            start=start,
            original_volume=original_volume,
            background=background,
        )
