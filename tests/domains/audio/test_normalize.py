import pytest

from domains import Runtime
from domains.audio.extract import extract_audio
from domains.audio.normalize import normalize_audio
from domains.video.probe import probe_video
from tests.helpers import unwrap

pytestmark = pytest.mark.ffmpeg


def test_normalize_video(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(normalize_audio(runtime, sample_video, preset="tiktok"))
    assert result["output"] == "sample_normalized.mp4"
    assert result["target_lufs"] == -14.0
    assert result["is_video"] is True
    assert result["measured_lufs"] is not None
    assert result["gain_db"] is not None
    probed = unwrap(probe_video(runtime, result["output"]))
    assert probed["info"]["has_video"]
    assert probed["info"]["has_audio"]


def test_normalize_audio_file_custom_target(runtime: Runtime, sample_video: str) -> None:
    audio = unwrap(extract_audio(runtime, sample_video))
    result = unwrap(normalize_audio(runtime, audio["output"], target_lufs=-16))
    assert result["is_video"] is False
    assert result["target_lufs"] == -16


def test_normalize_invalid(runtime: Runtime, sample_video: str) -> None:
    assert normalize_audio(runtime, sample_video, target_lufs=5).get("code") == ("invalid_argument")


def test_normalize_missing(runtime: Runtime) -> None:
    assert normalize_audio(runtime, "ghost.mp4").get("code") == "not_found"
