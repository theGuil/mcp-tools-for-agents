import pytest

from domains import Runtime
from domains.video.cut import cut_video
from domains.video.probe import probe_video
from tests.helpers import unwrap, unwrap_job

pytestmark = pytest.mark.ffmpeg


def test_cut_creates_file(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(cut_video(runtime, sample_video, 0.5, 2.0, reencode=True))
    assert result["output"] == "sample_cut_0.5-2.mp4"
    probed = unwrap(probe_video(runtime, result["output"]))
    assert abs(probed["info"]["duration"] - 1.5) < 0.2


def test_cut_invalid_range(runtime: Runtime, sample_video: str) -> None:
    result = cut_video(runtime, sample_video, 2.0, 1.0)
    assert result.get("code") == "invalid_argument"


def test_cut_beyond_duration(runtime: Runtime, sample_video: str) -> None:
    result = cut_video(runtime, sample_video, 0.0, 99.0)
    assert result.get("code") == "invalid_argument"


def test_cut_missing_file(runtime: Runtime) -> None:
    result = cut_video(runtime, "ghost.mp4", 0.0, 1.0)
    assert result.get("code") == "not_found"


def test_cut_background(runtime: Runtime, sample_video: str) -> None:
    submitted = unwrap_job(cut_video(runtime, sample_video, 0.0, 1.0, background=True))
    job = runtime.jobs.wait(submitted["job_id"], timeout=30)
    assert job.status == "done"
    assert job.result is not None
    assert str(job.result["output"]).startswith("sample_cut_")
