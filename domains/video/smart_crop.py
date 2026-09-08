"""Tool ``smart_crop``: reenquadra para vertical/quadrado seguindo o rosto de quem fala."""

from __future__ import annotations

import tempfile
from pathlib import Path
from typing import TYPE_CHECKING, Final, Literal, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted
from core.vision import load_face_detector

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

type CropAspect = Literal["9:16", "1:1", "4:5", "16:9"]
type CropMode = Literal["face", "center"]

_ASPECTS: Final[dict[CropAspect, tuple[int, int]]] = {
    "9:16": (9, 16),
    "1:1": (1, 1),
    "4:5": (4, 5),
    "16:9": (16, 9),
}
_ANALYSIS_WIDTH: Final = 480
_COMMAND_STEP: Final = 0.1
_MIN_INTERVAL: Final = 0.1
_MAX_INTERVAL: Final = 5.0
_DEADZONE_RATIO: Final = 0.04
_SMOOTHING: Final = 0.35


class SmartCropResult(TypedDict):
    """Vídeo gerado com o novo enquadramento."""

    output: str
    aspect: CropAspect
    mode_used: CropMode
    width: int
    height: int
    frames_analyzed: int
    frames_with_face: int


def crop_size(width: int, height: int, aspect: CropAspect) -> tuple[int, int]:
    """Maior janela com a proporção pedida que cabe dentro do vídeo (dimensões pares)."""
    num, den = _ASPECTS[aspect]
    crop_w, crop_h = width, round(width * den / num)
    if crop_h > height:
        crop_h, crop_w = height, round(height * num / den)
    return crop_w - crop_w % 2, crop_h - crop_h % 2


def smooth_positions(
    samples: list[float | None], *, default: float, deadzone: float
) -> list[float]:
    """Suaviza a trajetória do rosto: ignora tremores pequenos e segura a última posição."""
    positions: list[float] = []
    current = next((s for s in samples if s is not None), default)
    for sample in samples:
        if sample is not None:
            delta = sample - current
            if abs(delta) > deadzone:
                current += delta * _SMOOTHING
        positions.append(current)
    return positions


def _extract_frames(runtime: Runtime, source: Path, folder: Path, interval: float) -> list[Path]:
    runtime.ffmpeg.run(
        [
            "-i",
            str(source),
            "-vf",
            f"fps=1/{interval:g},scale={_ANALYSIS_WIDTH}:-2",
            "-q:v",
            "4",
            str(folder / "f%06d.jpg"),
        ]
    )
    return sorted(folder.glob("f*.jpg"))


def _face_track(
    runtime: Runtime, source: Path, *, width: int, interval: float
) -> tuple[list[float | None], int]:
    detector = load_face_detector()
    scale = width / _ANALYSIS_WIDTH
    with tempfile.TemporaryDirectory(prefix="smart_crop_") as tmp:
        frames = _extract_frames(runtime, source, Path(tmp), interval)
        samples: list[float | None] = []
        found = 0
        for frame in frames:
            face = detector.largest_face(frame)
            if face is None:
                samples.append(None)
            else:
                samples.append(face.center_x * scale)
                found += 1
    return samples, found


def _write_commands(positions: list[float], *, interval: float, crop_w: int, width: int) -> str:
    """Arquivo do ``sendcmd``: posição x do crop a cada décimo de segundo, interpolada."""
    lines: list[str] = []
    max_x = width - crop_w
    for index, pos in enumerate(positions):
        nxt = positions[index + 1] if index + 1 < len(positions) else pos
        steps = max(round(interval / _COMMAND_STEP), 1)
        for step in range(steps):
            t = index * interval + step * _COMMAND_STEP
            value = pos + (nxt - pos) * (step / steps)
            x = min(max(round(value - crop_w / 2), 0), max_x)
            lines.append(f"{t:.2f} crop x {x};")
    return "\n".join(lines) + "\n"


def _validate(aspect: CropAspect, mode: CropMode, sample_interval: float) -> None:
    if aspect not in _ASPECTS:
        raise ToolError(
            f"aspect '{aspect}' não existe.",
            code="invalid_argument",
            hint="Use 9:16 (TikTok, Reels, Shorts), 1:1, 4:5 ou 16:9.",
        )
    if mode not in {"face", "center"}:
        raise ToolError("mode deve ser 'face' ou 'center'.", code="invalid_argument")
    if not _MIN_INTERVAL <= sample_interval <= _MAX_INTERVAL:
        raise ToolError(
            f"sample_interval deve estar entre {_MIN_INTERVAL:g} e {_MAX_INTERVAL:g} segundos.",
            code="invalid_argument",
        )


def _command_file(
    runtime: Runtime,
    source: Path,
    *,
    width: int,
    crop_w: int,
    interval: float,
) -> tuple[Path | None, int, int]:
    """Rastreia o rosto e grava o arquivo do sendcmd. Devolve (arquivo, analisados, com rosto)."""
    samples, found = _face_track(runtime, source, width=width, interval=interval)
    if found == 0:
        return None, len(samples), 0
    positions = smooth_positions(samples, default=width / 2, deadzone=width * _DEADZONE_RATIO)
    with tempfile.NamedTemporaryFile("w", suffix=".cmd", delete=False, encoding="utf-8") as handle:
        handle.write(_write_commands(positions, interval=interval, crop_w=crop_w, width=width))
        return Path(handle.name), len(samples), found


def _do_smart_crop(
    runtime: Runtime,
    path: str,
    *,
    aspect: CropAspect,
    mode: CropMode,
    sample_interval: float,
) -> SmartCropResult:
    source = runtime.workspace.existing(path)
    _validate(aspect, mode, sample_interval)
    info = runtime.ffmpeg.probe(source)
    if not info["has_video"] or info["width"] is None or info["height"] is None:
        raise ToolError(
            f"'{path}' não tem trilha de vídeo.",
            code="invalid_argument",
            hint="smart_crop só se aplica a vídeos.",
        )
    width, height = info["width"], info["height"]
    crop_w, crop_h = crop_size(width, height, aspect)
    if crop_w == width and crop_h == height:
        raise ToolError(
            f"O vídeo já está em {aspect} ({width}x{height}); não há o que recortar.",
            code="invalid_argument",
            hint="Use apply_template para adicionar bordas ou fundo desfocado.",
        )
    center_x = (width - crop_w) // 2
    y = (height - crop_h) // 2
    command_file: Path | None = None
    frames_analyzed = frames_with_face = 0
    if mode == "face" and crop_w < width:
        command_file, frames_analyzed, frames_with_face = _command_file(
            runtime, source, width=width, crop_w=crop_w, interval=sample_interval
        )
    mode_used: CropMode = "face" if command_file is not None else "center"
    filter_expr = f"crop={crop_w}:{crop_h}:{center_x}:{y}"
    if command_file is not None:
        escaped = str(command_file).replace("\\", "/").replace(":", "\\:")
        filter_expr = f"sendcmd=f='{escaped}',{filter_expr}"
    output = runtime.workspace.output_for(source, f"crop_{aspect.replace(':', 'x')}")
    audio_args = ["-c:a", "copy"] if info["has_audio"] else []
    try:
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
                "-pix_fmt",
                "yuv420p",
                *audio_args,
                str(output),
            ]
        )
    finally:
        if command_file is not None:
            command_file.unlink(missing_ok=True)
    return SmartCropResult(
        output=runtime.workspace.relative(output),
        aspect=aspect,
        mode_used=mode_used,
        width=crop_w,
        height=crop_h,
        frames_analyzed=frames_analyzed,
        frames_with_face=frames_with_face,
    )


@guarded
def smart_crop(
    runtime: Runtime,
    path: str,
    *,
    aspect: CropAspect = "9:16",
    mode: CropMode = "face",
    sample_interval: float = 0.5,
    background: bool = False,
) -> SmartCropResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "smart_crop",
            lambda: _do_smart_crop(
                runtime, path, aspect=aspect, mode=mode, sample_interval=sample_interval
            ),
        )
    return _do_smart_crop(runtime, path, aspect=aspect, mode=mode, sample_interval=sample_interval)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="smart_crop")
    def _tool(
        path: str,
        aspect: CropAspect = "9:16",
        mode: CropMode = "face",
        sample_interval: float = 0.5,
        background: bool = False,
    ) -> SmartCropResult | JobSubmitted | ErrorPayload:
        """Reenquadra um vídeo horizontal para vertical (9:16) seguindo o rosto de quem fala.

        É o passo que transforma um podcast ou aula gravada em 16:9 em um corte
        para TikTok, Reels ou Shorts sem cortar a pessoa fora do quadro. A tool
        analisa um frame a cada sample_interval segundos, encontra o maior rosto,
        suaviza o movimento da câmera virtual e recorta o vídeo acompanhando.
        Sem rosto detectado (ou com mode="center") recorta o centro. Não adiciona
        bordas: o resultado tem exatamente a proporção pedida com a altura do
        original. Depois use export_for_platform para 1080x1920.

        Diferença para apply_template "shorts": o template mantém o vídeo inteiro
        pequeno sobre um fundo desfocado; smart_crop preenche a tela com a pessoa.

        Requer o extra "vision" (uv sync --extra vision) para mode="face". O
        original não é modificado; o vídeo é re-encodado. Use background=true
        em vídeos longos.

        Args:
            path: Vídeo, relativo ao workspace.
            aspect: Proporção final: 9:16 (padrão), 1:1, 4:5 ou 16:9.
            mode: "face" segue o rosto; "center" recorta o centro fixo.
            sample_interval: Segundos entre frames analisados. Menor = mais preciso e lento.
            background: Executa como job e devolve job_id.
        """
        return smart_crop(
            runtime,
            path,
            aspect=aspect,
            mode=mode,
            sample_interval=sample_interval,
            background=background,
        )
