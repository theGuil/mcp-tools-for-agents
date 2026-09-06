import pytest

from domains import Runtime
from domains.video.burn_subtitles import burn_subtitles
from domains.video.probe import probe_video
from domains.video.subtitles import SubtitleSegment, create_subtitles, format_timestamp
from tests.helpers import unwrap

SEGMENTS = [
    SubtitleSegment(start=0.0, end=1.2, text="Primeira frase"),
    SubtitleSegment(start=1.2, end=2.8, text="Segunda frase"),
]


def test_format_timestamp() -> None:
    assert format_timestamp(0) == "00:00:00,000"
    assert format_timestamp(3661.5) == "01:01:01,500"


def test_create_subtitles(runtime: Runtime) -> None:
    result = unwrap(create_subtitles(runtime, SEGMENTS, "legendas/teste.srt"))
    assert result["output"] == "legendas/teste.srt"
    assert result["segments"] == 2
    content = runtime.workspace.existing("legendas/teste.srt").read_text(encoding="utf-8")
    assert "00:00:01,200 --> 00:00:02,800\nSegunda frase" in content


def test_create_subtitles_invalid(runtime: Runtime) -> None:
    assert create_subtitles(runtime, [], "a.srt").get("code") == "invalid_argument"
    assert create_subtitles(runtime, SEGMENTS, "a.txt").get("code") == "invalid_argument"
    bad = [SubtitleSegment(start=2.0, end=1.0, text="x")]
    assert create_subtitles(runtime, bad, "a.srt").get("code") == "invalid_argument"
    empty = [SubtitleSegment(start=0.0, end=1.0, text="  ")]
    assert create_subtitles(runtime, empty, "a.srt").get("code") == "invalid_argument"


def test_create_subtitles_outside(runtime: Runtime) -> None:
    assert create_subtitles(runtime, SEGMENTS, "../a.srt").get("code") == "outside_workspace"


@pytest.mark.ffmpeg
def test_burn_subtitles(runtime: Runtime, sample_video: str) -> None:
    unwrap(create_subtitles(runtime, SEGMENTS, "sample.srt"))
    result = unwrap(burn_subtitles(runtime, sample_video, "sample.srt"))
    assert result["output"] == "sample_subtitled.mp4"
    probed = unwrap(probe_video(runtime, result["output"]))
    assert abs(probed["info"]["duration"] - 3.0) < 0.2


@pytest.mark.ffmpeg
def test_burn_subtitles_wrong_extension(runtime: Runtime, sample_video: str) -> None:
    assert burn_subtitles(runtime, sample_video, sample_video).get("code") == "invalid_argument"


def test_burn_subtitles_missing(runtime: Runtime) -> None:
    assert burn_subtitles(runtime, "ghost.mp4", "x.srt").get("code") == "not_found"
