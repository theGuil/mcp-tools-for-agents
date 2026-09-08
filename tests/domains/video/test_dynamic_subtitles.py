import pytest

from domains import Runtime
from domains.video.burn_subtitles import burn_subtitles
from domains.video.dynamic_subtitles import (
    WordTiming,
    ass_color,
    ass_time,
    create_dynamic_subtitles,
)
from tests.helpers import unwrap

WORDS = [
    WordTiming(start=0.0, end=0.4, word="olá"),
    WordTiming(start=0.4, end=0.8, word="mundo"),
    WordTiming(start=0.9, end=1.3, word="isso"),
    WordTiming(start=1.3, end=1.6, word="é"),
    WordTiming(start=1.6, end=2.0, word="teste"),
    WordTiming(start=2.8, end=2.95, word="fim"),
]


def test_ass_helpers() -> None:
    assert ass_time(0) == "0:00:00.00"
    assert ass_time(3661.5) == "1:01:01.50"
    assert ass_color("#FFD700", "x") == "&H0000D7FF"


def test_create_dynamic_subtitles_highlight(runtime: Runtime) -> None:
    result = unwrap(
        create_dynamic_subtitles(runtime, WORDS, "legendas/din.ass", max_words=4, max_gap=0.5)
    )
    assert result["output"] == "legendas/din.ass"
    assert result["words"] == 6
    assert result["groups"] == 3
    assert result["duration"] == 2.95
    assert result["play_res"] == "1080x1920"
    content = runtime.workspace.existing("legendas/din.ass").read_text(encoding="utf-8")
    assert "PlayResX: 1080" in content
    assert "{\\c&H0000D7FF&\\fscx108\\fscy108}OLÁ" in content
    assert content.count("Dialogue:") == 6


def test_create_dynamic_subtitles_word_and_block(runtime: Runtime) -> None:
    word = unwrap(create_dynamic_subtitles(runtime, WORDS, "w.ass", style="word", uppercase=False))
    content = runtime.workspace.existing("w.ass").read_text(encoding="utf-8")
    assert word["words"] == 6
    assert "olá" in content
    assert "\\t(0,70," in content
    block = unwrap(create_dynamic_subtitles(runtime, WORDS, "b.ass", style="block"))
    content = runtime.workspace.existing("b.ass").read_text(encoding="utf-8")
    assert block["groups"] == content.count("Dialogue:")
    assert "\\fad(80,80)" in content


def test_create_dynamic_subtitles_invalid(runtime: Runtime) -> None:
    assert create_dynamic_subtitles(runtime, [], "a.ass").get("code") == "invalid_argument"
    assert create_dynamic_subtitles(runtime, WORDS, "a.srt").get("code") == "invalid_argument"
    assert create_dynamic_subtitles(runtime, WORDS, "a.ass", max_words=0).get("code") == (
        "invalid_argument"
    )
    assert create_dynamic_subtitles(runtime, WORDS, "a.ass", highlight_color="ouro").get(
        "code"
    ) == ("invalid_argument")
    unordered = [WORDS[1], WORDS[0]]
    assert create_dynamic_subtitles(runtime, unordered, "a.ass").get("code") == ("invalid_argument")


def test_create_dynamic_subtitles_missing_video(runtime: Runtime) -> None:
    assert create_dynamic_subtitles(runtime, WORDS, "a.ass", video_path="ghost.mp4").get(
        "code"
    ) == ("not_found")


@pytest.mark.ffmpeg
def test_dynamic_subtitles_burn(runtime: Runtime, sample_video: str) -> None:
    subs = unwrap(create_dynamic_subtitles(runtime, WORDS, "din.ass", video_path=sample_video))
    assert subs["play_res"] == "320x240"
    result = unwrap(burn_subtitles(runtime, sample_video, subs["output"]))
    assert result["styled_by_file"] is True
    assert result["output"] == "sample_subtitled.mp4"
