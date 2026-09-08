import pytest

from domains import Runtime
from domains.video.export import export_for_platform, fit_size
from domains.video.probe import probe_video
from tests.helpers import unwrap, unwrap_job

pytestmark = pytest.mark.ffmpeg


def test_fit_size() -> None:
    assert fit_size(3840, 2160, 1920, 1080) == (1920, 1080)
    assert fit_size(1080, 1920, 1080, 1920) == (1080, 1920)
    assert fit_size(640, 360, 1920, 1080) == (640, 360)
    assert fit_size(1500, 1000, 1080, 1920) == (1080, 720)


def test_export_youtube(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(export_for_platform(runtime, sample_video, "youtube"))
    assert result["output"] == "sample_youtube.mp4"
    assert (result["width"], result["height"]) == (320, 240)
    assert result["fps"] == 25.0
    assert any("Resolução baixa" in w for w in result["warnings"])
    probed = unwrap(probe_video(runtime, result["output"]))
    assert probed["info"]["video_codec"] == "h264"
    assert probed["info"]["audio_codec"] == "aac"


def test_export_tiktok_warns_orientation(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(
        export_for_platform(runtime, sample_video, "tiktok", quality="high", output="final/t.mp4")
    )
    assert result["output"] == "final/t.mp4"
    assert any("smart_crop" in w for w in result["warnings"])


def test_export_invalid(runtime: Runtime, sample_video: str) -> None:
    assert export_for_platform(runtime, sample_video, "vimeo").get("code") == (  # type: ignore[arg-type]
        "invalid_argument"
    )
    assert export_for_platform(runtime, sample_video, "youtube", output="a.mkv").get("code") == (
        "invalid_argument"
    )


def test_export_missing(runtime: Runtime) -> None:
    assert export_for_platform(runtime, "ghost.mp4", "youtube").get("code") == "not_found"


def test_export_background(runtime: Runtime, sample_video: str) -> None:
    submitted = unwrap_job(export_for_platform(runtime, sample_video, "youtube", background=True))
    job = runtime.jobs.wait(submitted["job_id"], timeout=60)
    assert job.status == "done"
