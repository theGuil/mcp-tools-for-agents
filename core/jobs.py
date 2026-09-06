"""Tarefas longas executadas em background.

Cortes e re-encodes podem levar minutos. Em vez de travar o agente, a tool
devolve um ``job_id`` e o agente consulta o status quando quiser.
"""

from __future__ import annotations

import threading
import time
import uuid
from concurrent.futures import Future, ThreadPoolExecutor
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Literal, NewType, TypedDict

from core.errors import ToolError

if TYPE_CHECKING:
    from collections.abc import Callable, Mapping

JobId = NewType("JobId", str)
type JobStatus = Literal["pending", "running", "done", "failed"]
type JobResult = Mapping[str, object]


class JobSubmitted(TypedDict):
    """Resposta de uma tool que foi enfileirada em background."""

    job_id: str
    status: JobStatus
    tool: str


class JobStatusPayload(TypedDict):
    """Estado de um job para o agente acompanhar."""

    job_id: str
    tool: str
    status: JobStatus
    elapsed_seconds: float
    error: str | None


@dataclass(slots=True)
class Job:
    """Registro mutável de uma tarefa em execução."""

    id: JobId
    tool: str
    created_at: float = field(default_factory=time.monotonic)
    finished_at: float | None = None
    status: JobStatus = "pending"
    result: JobResult | None = None
    error: str | None = None

    def elapsed(self) -> float:
        """Segundos entre criação e término, ou até agora se ainda roda."""
        end = self.finished_at if self.finished_at is not None else time.monotonic()
        return round(end - self.created_at, 3)

    def to_payload(self) -> JobStatusPayload:
        """Serializa o estado para o agente."""
        return JobStatusPayload(
            job_id=self.id,
            tool=self.tool,
            status=self.status,
            elapsed_seconds=self.elapsed(),
            error=self.error,
        )


class JobManager:
    """Fila de jobs em threads, com registro em memória."""

    def __init__(self, workers: int = 2) -> None:
        """Cria o gerenciador com ``workers`` threads concorrentes."""
        self._executor = ThreadPoolExecutor(max_workers=workers, thread_name_prefix="job")
        self._jobs: dict[JobId, Job] = {}
        self._lock = threading.Lock()

    def submit(self, tool: str, fn: Callable[[], JobResult]) -> JobSubmitted:
        """Agenda ``fn`` e devolve imediatamente o ``job_id``."""
        job = Job(id=JobId(uuid.uuid4().hex[:12]), tool=tool)
        with self._lock:
            self._jobs[job.id] = job
        future: Future[JobResult] = self._executor.submit(self._run, job, fn)
        future.add_done_callback(lambda _: None)
        return JobSubmitted(job_id=job.id, status="pending", tool=tool)

    def get(self, job_id: str) -> Job:
        """Busca um job pelo id.

        Raises:
            ToolError: Se o id não existir.
        """
        with self._lock:
            job = self._jobs.get(JobId(job_id))
        if job is None:
            raise ToolError(
                f"Job '{job_id}' não encontrado.",
                code="job_not_found",
                hint="O id vem da resposta da tool que criou o job.",
            )
        return job

    def result(self, job_id: str) -> JobResult:
        """Resultado de um job concluído.

        Raises:
            ToolError: Se o job não existir, ainda rodar ou tiver falhado.
        """
        job = self.get(job_id)
        if job.status in {"pending", "running"}:
            raise ToolError(
                f"Job '{job_id}' ainda está {job.status}.",
                code="job_not_finished",
                hint="Consulte job_status e tente novamente em alguns segundos.",
            )
        if job.status == "failed" or job.result is None:
            raise ToolError(
                f"Job '{job_id}' falhou: {job.error or 'sem detalhes'}",
                code="ffmpeg_failed",
            )
        return job.result

    def wait(self, job_id: str, timeout: float | None = None) -> Job:
        """Bloqueia até o job terminar. Útil em testes."""
        job = self.get(job_id)
        deadline = None if timeout is None else time.monotonic() + timeout
        while job.status in {"pending", "running"}:
            if deadline is not None and time.monotonic() > deadline:
                break
            time.sleep(0.01)
        return job

    def shutdown(self) -> None:
        """Encerra as threads sem esperar jobs pendentes."""
        self._executor.shutdown(wait=False, cancel_futures=True)

    @staticmethod
    def _run(job: Job, fn: Callable[[], JobResult]) -> JobResult:
        job.status = "running"
        try:
            result = fn()
        except ToolError as exc:
            job.status = "failed"
            job.error = exc.message
            job.finished_at = time.monotonic()
            raise
        except Exception as exc:
            job.status = "failed"
            job.error = f"{type(exc).__name__}: {exc}"
            job.finished_at = time.monotonic()
            raise
        job.result = result
        job.status = "done"
        job.finished_at = time.monotonic()
        return result
