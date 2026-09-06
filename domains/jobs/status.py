"""Tool ``job_status``: consulta o andamento de um job."""

from __future__ import annotations

from typing import TYPE_CHECKING

from core.errors import ErrorPayload, guarded
from core.jobs import JobStatusPayload

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


@guarded
def job_status(runtime: Runtime, job_id: str) -> JobStatusPayload:
    """Implementação pura, testável sem MCP."""
    return runtime.jobs.get(job_id).to_payload()


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="job_status")
    def _tool(job_id: str) -> JobStatusPayload | ErrorPayload:
        """Informa se um job em background está pending, running, done ou failed.

        Quando estiver done, chame job_result para obter a saída.

        Args:
            job_id: Id devolvido pela tool que iniciou o job.
        """
        return job_status(runtime, job_id)
