"""Tool ``add_banner``: faixa com fundo colorido e texto no topo ou no rodapé do vídeo."""

from __future__ import annotations

import re
from typing import TYPE_CHECKING, Final, Literal, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.fonts import escape_drawtext, escape_filter_path, require_font
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

type BannerPosition = Literal["top", "bottom"]

_POSITIONS: Final[tuple[BannerPosition, ...]] = ("top", "bottom")
_MAX_TEXT: Final = 300
_MIN_HEIGHT_RATIO: Final = 0.03
_MAX_HEIGHT_RATIO: Final = 0.4
_MAX_OFFSET_RATIO: Final = 0.6
_MIN_BAND_PX: Final = 24
_FONT_RATIO: Final = 0.4
_MIN_FONT_PX: Final = 12
_LINE_SPACING: Final = 8
_COLOR: Final = re.compile(r"^[A-Za-z0-9#@.]+$")


class AddBannerResult(TypedDict):
    """Vídeo gerado com a faixa."""

    output: str
    text: str
    position: BannerPosition
    color: str
    band_height_px: int
    offset_px: int
    font_size: int
    start: float
    end: float | None


def _validate_color(name: str, value: str) -> str:
    value = value.strip()
    if not value or not _COLOR.match(value):
        raise ToolError(
            f"{name}='{value}' não é uma cor válida.",
            code="invalid_argument",
            hint="Use um nome (blue, white), hexadecimal (#1E40AF) ou com opacidade (blue@0.8).",
        )
    return value


def _validate(
    *,
    text: str,
    position: BannerPosition,
    height_ratio: float,
    offset_ratio: float,
    font_size: int | None,
    start: float,
    end: float | None,
) -> str:
    text = text.strip()
    if not text or len(text) > _MAX_TEXT:
        raise ToolError(
            f"text deve ter entre 1 e {_MAX_TEXT} caracteres.",
            code="invalid_argument",
            hint="Encurte o texto ou quebre em linhas com \\n.",
        )
    if position not in _POSITIONS:
        raise ToolError(
            f"position='{position}' inválida.",
            code="invalid_argument",
            hint="Use top ou bottom.",
        )
    if not _MIN_HEIGHT_RATIO <= height_ratio <= _MAX_HEIGHT_RATIO:
        raise ToolError(
            f"height_ratio deve estar entre {_MIN_HEIGHT_RATIO} e {_MAX_HEIGHT_RATIO}.",
            code="invalid_argument",
            hint="Use algo como 0.06 para uma faixa fina ou 0.12 para uma faixa grossa.",
        )
    if not 0 <= offset_ratio <= _MAX_OFFSET_RATIO:
        raise ToolError(
            f"offset_ratio deve estar entre 0 e {_MAX_OFFSET_RATIO}.",
            code="invalid_argument",
            hint="Use 0 para colar na borda ou 0.15 para escapar da interface do app.",
        )
    if font_size is not None and font_size <= 0:
        raise ToolError("font_size deve ser maior que zero.", code="invalid_argument")
    if start < 0 or (end is not None and end <= start):
        raise ToolError(
            f"Intervalo inválido: start={start}, end={end}.",
            code="invalid_argument",
            hint="start deve ser >= 0 e end maior que start, ou omitido para ir até o fim.",
        )
    return text


def _enable(start: float, end: float | None) -> str:
    if start == 0 and end is None:
        return ""
    expr = f"between(t\\,{start}\\,{end})" if end is not None else f"gte(t\\,{start})"
    return f":enable='{expr}'"


def _do_banner(
    runtime: Runtime,
    path: str,
    text: str,
    *,
    position: BannerPosition,
    color: str,
    text_color: str,
    height_ratio: float,
    offset_ratio: float,
    font_size: int | None,
    start: float,
    end: float | None,
) -> AddBannerResult:
    source = runtime.workspace.existing(path)
    text = _validate(
        text=text,
        position=position,
        height_ratio=height_ratio,
        offset_ratio=offset_ratio,
        font_size=font_size,
        start=start,
        end=end,
    )
    color = _validate_color("color", color)
    text_color = _validate_color("text_color", text_color)
    info = runtime.ffmpeg.probe(source)
    if not info["has_video"] or info["height"] is None:
        raise ToolError(
            f"'{path}' não tem trilha de vídeo.",
            code="invalid_argument",
            hint="A faixa só se aplica a vídeos.",
        )
    if start >= info["duration"]:
        raise ToolError(
            f"start={start}s ultrapassa a duração do vídeo ({info['duration']:.2f}s).",
            code="invalid_argument",
            hint="Use probe_video para conferir a duração.",
        )
    band = max(int(info["height"] * height_ratio), _MIN_BAND_PX)
    offset = int(info["height"] * offset_ratio)
    size = font_size if font_size is not None else max(int(band * _FONT_RATIO), _MIN_FONT_PX)
    enable = _enable(start, end)
    box_y = f"{offset}" if position == "top" else f"ih-{band}-{offset}"
    band_top = f"{offset}" if position == "top" else f"h-{band}-{offset}"
    filters = [f"drawbox=x=0:y={box_y}:w=iw:h={band}:color={color}:t=fill{enable}"]
    # Uma chamada de drawtext por linha: o drawtext centraliza o bloco inteiro,
    # mas alinha as linhas pela esquerda. Assim cada linha fica centralizada.
    lines = [line.strip() for line in text.split("\n") if line.strip()]
    block = len(lines) * size + (len(lines) - 1) * _LINE_SPACING
    font = escape_filter_path(require_font())
    for index, line in enumerate(lines):
        y = f"{band_top}+({band}-{block})/2+{index * (size + _LINE_SPACING)}+({size}-text_h)/2"
        filters.append(
            f"drawtext=fontfile='{font}':text={escape_drawtext(line)}"
            f":fontsize={size}:fontcolor={text_color}:x=(w-text_w)/2:y={y}{enable}"
        )
    output = runtime.workspace.output_for(source, "banner")
    audio_args = ["-c:a", "copy"] if info["has_audio"] else []
    runtime.ffmpeg.run(
        [
            "-i",
            str(source),
            "-vf",
            ",".join(filters),
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            *audio_args,
            str(output),
        ]
    )
    return AddBannerResult(
        output=runtime.workspace.relative(output),
        text=text,
        position=position,
        color=color,
        band_height_px=band,
        offset_px=offset,
        font_size=size,
        start=start,
        end=end,
    )


@guarded
def add_banner(
    runtime: Runtime,
    path: str,
    text: str,
    *,
    position: BannerPosition = "top",
    color: str = "blue",
    text_color: str = "white",
    height_ratio: float = 0.07,
    offset_ratio: float = 0.0,
    font_size: int | None = None,
    start: float = 0.0,
    end: float | None = None,
    background: bool = False,
) -> AddBannerResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "add_banner",
            lambda: _do_banner(
                runtime,
                path,
                text,
                position=position,
                color=color,
                text_color=text_color,
                height_ratio=height_ratio,
                offset_ratio=offset_ratio,
                font_size=font_size,
                start=start,
                end=end,
            ),
        )
    return _do_banner(
        runtime,
        path,
        text,
        position=position,
        color=color,
        text_color=text_color,
        height_ratio=height_ratio,
        offset_ratio=offset_ratio,
        font_size=font_size,
        start=start,
        end=end,
    )


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="add_banner")
    def _tool(  # noqa: PLR0917  # a assinatura da tool é a interface do agente
        path: str,
        text: str,
        position: BannerPosition = "top",
        color: str = "blue",
        text_color: str = "white",
        height_ratio: float = 0.07,
        offset_ratio: float = 0.0,
        font_size: int | None = None,
        start: float = 0.0,
        end: float | None = None,
        background: bool = False,
    ) -> AddBannerResult | JobSubmitted | ErrorPayload:
        r"""Desenha uma faixa de fundo colorido com texto centralizado no topo ou no rodapé.

        Use para avisos fixos, chamadas de página ou créditos que precisam de
        fundo sólido para leitura, como "Só artistas sem auto-tune". A faixa
        ocupa toda a largura, com altura proporcional ao vídeo (height_ratio).
        Em vídeos verticais para TikTok, Reels e Shorts, a interface do app cobre
        cerca de 12% do topo e 25% do rodapé: use offset_ratio para afastar a
        faixa da borda e mantê-la visível.
        Para texto sem fundo use add_text_overlay; para falas use burn_subtitles.
        O original não é modificado; o vídeo é re-encodado e ganha o sufixo _banner.

        Args:
            path: Vídeo, relativo ao workspace.
            text: Texto da faixa. Até 300 caracteres; use \\n para quebrar linhas.
            position: top ou bottom.
            color: Cor de fundo: nome (blue), hexadecimal (#1E40AF) ou com opacidade (blue@0.8).
            text_color: Cor do texto, no mesmo formato.
            height_ratio: Altura da faixa como fração da altura do vídeo (0.03 a 0.4).
            offset_ratio: Distância da borda até a faixa, como fração da altura (0 a 0.6).
                Use 0.15 no topo para ficar abaixo da interface do TikTok.
            font_size: Tamanho da fonte em pixels. Omitido = 40% da altura da faixa.
            start: Segundo em que a faixa aparece.
            end: Segundo em que a faixa some. Omitido = até o fim.
            background: Executa como job e devolve job_id.
        """
        return add_banner(
            runtime,
            path,
            text,
            position=position,
            color=color,
            text_color=text_color,
            height_ratio=height_ratio,
            offset_ratio=offset_ratio,
            font_size=font_size,
            start=start,
            end=end,
            background=background,
        )
