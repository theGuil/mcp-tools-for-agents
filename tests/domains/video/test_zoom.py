import pytest

from domains import Runtime
from domains.video.probe import probe_video
from domains.video.zoom import zoom_expression, zoom_video
from tests.helpers import unwrap

pytestmark = pytest.mark.ffmpeg


def test_zoom_expression() -> None:
    assert zoom_expression("punch", 1.5, 1.0, 2.0) == (
        "if(between(t\\,1.000\\,2.000)\\,1.5000\\,1)"
    )
    assert "(1+(1.5000-1)*" in zoom_expression("in", 1.5, 0.0, 1.0)
    assert "(1.5000-(1.5000-1)*" in zoom_expression("out", 1.5, 0.0, 1.0)


@pytest.mark.parametrize("mode", ["punch", "in", "out"])
def test_zoom_video(runtime: Runtime, sample_video: str, mode: str) -> None:
    result = unwrap(zoom_video(runtime, sample_video, 0.5, 2.0, mode=mode, focus_y=0.3))  # type: ignore[arg-type]
    assert result["output"] == f"sample_zoom_{mode}.mp4"
    probed = unwrap(probe_video(runtime, result["output"]))
    assert probed["info"]["width"] == 320
    assert probed["info"]["height"] == 240
    assert abs(probed["info"]["duration"] - 3.0) < 0.3


def test_zoom_video_invalid(runtime: Runtime, sample_video: str) -> None:
    assert zoom_video(runtime, sample_video, 2.0, 1.0).get("code") == "invalid_argument"
    assert zoom_video(runtime, sample_video, 0.0, 1.0, zoom=10).get("code") == "invalid_argument"
    assert zoom_video(runtime, sample_video, 0.0, 1.0, focus_x=2).get("code") == (
        "invalid_argument"
    )
    assert zoom_video(runtime, sample_video, 50.0, 51.0).get("code") == "invalid_argument"


def test_zoom_video_missing(runtime: Runtime) -> None:
    assert zoom_video(runtime, "ghost.mp4", 0.0, 1.0).get("code") == "not_found"
