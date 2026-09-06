import pytest

from domains import Runtime
from domains.video.probe import probe_video
from domains.video.text_overlay import add_text_overlay
from tests.helpers import unwrap, unwrap_job

pytestmark = pytest.mark.ffmpeg


def test_overlay_creates_file(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(add_text_overlay(runtime, sample_video, "Olá: 'mundo'\\n100%", end=2.0))
    assert result["output"] == "sample_text.mp4"
    probed = unwrap(probe_video(runtime, result["output"]))
    assert abs(probed["info"]["duration"] - 3.0) < 0.2
    assert probed["info"]["has_audio"]


def test_overlay_empty_text(runtime: Runtime, sample_video: str) -> None:
    assert add_text_overlay(runtime, sample_video, "   ").get("code") == "invalid_argument"


def test_overlay_bad_interval(runtime: Runtime, sample_video: str) -> None:
    assert add_text_overlay(runtime, sample_video, "x", start=2, end=1).get("code") == (
        "invalid_argument"
    )
    assert add_text_overlay(runtime, sample_video, "x", start=99).get("code") == (
        "invalid_argument"
    )


def test_overlay_missing_file(runtime: Runtime) -> None:
    assert add_text_overlay(runtime, "ghost.mp4", "x").get("code") == "not_found"


def test_overlay_background(runtime: Runtime, sample_video: str) -> None:
    submitted = unwrap_job(add_text_overlay(runtime, sample_video, "bg", background=True))
    job = runtime.jobs.wait(submitted["job_id"], timeout=60)
    assert job.status == "done"
