import pytest

from core.ffmpeg import FFmpeg
from domains import Runtime
from domains.audio.extract import extract_audio
from tests.helpers import unwrap

pytestmark = pytest.mark.ffmpeg


def test_extract_wav(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(extract_audio(runtime, sample_video, audio_format="wav"))
    assert result["output"] == "sample_audio.wav"
    info = FFmpeg().probe(runtime.workspace.root / result["output"])
    assert info["has_audio"]
    assert not info["has_video"]


def test_extract_missing(runtime: Runtime) -> None:
    result = extract_audio(runtime, "nada.mp4")
    assert result.get("code") == "not_found"
