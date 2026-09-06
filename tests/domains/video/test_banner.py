import pytest

from domains import Runtime
from domains.video.banner import add_banner
from tests.helpers import unwrap, unwrap_job

pytestmark = pytest.mark.ffmpeg

TEXT = "Nesta página mostramos apenas artistas sem auto-tune"


def test_banner_creates_file(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(add_banner(runtime, sample_video, TEXT, color="#1E40AF", font_size=14))
    assert result["output"] == "sample_banner.mp4"
    assert result["band_height_px"] == 24
    assert result["offset_px"] == 0
    assert result["font_size"] == 14
    info = runtime.ffmpeg.probe(runtime.workspace.existing(result["output"]))
    assert (info["width"], info["height"]) == (320, 240)
    assert info["has_audio"]


def test_banner_offset_two_lines(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(
        add_banner(
            runtime,
            sample_video,
            "Nesta página mostramos apenas\nartistas sem auto-tune",
            height_ratio=0.2,
            offset_ratio=0.15,
        )
    )
    assert result["band_height_px"] == 48
    assert result["offset_px"] == 36
    assert runtime.workspace.existing(result["output"]).stat().st_size > 0


def test_banner_bottom_interval_background(runtime: Runtime, sample_video: str) -> None:
    job = unwrap_job(
        add_banner(
            runtime,
            sample_video,
            "Rodapé: 100% ao vivo",
            position="bottom",
            height_ratio=0.2,
            offset_ratio=0.25,
            start=0.5,
            end=2.0,
            background=True,
        )
    )
    finished = runtime.jobs.wait(job["job_id"], timeout=60)
    assert finished.status == "done"
    assert finished.result is not None
    assert finished.result["band_height_px"] == 48
    assert finished.result["offset_px"] == 60


def test_banner_empty_text(runtime: Runtime, sample_video: str) -> None:
    assert add_banner(runtime, sample_video, "   ").get("code") == "invalid_argument"


def test_banner_bad_ratio(runtime: Runtime, sample_video: str) -> None:
    result = add_banner(runtime, sample_video, TEXT, height_ratio=0.9)
    assert result.get("code") == "invalid_argument"


def test_banner_bad_offset(runtime: Runtime, sample_video: str) -> None:
    result = add_banner(runtime, sample_video, TEXT, offset_ratio=0.9)
    assert result.get("code") == "invalid_argument"


def test_banner_bad_color(runtime: Runtime, sample_video: str) -> None:
    result = add_banner(runtime, sample_video, TEXT, color="blue;rm")
    assert result.get("code") == "invalid_argument"


def test_banner_bad_interval(runtime: Runtime, sample_video: str) -> None:
    result = add_banner(runtime, sample_video, TEXT, start=2.0, end=1.0)
    assert result.get("code") == "invalid_argument"


def test_banner_start_after_end_of_video(runtime: Runtime, sample_video: str) -> None:
    result = add_banner(runtime, sample_video, TEXT, start=50.0)
    assert result.get("code") == "invalid_argument"


def test_banner_missing_file(runtime: Runtime) -> None:
    assert add_banner(runtime, "ghost.mp4", TEXT).get("code") == "not_found"
