import subprocess

import pytest

from core.paths import Workspace
from domains import Runtime
from domains.video.background_music import add_background_music
from domains.video.probe import probe_video
from tests.helpers import unwrap, unwrap_job

pytestmark = pytest.mark.ffmpeg


@pytest.fixture
def music(workspace: Workspace) -> str:
    target = workspace.root / "trilha.mp3"
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
            "sine=frequency=220:duration=1",
            "-c:a",
            "libmp3lame",
            str(target),
        ],
        check=True,
    )
    return "trilha.mp3"


def test_add_background_music_with_ducking(runtime: Runtime, sample_video: str, music: str) -> None:
    result = unwrap(add_background_music(runtime, sample_video, music))
    assert result["output"] == "sample_music.mp4"
    assert result["ducking"] is True
    assert result["looped"] is True
    probed = unwrap(probe_video(runtime, result["output"]))
    assert probed["info"]["has_audio"]
    assert abs(probed["info"]["duration"] - 3.0) < 0.3


def test_add_background_music_no_ducking_no_loop(
    runtime: Runtime, sample_video: str, music: str
) -> None:
    result = unwrap(
        add_background_music(
            runtime, sample_video, music, ducking=False, loop=False, start=1.0, fade_out=0.0
        )
    )
    assert result["ducking"] is False
    assert result["looped"] is False


def test_add_background_music_invalid(runtime: Runtime, sample_video: str, music: str) -> None:
    assert add_background_music(runtime, sample_video, music, music_volume=0).get("code") == (
        "invalid_argument"
    )
    assert add_background_music(runtime, sample_video, music, fade_in=-1).get("code") == (
        "invalid_argument"
    )
    assert add_background_music(runtime, sample_video, music, start=10).get("code") == (
        "invalid_argument"
    )


def test_add_background_music_missing(runtime: Runtime, sample_video: str) -> None:
    assert add_background_music(runtime, sample_video, "ghost.mp3").get("code") == "not_found"
    assert add_background_music(runtime, "ghost.mp4", sample_video).get("code") == "not_found"


def test_add_background_music_background(runtime: Runtime, sample_video: str, music: str) -> None:
    submitted = unwrap_job(add_background_music(runtime, sample_video, music, background=True))
    job = runtime.jobs.wait(submitted["job_id"], timeout=60)
    assert job.status == "done"
