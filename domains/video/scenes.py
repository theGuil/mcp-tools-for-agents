"""Tool ``detect_scenes``: encontra mudanças de cena em um vídeo."""

from __future__ import annotations

import re
from itertools import pairwise
from typing import TYPE_CHECKING, Final, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

_PTS_TIME: Final = re.compile(r"pts_time:(?P<t>[0-9]+(?:\.[0-9]+)?)")
_DEFAULT_THRESHOLD: Final = 0.4


class Scene(TypedDict):
    """Um trecho contínuo entre duas mudanças de cena."""

    index: int
    start: float
    end: float
    duration: float


class DetectScenesResult(TypedDict):
    """Cenas encontradas em um vídeo."""

    path: str
    threshold: float
    scenes: list[Scene]
    count: int


def _do_detect(runtime: Runtime, path: str, threshold: float) -> DetectScenesResult:
    if not 0.0 < threshold <= 1.0:
        raise ToolError(
            f"threshold={threshold} fora do intervalo (0, 1].",
            code="invalid_argument",
            hint="Valores típicos: 0.3 (sensível) a 0.6 (só cortes bruscos).",
        )
    source = runtime.workspace.existing(path)
    info = runtime.ffmpeg.probe(source)
    stderr = runtime.ffmpeg.run(
        [
            "-i",
            str(source),
            "-vf",
            f"select='gt(scene,{threshold})',showinfo",
            "-an",
            "-f",
            "null",
            "-",
        ]
    )
    cuts = sorted({float(m.group("t")) for m in _PTS_TIME.finditer(stderr)})
    boundaries = [0.0, *cuts, info["duration"]]
    scenes = [
        Scene(index=i, start=round(a, 3), end=round(b, 3), duration=round(b - a, 3))
        for i, (a, b) in enumerate(pairwise(boundaries))
        if b > a
    ]
    return DetectScenesResult(
        path=runtime.workspace.relative(source),
        threshold=threshold,
        scenes=scenes,
        count=len(scenes),
    )


@guarded
def detect_scenes(
    runtime: Runtime,
    path: str,
    *,
    threshold: float = _DEFAULT_THRESHOLD,
    background: bool = False,
) -> DetectScenesResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit("detect_scenes", lambda: _do_detect(runtime, path, threshold))
    return _do_detect(runtime, path, threshold)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="detect_scenes")
    def _tool(
        path: str,
        threshold: float = _DEFAULT_THRESHOLD,
        background: bool = False,
    ) -> DetectScenesResult | JobSubmitted | ErrorPayload:
        """Detecta mudanças de cena e devolve os intervalos de cada cena.

        Use para decidir onde cortar. Percorre o vídeo inteiro, então em vídeos
        longos prefira background=true.

        Args:
            path: Vídeo, relativo ao workspace.
            threshold: Sensibilidade entre 0 e 1. Menor detecta mais cenas.
            background: Executa como job e devolve job_id.
        """
        return detect_scenes(runtime, path, threshold=threshold, background=background)
