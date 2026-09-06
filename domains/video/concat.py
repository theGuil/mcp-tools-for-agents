"""Tool ``concat_videos``: junta vários vídeos em sequência."""

from __future__ import annotations

import tempfile
from pathlib import Path
from typing import TYPE_CHECKING, Final, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

_MIN_INPUTS: Final = 2


class ConcatVideosResult(TypedDict):
    """Arquivo gerado pela concatenação."""

    output: str
    inputs: list[str]
    count: int


def _escape_for_concat(path: Path) -> str:
    return str(path).replace("'", r"'\''")


def _do_concat(runtime: Runtime, paths: list[str], output_name: str | None) -> ConcatVideosResult:
    if len(paths) < _MIN_INPUTS:
        raise ToolError(
            "Informe pelo menos dois vídeos para concatenar.",
            code="invalid_argument",
        )
    sources = [runtime.workspace.existing(p) for p in paths]
    output = (
        runtime.workspace.resolve(output_name)
        if output_name
        else runtime.workspace.output_for(sources[0], f"concat_{len(sources)}")
    )
    output.parent.mkdir(parents=True, exist_ok=True)

    with tempfile.NamedTemporaryFile("w", suffix=".txt", delete=False, encoding="utf-8") as listing:
        listing.writelines(f"file '{_escape_for_concat(s)}'\n" for s in sources)
        list_path = Path(listing.name)
    try:
        runtime.ffmpeg.run(
            ["-f", "concat", "-safe", "0", "-i", str(list_path), "-c", "copy", str(output)]
        )
    finally:
        list_path.unlink(missing_ok=True)

    return ConcatVideosResult(
        output=runtime.workspace.relative(output),
        inputs=[runtime.workspace.relative(s) for s in sources],
        count=len(sources),
    )


@guarded
def concat_videos(
    runtime: Runtime,
    paths: list[str],
    *,
    output_name: str | None = None,
    background: bool = False,
) -> ConcatVideosResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit("concat_videos", lambda: _do_concat(runtime, paths, output_name))
    return _do_concat(runtime, paths, output_name)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="concat_videos")
    def _tool(
        paths: list[str],
        output_name: str | None = None,
        background: bool = False,
    ) -> ConcatVideosResult | JobSubmitted | ErrorPayload:
        """Junta dois ou mais vídeos na ordem informada em um único arquivo.

        Os vídeos devem ter o mesmo codec e resolução (por exemplo, cortes do
        mesmo original). Os arquivos de entrada não são alterados.

        Args:
            paths: Lista de vídeos, relativa ao workspace, na ordem desejada.
            output_name: Nome do arquivo de saída. Gerado automaticamente se omitido.
            background: Executa como job e devolve job_id.
        """
        return concat_videos(runtime, paths, output_name=output_name, background=background)
