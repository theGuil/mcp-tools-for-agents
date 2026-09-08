from domains.video.remove_silence import _parse_silences, _speech_segments

STDERR = """
[silencedetect @ 0x1] silence_start: 1.02
[silencedetect @ 0x1] silence_end: 1.98 | silence_duration: 0.96
[silencedetect @ 0x1] silence_start: 2.9
"""


def test_parse_silences_closes_open_interval() -> None:
    assert _parse_silences(STDERR, 3.0) == [(1.02, 1.98), (2.9, 3.0)]


def test_speech_segments_inverts_and_pads() -> None:
    segments = _speech_segments([(1.0, 2.0)], 3.0, 0.2)
    assert [(s["start"], s["end"]) for s in segments] == [(0.0, 1.2), (1.8, 3.0)]


def test_speech_segments_merges_overlaps() -> None:
    segments = _speech_segments([(1.0, 1.3)], 3.0, 0.2)
    assert [(s["start"], s["end"]) for s in segments] == [(0.0, 3.0)]


def test_speech_segments_all_silent() -> None:
    assert _speech_segments([(0.0, 3.0)], 3.0, 0.0) == []
