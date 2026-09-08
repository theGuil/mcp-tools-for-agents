from pathlib import Path

import pytest

from core.vision import load_face_detector
from domains import Runtime
from domains.video import smart_crop as module
from domains.video.probe import probe_video
from domains.video.smart_crop import crop_size, smart_crop, smooth_positions
from tests.helpers import unwrap

pytestmark = pytest.mark.ffmpeg


def test_crop_size() -> None:
    assert crop_size(1920, 1080, "9:16") == (608, 1080)
    assert crop_size(1920, 1080, "1:1") == (1080, 1080)
    assert crop_size(1080, 1920, "16:9") == (1080, 608)
    assert crop_size(320, 240, "9:16") == (134, 240)


def test_smooth_positions() -> None:
    positions = smooth_positions([100.0, None, 100.0, 101.0, 500.0], default=50.0, deadzone=10.0)
    assert positions[0] == 100.0
    assert positions[1] == 100.0
    assert positions[3] == 100.0  # dentro da zona morta
    assert 100.0 < positions[4] < 500.0


def test_smart_crop_center(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(smart_crop(runtime, sample_video, mode="center"))
    assert result["output"] == "sample_crop_9x16.mp4"
    assert result["mode_used"] == "center"
    assert (result["width"], result["height"]) == (134, 240)
    probed = unwrap(probe_video(runtime, result["output"]))
    assert probed["info"]["width"] == 134
    assert probed["info"]["has_audio"]


def test_smart_crop_face_fallback(runtime: Runtime, sample_video: str) -> None:
    pytest.importorskip("cv2")
    result = unwrap(smart_crop(runtime, sample_video, aspect="1:1"))
    assert result["mode_used"] == "center"
    assert result["frames_analyzed"] > 0
    assert result["frames_with_face"] == 0


def test_smart_crop_face_tracking(
    runtime: Runtime, sample_video: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    def fake_track(*_args: object, **_kwargs: object) -> tuple[list[float | None], int]:
        return [60.0, 80.0, None, 200.0, 260.0, 300.0], 5

    monkeypatch.setattr(module, "_face_track", fake_track)
    result = unwrap(smart_crop(runtime, sample_video, sample_interval=0.5))
    assert result["mode_used"] == "face"
    assert result["frames_with_face"] == 5
    probed = unwrap(probe_video(runtime, result["output"]))
    assert probed["info"]["width"] == 134
    assert abs(probed["info"]["duration"] - 3.0) < 0.3


def test_smart_crop_invalid(runtime: Runtime, sample_video: str) -> None:
    assert smart_crop(runtime, sample_video, aspect="2:3").get("code") == "invalid_argument"  # type: ignore[arg-type]
    assert smart_crop(runtime, sample_video, sample_interval=0).get("code") == "invalid_argument"
    assert smart_crop(runtime, sample_video, mode="tracking").get("code") == (  # type: ignore[arg-type]
        "invalid_argument"
    )


def test_smart_crop_missing(runtime: Runtime) -> None:
    assert smart_crop(runtime, "ghost.mp4").get("code") == "not_found"


def test_face_detector_loads_and_ignores_frames_without_face(tmp_path: Path) -> None:
    pytest.importorskip("cv2")
    detector = load_face_detector()
    assert detector.largest_face(tmp_path / "nada.jpg") is None
