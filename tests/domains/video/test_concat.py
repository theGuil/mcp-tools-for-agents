import pytest

from domains import Runtime
from domains.video.concat import concat_videos, transition_offsets
from domains.video.cut import cut_video
from domains.video.probe import probe_video
from tests.helpers import unwrap

pytestmark = pytest.mark.ffmpeg


def test_transition_offsets() -> None:
    assert transition_offsets([3.0, 3.0, 3.0], 0.5) == [2.5, 5.0]


@pytest.fixture
def clips(runtime: Runtime, sample_video: str) -> list[str]:
    first = unwrap(cut_video(runtime, sample_video, 0.0, 1.5, reencode=True))
    second = unwrap(cut_video(runtime, sample_video, 1.5, 3.0, reencode=True))
    return [first["output"], second["output"]]


def test_concat_copy(runtime: Runtime, clips: list[str]) -> None:
    result = unwrap(concat_videos(runtime, clips))
    assert result["count"] == 2
    assert result["transition"] is None
    assert result["has_audio"] is True
    probed = unwrap(probe_video(runtime, result["output"]))
    assert abs(probed["info"]["duration"] - 3.0) < 0.3


def test_concat_with_transition(runtime: Runtime, clips: list[str]) -> None:
    result = unwrap(
        concat_videos(
            runtime, clips, transition="fade", transition_duration=0.5, output_name="f.mp4"
        )
    )
    assert result["output"] == "f.mp4"
    assert result["transition"] == "fade"
    assert result["has_audio"] is True
    probed = unwrap(probe_video(runtime, result["output"]))
    assert abs(probed["info"]["duration"] - 2.5) < 0.3


def test_concat_transition_invalid(runtime: Runtime, clips: list[str]) -> None:
    assert concat_videos(runtime, clips, transition="explode").get("code") == (  # type: ignore[arg-type]
        "invalid_argument"
    )
    assert concat_videos(runtime, clips, transition="fade", transition_duration=2.0).get(
        "code"
    ) == ("invalid_argument")
    assert concat_videos(runtime, clips[:1]).get("code") == "invalid_argument"


def test_concat_missing(runtime: Runtime, sample_video: str) -> None:
    assert concat_videos(runtime, [sample_video, "ghost.mp4"]).get("code") == "not_found"
