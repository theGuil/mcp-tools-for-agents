import subprocess

import pytest

from core.paths import Workspace
from domains import Runtime
from domains.video.narration import add_narration
from domains.video.probe import probe_video
from tests.helpers import unwrap

pytestmark = pytest.mark.ffmpeg


@pytest.fixture
def narration(workspace: Workspace) -> str:
    target = workspace.root / "voz.m4a"
    subprocess.run(
        [
            "ffmpeg",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:duration=1.5",
            "-c:a",
            "aac",
            str(target),
        ],
        check=True,
    )
    return "voz.m4a"


def test_add_narration_mix(runtime: Runtime, sample_video: str, narration: str) -> None:
    result = unwrap(add_narration(runtime, sample_video, narration, start=0.5))
    assert result["output"] == "sample_narrated.mp4"
    assert result["replaced_audio"] is False
    probed = unwrap(probe_video(runtime, result["output"]))
    assert probed["info"]["has_audio"]
    assert abs(probed["info"]["duration"] - 3.0) < 0.3


def test_add_narration_replace(runtime: Runtime, sample_video: str, narration: str) -> None:
    result = unwrap(add_narration(runtime, sample_video, narration, original_volume=0.0))
    assert result["replaced_audio"] is True


def test_add_narration_invalid(runtime: Runtime, sample_video: str, narration: str) -> None:
    assert add_narration(runtime, sample_video, narration, start=-1).get("code") == (
        "invalid_argument"
    )
    assert add_narration(runtime, sample_video, narration, original_volume=2).get("code") == (
        "invalid_argument"
    )
    assert add_narration(runtime, sample_video, narration, start=50).get("code") == (
        "invalid_argument"
    )


def test_add_narration_missing(runtime: Runtime, sample_video: str) -> None:
    assert add_narration(runtime, sample_video, "ghost.mp3").get("code") == "not_found"
