"""Tool ``job_result``: devolve a saída de um job concluído."""

from __future__ import annotations

from typing import TYPE_CHECKING, TypedDict

from core.errors import ErrorPayload, guarded

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


class JobResultPayload(TypedDict):
    """Saída de um job, no mesmo formato que a tool síncrona devolveria."""

    job_id: str
    tool: str
    result: dict[str, object]


@guarded
def job_result(runtime: Runtime, job_id: str) -> JobResultPayload:
    """Implementação pura, testável sem MCP."""
    job = runtime.jobs.get(job_id)
    return JobResultPayload(job_id=job.id, tool=job.tool, result=dict(runtime.jobs.result(job_id)))


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="job_result")
    def _tool(job_id: str) -> JobResultPayload | ErrorPayload:
        """Obtém o resultado de um job que já terminou.

        Retorna erro job_not_finished se ainda estiver rodando.

        Args:
            job_id: Id devolvido pela tool que iniciou o job.
        """
        return job_result(runtime, job_id)
