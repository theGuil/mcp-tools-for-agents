import pytest

from domains import Runtime
from domains.video.fade import add_fade
from domains.video.probe import probe_video
from tests.helpers import unwrap

pytestmark = pytest.mark.ffmpeg


def test_add_fade(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(add_fade(runtime, sample_video, fade_in=0.5, fade_out=0.5))
    assert result["output"] == "sample_fade.mp4"
    assert result["audio_faded"] is True
    probed = unwrap(probe_video(runtime, result["output"]))
    assert abs(probed["info"]["duration"] - 3.0) < 0.3


def test_add_fade_only_out_white(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(add_fade(runtime, sample_video, fade_in=0, fade_out=1, color="white"))
    assert result["color"] == "white"
    assert result["fade_in"] == 0


def test_add_fade_invalid(runtime: Runtime, sample_video: str) -> None:
    assert add_fade(runtime, sample_video, fade_in=0, fade_out=0).get("code") == (
        "invalid_argument"
    )
    assert add_fade(runtime, sample_video, fade_in=-1).get("code") == "invalid_argument"
    assert add_fade(runtime, sample_video, fade_in=2, fade_out=2).get("code") == (
        "invalid_argument"
    )


def test_add_fade_missing(runtime: Runtime) -> None:
    assert add_fade(runtime, "ghost.mp4").get("code") == "not_found"
