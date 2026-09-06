"""Tool ``delete_file``: remove um arquivo do workspace."""

from __future__ import annotations

from typing import TYPE_CHECKING, TypedDict

from core.errors import ErrorPayload, guarded

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


class DeleteFileResult(TypedDict):
    """Confirmação da remoção."""

    deleted: str
    freed_bytes: int


@guarded
def delete_file(runtime: Runtime, path: str) -> DeleteFileResult:
    """Implementação pura, testável sem MCP."""
    target = runtime.workspace.existing(path)
    size = target.stat().st_size
    target.unlink()
    return DeleteFileResult(deleted=runtime.workspace.relative(target), freed_bytes=size)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="delete_file")
    def _tool(path: str) -> DeleteFileResult | ErrorPayload:
        """Apaga permanentemente um arquivo do workspace.

        Use para limpar saídas intermediárias. Não há lixeira nem desfazer.

        Args:
            path: Arquivo, relativo ao workspace.
        """
        return delete_file(runtime, path)
