import pytest

from core.errors import ToolError
from core.jobs import JobManager


def test_submit_and_result(jobs: JobManager) -> None:
    submitted = jobs.submit("demo", lambda: {"answer": 42})
    assert submitted["status"] == "pending"
    job = jobs.wait(submitted["job_id"], timeout=5)
    assert job.status == "done"
    assert jobs.result(job.id) == {"answer": 42}
    assert job.to_payload()["elapsed_seconds"] >= 0


def test_failed_job(jobs: JobManager) -> None:
    def boom() -> dict[str, object]:
        raise ToolError("quebrou", code="ffmpeg_failed")

    submitted = jobs.submit("demo", boom)
    job = jobs.wait(submitted["job_id"], timeout=5)
    assert job.status == "failed"
    assert job.error == "quebrou"
    with pytest.raises(ToolError) as exc:
        jobs.result(job.id)
    assert exc.value.code == "ffmpeg_failed"


def test_unknown_job(jobs: JobManager) -> None:
    with pytest.raises(ToolError) as exc:
        jobs.get("nope")
    assert exc.value.code == "job_not_found"
