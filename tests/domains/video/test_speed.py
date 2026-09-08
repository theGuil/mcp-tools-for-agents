import pytest

from domains import Runtime
from domains.video.probe import probe_video
from domains.video.speed import atempo_chain, change_speed
from tests.helpers import unwrap

pytestmark = pytest.mark.ffmpeg


def test_atempo_chain() -> None:
    assert atempo_chain(2.0) == "atempo=2"
    assert atempo_chain(0.25) == "atempo=0.5,atempo=0.5"
    assert atempo_chain(0.4) == "atempo=0.5,atempo=0.8"


def test_change_speed_whole(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(change_speed(runtime, sample_video, 2.0))
    assert result["output"] == "sample_speed_2x.mp4"
    assert result["new_duration"] == 1.5
    probed = unwrap(probe_video(runtime, result["output"]))
    assert abs(probed["info"]["duration"] - 1.5) < 0.3


def test_change_speed_segment(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(change_speed(runtime, sample_video, 0.5, start=1.0, end=2.0))
    assert result["new_duration"] == 4.0
    probed = unwrap(probe_video(runtime, result["output"]))
    assert abs(probed["info"]["duration"] - 4.0) < 0.4


def test_change_speed_invalid(runtime: Runtime, sample_video: str) -> None:
    assert change_speed(runtime, sample_video, 1.0).get("code") == "invalid_argument"
    assert change_speed(runtime, sample_video, 20.0).get("code") == "invalid_argument"
    assert change_speed(runtime, sample_video, 2.0, start=1.0).get("code") == "invalid_argument"
    assert change_speed(runtime, sample_video, 2.0, start=2.0, end=1.0).get("code") == (
        "invalid_argument"
    )
    assert change_speed(runtime, sample_video, 2.0, start=0.0, end=99.0).get("code") == (
        "invalid_argument"
    )


def test_change_speed_missing(runtime: Runtime) -> None:
    assert change_speed(runtime, "ghost.mp4", 2.0).get("code") == "not_found"
