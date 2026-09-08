"""Tool ``zoom_video``: zoom de impacto (punch-in) ou zoom progressivo (Ken Burns)."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final, Literal, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

type ZoomMode = Literal["punch", "in", "out"]

_MIN_ZOOM: Final = 1.05
_MAX_ZOOM: Final = 4.0


class ZoomVideoResult(TypedDict):
    """Vídeo gerado com o zoom."""

    output: str
    mode: ZoomMode
    zoom: float
    start: float
    end: float
    focus_x: float
    focus_y: float


def _validate(
    *, mode: ZoomMode, zoom: float, start: float, end: float, focus_x: float, focus_y: float
) -> None:
    if mode not in {"punch", "in", "out"}:
        raise ToolError(
            f"mode '{mode}' não existe.",
            code="invalid_argument",
            hint="Use 'punch' (corte seco), 'in' (aproxima aos poucos) ou 'out' (afasta).",
        )
    if not _MIN_ZOOM <= zoom <= _MAX_ZOOM:
        raise ToolError(
            f"zoom deve estar entre {_MIN_ZOOM:g} e {_MAX_ZOOM:g}.",
            code="invalid_argument",
            hint="1.2 é um punch-in sutil, 1.5 forte, 2 bem fechado.",
        )
    if start < 0 or end <= start:
        raise ToolError(
            f"Intervalo inválido: start={start}, end={end}.",
            code="invalid_argument",
            hint="start deve ser >= 0 e end maior que start, em segundos.",
        )
    if not (0.0 <= focus_x <= 1.0 and 0.0 <= focus_y <= 1.0):
        raise ToolError(
            "focus_x e focus_y devem estar entre 0 e 1.",
            code="invalid_argument",
            hint="0.5,0.5 é o centro; 0.5,0.3 mira um pouco acima (rosto de quem fala).",
        )


def zoom_expression(mode: ZoomMode, zoom: float, start: float, end: float) -> str:
    """Expressão do fator de zoom em função do tempo ``t`` para o filtro ``crop``."""
    length = end - start
    progress = f"((t-{start:.3f})/{length:.3f})"
    if mode == "punch":
        active = f"{zoom:.4f}"
    elif mode == "in":
        active = f"(1+({zoom:.4f}-1)*{progress})"
    else:
        active = f"({zoom:.4f}-({zoom:.4f}-1)*{progress})"
    return f"if(between(t\\,{start:.3f}\\,{end:.3f})\\,{active}\\,1)"


def _do_zoom(
    runtime: Runtime,
    path: str,
    *,
    mode: ZoomMode,
    zoom: float,
    start: float,
    end: float,
    focus_x: float,
    focus_y: float,
) -> ZoomVideoResult:
    source = runtime.workspace.existing(path)
    _validate(mode=mode, zoom=zoom, start=start, end=end, focus_x=focus_x, focus_y=focus_y)
    info = runtime.ffmpeg.probe(source)
    if not info["has_video"] or info["width"] is None or info["height"] is None:
        raise ToolError(
            f"'{path}' não tem trilha de vídeo.",
            code="invalid_argument",
            hint="zoom_video só se aplica a vídeos.",
        )
    if start >= info["duration"]:
        raise ToolError(
            f"start={start}s ultrapassa a duração do vídeo ({info['duration']:.2f}s).",
            code="invalid_argument",
            hint="Use probe_video para conferir a duração.",
        )
    end = min(end, info["duration"])
    width, height = info["width"], info["height"]
    z = zoom_expression(mode, zoom, start, end)
    # Recorta uma janela de tamanho iw/z centrada no ponto de foco (limitada às bordas)
    # e amplia de volta ao tamanho original. Resolução e proporção não mudam.
    crop = (
        f"crop=w='iw/({z})':h='ih/({z})'"
        f":x='clip(iw*{focus_x:.4f}-ow/2\\,0\\,iw-ow)'"
        f":y='clip(ih*{focus_y:.4f}-oh/2\\,0\\,ih-oh)'"
    )
    filter_expr = f"{crop},scale={width}:{height}:flags=lanczos,format=yuv420p"
    output = runtime.workspace.output_for(source, f"zoom_{mode}")
    audio_args = ["-c:a", "copy"] if info["has_audio"] else []
    runtime.ffmpeg.run(
        [
            "-i",
            str(source),
            "-vf",
            filter_expr,
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            *audio_args,
            str(output),
        ]
    )
    return ZoomVideoResult(
        output=runtime.workspace.relative(output),
        mode=mode,
        zoom=zoom,
        start=start,
        end=round(end, 3),
        focus_x=focus_x,
        focus_y=focus_y,
    )


@guarded
def zoom_video(
    runtime: Runtime,
    path: str,
    start: float,
    end: float,
    *,
    mode: ZoomMode = "punch",
    zoom: float = 1.3,
    focus_x: float = 0.5,
    focus_y: float = 0.5,
    background: bool = False,
) -> ZoomVideoResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "zoom_video",
            lambda: _do_zoom(
                runtime,
                path,
                mode=mode,
                zoom=zoom,
                start=start,
                end=end,
                focus_x=focus_x,
                focus_y=focus_y,
            ),
        )
    return _do_zoom(
        runtime,
        path,
        mode=mode,
        zoom=zoom,
        start=start,
        end=end,
        focus_x=focus_x,
        focus_y=focus_y,
    )


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="zoom_video")
    def _tool(  # noqa: PLR0917  # a assinatura da tool é a interface do agente
        path: str,
        start: float,
        end: float,
        mode: ZoomMode = "punch",
        zoom: float = 1.3,
        focus_x: float = 0.5,
        focus_y: float = 0.5,
        background: bool = False,
    ) -> ZoomVideoResult | JobSubmitted | ErrorPayload:
        """Aplica zoom em um trecho do vídeo: punch-in de impacto ou zoom progressivo.

        É o efeito mais usado em cortes de podcast e TikTok para dar ênfase a uma
        frase ou esconder um jump cut. Modos:
        - "punch": a imagem fecha de uma vez no instante start e volta ao normal em
          end (zoom seco de ênfase). Use zoom entre 1.2 e 1.5.
        - "in": aproxima aos poucos de 1x até zoom entre start e end (Ken Burns).
        - "out": começa em zoom e afasta até 1x entre start e end.
        A resolução do vídeo não muda. focus_x/focus_y (0 a 1) dizem para onde o
        zoom mira: 0.5,0.5 é o centro, 0.5,0.3 mira mais alto (rosto). Para
        vários zooms encadeie chamadas no arquivo gerado. O original não é modificado.

        Args:
            path: Vídeo, relativo ao workspace.
            start: Segundo em que o zoom começa.
            end: Segundo em que o zoom termina.
            mode: "punch", "in" ou "out".
            zoom: Fator de zoom entre 1.05 e 4. 1.3 é um bom padrão.
            focus_x: Ponto horizontal de foco, de 0 (esquerda) a 1 (direita).
            focus_y: Ponto vertical de foco, de 0 (topo) a 1 (base).
            background: Executa como job e devolve job_id.
        """
        return zoom_video(
            runtime,
            path,
            start,
            end,
            mode=mode,
            zoom=zoom,
            focus_x=focus_x,
            focus_y=focus_y,
            background=background,
        )
