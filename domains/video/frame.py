"""Tool ``extract_frame``: salva um frame do vídeo como imagem."""

from __future__ import annotations

from typing import TYPE_CHECKING, Literal, TypedDict

from core.errors import ErrorPayload, ToolError, guarded

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

type ImageFormat = Literal["png", "jpg"]


class ExtractFrameResult(TypedDict):
    """Imagem gerada."""

    output: str
    time: float
    format: ImageFormat
    size_bytes: int


@guarded
def extract_frame(
    runtime: Runtime, path: str, time: float, *, image_format: ImageFormat = "png"
) -> ExtractFrameResult:
    """Implementação pura, testável sem MCP."""
    source = runtime.workspace.existing(path)
    if time < 0:
        raise ToolError("time deve ser >= 0.", code="invalid_argument")
    info = runtime.ffmpeg.probe(source)
    if time > info["duration"]:
        raise ToolError(
            f"time={time}s ultrapassa a duração ({info['duration']:.2f}s).",
            code="invalid_argument",
        )
    output = runtime.workspace.output_for(source, f"frame_{time:g}", extension=image_format)
    runtime.ffmpeg.run(["-ss", f"{time}", "-i", str(source), "-frames:v", "1", str(output)])
    return ExtractFrameResult(
        output=runtime.workspace.relative(output),
        time=time,
        format=image_format,
        size_bytes=output.stat().st_size,
    )


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="extract_frame")
    def _tool(
        path: str, time: float, image_format: ImageFormat = "png"
    ) -> ExtractFrameResult | ErrorPayload:
        """Extrai um único frame do vídeo no instante informado e salva como imagem.

        Útil para conferir visualmente o conteúdo antes de cortar.

        Args:
            path: Vídeo, relativo ao workspace.
            time: Instante em segundos.
            image_format: png ou jpg.
        """
        return extract_frame(runtime, path, time, image_format=image_format)
