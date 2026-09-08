"""Tool ``export_for_platform``: codifica o vídeo final com o preset da plataforma."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final, Literal, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

type Platform = Literal[
    "tiktok", "instagram_reels", "youtube_shorts", "youtube", "youtube_4k", "twitter"
]
type Quality = Literal["standard", "high"]
type Orientation = Literal["vertical", "horizontal", "any"]


class PlatformPreset(TypedDict):
    """Regras de codificação de uma plataforma."""

    name: Platform
    description: str
    max_width: int
    max_height: int
    max_fps: float
    crf: int
    audio_bitrate: str
    orientation: Orientation
    max_seconds: float | None


PRESETS: Final[dict[Platform, PlatformPreset]] = {
    "tiktok": PlatformPreset(
        name="tiktok",
        description="Vertical 1080x1920, H.264 High, 30 fps, AAC 192k, até 10 min.",
        max_width=1080,
        max_height=1920,
        max_fps=30.0,
        crf=20,
        audio_bitrate="192k",
        orientation="vertical",
        max_seconds=600.0,
    ),
    "instagram_reels": PlatformPreset(
        name="instagram_reels",
        description="Vertical 1080x1920, H.264 High, 30 fps, AAC 192k, até 3 min.",
        max_width=1080,
        max_height=1920,
        max_fps=30.0,
        crf=20,
        audio_bitrate="192k",
        orientation="vertical",
        max_seconds=180.0,
    ),
    "youtube_shorts": PlatformPreset(
        name="youtube_shorts",
        description="Vertical 1080x1920, H.264 High, até 60 fps, AAC 192k, até 3 min.",
        max_width=1080,
        max_height=1920,
        max_fps=60.0,
        crf=19,
        audio_bitrate="192k",
        orientation="vertical",
        max_seconds=180.0,
    ),
    "youtube": PlatformPreset(
        name="youtube",
        description="Horizontal até 1920x1080, H.264 High, até 60 fps, AAC 256k.",
        max_width=1920,
        max_height=1080,
        max_fps=60.0,
        crf=18,
        audio_bitrate="256k",
        orientation="horizontal",
        max_seconds=None,
    ),
    "youtube_4k": PlatformPreset(
        name="youtube_4k",
        description="Horizontal até 3840x2160, H.264 High, até 60 fps, AAC 256k.",
        max_width=3840,
        max_height=2160,
        max_fps=60.0,
        crf=18,
        audio_bitrate="256k",
        orientation="horizontal",
        max_seconds=None,
    ),
    "twitter": PlatformPreset(
        name="twitter",
        description="Qualquer orientação até 1920x1200, H.264, 30 fps, AAC 128k, até 140 s.",
        max_width=1920,
        max_height=1200,
        max_fps=30.0,
        crf=21,
        audio_bitrate="128k",
        orientation="any",
        max_seconds=140.0,
    ),
}
_MIN_SHORT_SIDE: Final = 720
_QUALITY_CRF_DELTA: Final[dict[Quality, int]] = {"standard": 0, "high": -3}


class ExportForPlatformResult(TypedDict):
    """Vídeo final exportado."""

    output: str
    platform: Platform
    width: int
    height: int
    fps: float
    duration: float
    size_bytes: int
    warnings: list[str]


def fit_size(width: int, height: int, max_width: int, max_height: int) -> tuple[int, int]:
    """Reduz (nunca amplia) para caber na caixa, mantendo a proporção e dimensões pares."""
    factor = min(1.0, max_width / width, max_height / height)
    new_w = round(width * factor)
    new_h = round(height * factor)
    return new_w - new_w % 2, new_h - new_h % 2


def _warnings(preset: PlatformPreset, *, width: int, height: int, duration: float) -> list[str]:
    notes: list[str] = []
    if preset["orientation"] == "vertical" and width > height:
        notes.append(
            "O vídeo é horizontal, mas a plataforma é vertical. Use smart_crop "
            "(preenche a tela seguindo o rosto) ou apply_template 'shorts' antes de exportar."
        )
    if preset["orientation"] == "horizontal" and height > width:
        notes.append(
            "O vídeo é vertical, mas o preset é horizontal. Para Shorts use youtube_shorts; "
            "para manter, aplique apply_template 'landscape' antes."
        )
    if preset["max_seconds"] is not None and duration > preset["max_seconds"]:
        notes.append(
            f"Duração de {duration:.0f}s passa do limite de {preset['max_seconds']:.0f}s da "
            "plataforma. Use cut_video para encurtar."
        )
    if min(width, height) < _MIN_SHORT_SIDE:
        notes.append(
            f"Resolução baixa ({width}x{height}); a plataforma pode mostrar em qualidade "
            "reduzida. A exportação nunca amplia a imagem."
        )
    return notes


def _do_export(
    runtime: Runtime,
    path: str,
    platform: Platform,
    *,
    quality: Quality,
    output: str | None,
) -> ExportForPlatformResult:
    source = runtime.workspace.existing(path)
    preset = PRESETS.get(platform)
    if preset is None:
        raise ToolError(
            f"platform '{platform}' não existe.",
            code="invalid_argument",
            hint=f"Use uma de: {', '.join(PRESETS)}.",
        )
    if quality not in _QUALITY_CRF_DELTA:
        raise ToolError("quality deve ser 'standard' ou 'high'.", code="invalid_argument")
    info = runtime.ffmpeg.probe(source)
    if not info["has_video"] or info["width"] is None or info["height"] is None:
        raise ToolError(
            f"'{path}' não tem trilha de vídeo.",
            code="invalid_argument",
            hint="export_for_platform só se aplica a vídeos.",
        )
    width, height = fit_size(
        info["width"], info["height"], preset["max_width"], preset["max_height"]
    )
    fps = min(info["fps"] or preset["max_fps"], preset["max_fps"])
    target = (
        runtime.workspace.resolve(output)
        if output
        else runtime.workspace.output_for(source, platform, extension="mp4")
    )
    if target.suffix.lower() != ".mp4":
        raise ToolError(
            f"output '{output}' deve terminar em .mp4.",
            code="invalid_argument",
            hint="Todas as plataformas aceitam MP4 H.264.",
        )
    target.parent.mkdir(parents=True, exist_ok=True)
    crf = preset["crf"] + _QUALITY_CRF_DELTA[quality]
    video_filter = f"scale={width}:{height}:flags=lanczos,fps={fps:g},format=yuv420p"
    audio_args = (
        ["-c:a", "aac", "-b:a", preset["audio_bitrate"], "-ar", "48000"]
        if info["has_audio"]
        else ["-an"]
    )
    runtime.ffmpeg.run(
        [
            "-i",
            str(source),
            "-vf",
            video_filter,
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-profile:v",
            "high",
            "-level",
            "4.2",
            "-crf",
            str(crf),
            "-g",
            str(round(fps * 2)),
            *audio_args,
            "-movflags",
            "+faststart",
            str(target),
        ]
    )
    return ExportForPlatformResult(
        output=runtime.workspace.relative(target),
        platform=platform,
        width=width,
        height=height,
        fps=round(fps, 3),
        duration=round(info["duration"], 3),
        size_bytes=target.stat().st_size,
        warnings=_warnings(preset, width=width, height=height, duration=info["duration"]),
    )


@guarded
def export_for_platform(
    runtime: Runtime,
    path: str,
    platform: Platform,
    *,
    quality: Quality = "standard",
    output: str | None = None,
    background: bool = False,
) -> ExportForPlatformResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "export_for_platform",
            lambda: _do_export(runtime, path, platform, quality=quality, output=output),
        )
    return _do_export(runtime, path, platform, quality=quality, output=output)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="export_for_platform")
    def _tool(
        path: str,
        platform: Platform,
        quality: Quality = "standard",
        output: str | None = None,
        background: bool = False,
    ) -> ExportForPlatformResult | JobSubmitted | ErrorPayload:
        """Exporta o vídeo final no formato que a plataforma recomenda, pronto para subir.

        Último passo da edição. Aplica o preset de codec, resolução máxima, fps,
        qualidade (CRF) e bitrate de áudio da plataforma, gera MP4 H.264 High com
        faststart (começa a tocar antes de baixar). Nunca amplia a imagem nem muda a
        proporção: para virar vertical use smart_crop ou apply_template antes.
        Devolve warnings quando algo vai contra as regras da plataforma (orientação
        errada, duração acima do limite, resolução baixa), com a tool que resolve.

        Presets: tiktok e instagram_reels (1080x1920, 30 fps), youtube_shorts
        (1080x1920, 60 fps), youtube (1920x1080), youtube_4k (3840x2160), twitter
        (1920x1200, 140 s). Para volume padronizado rode normalize_audio antes.

        Args:
            path: Vídeo, relativo ao workspace.
            platform: tiktok, instagram_reels, youtube_shorts, youtube, youtube_4k ou twitter.
            quality: "standard" (equilíbrio tamanho/qualidade) ou "high" (arquivo maior).
            output: Caminho do .mp4 final. Gerado ao lado do original se omitido.
            background: Executa como job e devolve job_id.
        """
        return export_for_platform(
            runtime, path, platform, quality=quality, output=output, background=background
        )
