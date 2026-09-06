"""Tools ``list_templates`` e ``apply_template``: formatos prontos para redes sociais."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final, Literal, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.fonts import escape_drawtext, escape_filter_path, require_font
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from pathlib import Path

    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

type TemplateName = Literal["shorts", "square", "landscape", "intro_title", "watermark"]

_MAX_TITLE: Final = 120
_INTRO_SECONDS: Final = 3.0


class TemplateInfo(TypedDict):
    """Descrição de um template para o agente escolher."""

    name: TemplateName
    description: str
    output_size: str
    requires_title: bool
    requires_logo: bool


TEMPLATES: Final[tuple[TemplateInfo, ...]] = (
    TemplateInfo(
        name="shorts",
        description="Vertical 9:16 para Shorts, Reels e TikTok. O vídeo fica centralizado "
        "sobre uma versão desfocada dele mesmo. title opcional no topo.",
        output_size="1080x1920",
        requires_title=False,
        requires_logo=False,
    ),
    TemplateInfo(
        name="square",
        description="Quadrado 1:1 para feed. Fundo desfocado, title opcional no topo.",
        output_size="1080x1080",
        requires_title=False,
        requires_logo=False,
    ),
    TemplateInfo(
        name="landscape",
        description="Horizontal 16:9 Full HD. Redimensiona e preenche bordas com preto.",
        output_size="1920x1080",
        requires_title=False,
        requires_logo=False,
    ),
    TemplateInfo(
        name="intro_title",
        description="Mantém o tamanho e mostra o title em destaque, sobre faixa escura, "
        "nos 3 primeiros segundos.",
        output_size="original",
        requires_title=True,
        requires_logo=False,
    ),
    TemplateInfo(
        name="watermark",
        description="Mantém o tamanho e coloca a imagem logo_path no canto inferior direito.",
        output_size="original",
        requires_title=False,
        requires_logo=True,
    ),
)

_TEMPLATE_BY_NAME: Final = {t["name"]: t for t in TEMPLATES}
_CANVAS: Final[dict[str, tuple[int, int]]] = {
    "shorts": (1080, 1920),
    "square": (1080, 1080),
    "landscape": (1920, 1080),
}


class ListTemplatesResult(TypedDict):
    """Templates disponíveis."""

    templates: list[TemplateInfo]


class ApplyTemplateResult(TypedDict):
    """Vídeo gerado pelo template."""

    output: str
    template: TemplateName
    width: int
    height: int
    title: str | None


@guarded
def list_templates(runtime: Runtime) -> ListTemplatesResult:
    """Implementação pura, testável sem MCP."""
    del runtime
    return ListTemplatesResult(templates=list(TEMPLATES))


def _title_filter(title: str, font: Path, *, font_size: int, y: str, enable: str | None) -> str:
    expr = (
        f"drawtext=fontfile='{escape_filter_path(font)}':text={escape_drawtext(title)}"
        f":fontsize={font_size}:fontcolor=white:borderw=3:bordercolor=black@0.8"
        f":x=(w-text_w)/2:y={y}:line_spacing=10"
    )
    if enable is not None:
        expr += f":enable='{enable}'"
    return expr


def _blurred_canvas_filter(width: int, height: int, title: str | None) -> str:
    chain = (
        f"[0:v]split[bg][fg];"
        f"[bg]scale={width}:{height}:force_original_aspect_ratio=increase,"
        f"crop={width}:{height},boxblur=20:5[bgb];"
        f"[fg]scale={width}:{height}:force_original_aspect_ratio=decrease[fgs];"
        f"[bgb][fgs]overlay=(W-w)/2:(H-h)/2"
    )
    if title:
        chain += "," + _title_filter(
            title, require_font(), font_size=int(height * 0.035), y="h*0.06", enable=None
        )
    return chain + ",format=yuv420p[vout]"


def _landscape_filter(width: int, height: int, title: str | None) -> str:
    chain = (
        f"[0:v]scale={width}:{height}:force_original_aspect_ratio=decrease,"
        f"pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black"
    )
    if title:
        chain += "," + _title_filter(
            title, require_font(), font_size=int(height * 0.05), y="h*0.06", enable=None
        )
    return chain + ",format=yuv420p[vout]"


def _intro_filter(title: str, height: int) -> str:
    box = (
        f"drawbox=x=0:y=ih*0.35:w=iw:h=ih*0.3:color=black@0.6:t=fill"
        f":enable='lte(t\\,{_INTRO_SECONDS})'"
    )
    text = _title_filter(
        title,
        require_font(),
        font_size=max(int(height * 0.07), 24),
        y="(h-text_h)/2",
        enable=f"lte(t\\,{_INTRO_SECONDS})",
    )
    return f"[0:v]{box},{text},format=yuv420p[vout]"


def _watermark_filter(height: int) -> str:
    logo_h = max(int(height * 0.08), 24)
    return (
        f"[1:v]scale=-1:{logo_h}[logo];"
        f"[0:v][logo]overlay=W-w-{int(height * 0.03)}:H-h-{int(height * 0.03)},format=yuv420p[vout]"
    )


def _validate(template: TemplateName, title: str | None, logo_path: str | None) -> TemplateInfo:
    spec = _TEMPLATE_BY_NAME.get(template)
    if spec is None:
        raise ToolError(
            f"Template '{template}' não existe.",
            code="invalid_argument",
            hint="Use list_templates para ver os nomes válidos.",
        )
    if spec["requires_title"] and not (title and title.strip()):
        raise ToolError(
            f"Template '{template}' exige title.",
            code="invalid_argument",
            hint="Passe o texto do título em title.",
        )
    if title is not None and len(title) > _MAX_TITLE:
        raise ToolError(
            f"title deve ter até {_MAX_TITLE} caracteres.",
            code="invalid_argument",
            hint="Encurte o título ou use add_text_overlay para textos longos.",
        )
    if spec["requires_logo"] and not logo_path:
        raise ToolError(
            f"Template '{template}' exige logo_path.",
            code="invalid_argument",
            hint="Informe uma imagem PNG do workspace em logo_path.",
        )
    return spec


def _do_apply(
    runtime: Runtime, path: str, template: TemplateName, title: str | None, logo_path: str | None
) -> ApplyTemplateResult:
    source = runtime.workspace.existing(path)
    _validate(template, title, logo_path)
    title = title.strip() if title else None
    info = runtime.ffmpeg.probe(source)
    if not info["has_video"] or info["width"] is None or info["height"] is None:
        raise ToolError(
            f"'{path}' não tem trilha de vídeo.",
            code="invalid_argument",
            hint="Templates só se aplicam a vídeos.",
        )
    inputs = ["-i", str(source)]
    if template in _CANVAS:
        width, height = _CANVAS[template]
        filter_expr = (
            _landscape_filter(width, height, title)
            if template == "landscape"
            else _blurred_canvas_filter(width, height, title)
        )
    elif template == "intro_title":
        width, height = info["width"], info["height"]
        filter_expr = _intro_filter(title or "", height)
    else:
        width, height = info["width"], info["height"]
        logo = runtime.workspace.existing(logo_path or "")
        inputs += ["-i", str(logo)]
        filter_expr = _watermark_filter(height)
    output = runtime.workspace.output_for(source, template)
    audio_args = ["-map", "0:a?", "-c:a", "copy"] if info["has_audio"] else ["-an"]
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
            str(output),
        ]
    )
    return ApplyTemplateResult(
        output=runtime.workspace.relative(output),
        template=template,
        width=width,
        height=height,
        title=title,
    )


@guarded
def apply_template(
    runtime: Runtime,
    path: str,
    template: TemplateName,
    *,
    title: str | None = None,
    logo_path: str | None = None,
    background: bool = False,
) -> ApplyTemplateResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "apply_template", lambda: _do_apply(runtime, path, template, title, logo_path)
        )
    return _do_apply(runtime, path, template, title, logo_path)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe as tools no servidor."""

    @mcp.tool(name="list_templates")
    def _list() -> ListTemplatesResult | ErrorPayload:
        """Lista os templates visuais disponíveis para apply_template, com o que cada um exige."""
        return list_templates(runtime)

    @mcp.tool(name="apply_template")
    def _apply(
        path: str,
        template: TemplateName,
        title: str | None = None,
        logo_path: str | None = None,
        background: bool = False,
    ) -> ApplyTemplateResult | JobSubmitted | ErrorPayload:
        """Aplica um template visual pronto ao vídeo: formato de rede social, título ou marca.

        Templates: shorts (9:16), square (1:1), landscape (16:9), intro_title
        (título em destaque nos 3 primeiros segundos) e watermark (logo no canto).
        Veja detalhes com list_templates. Templates podem ser encadeados: aplique
        shorts e depois watermark, por exemplo. O original não é modificado.

        Args:
            path: Vídeo, relativo ao workspace.
            template: shorts, square, landscape, intro_title ou watermark.
            title: Texto do título. Obrigatório em intro_title, opcional nos formatos.
            logo_path: Imagem PNG do workspace. Obrigatório em watermark.
            background: Executa como job e devolve job_id.
        """
        return apply_template(
            runtime, path, template, title=title, logo_path=logo_path, background=background
        )
