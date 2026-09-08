import subprocess

import pytest

from core.paths import Workspace
from domains import Runtime
from domains.video.probe import probe_video
from domains.video.remove_silence import remove_silence
from tests.helpers import unwrap, unwrap_job

pytestmark = pytest.mark.ffmpeg

SPEECH_SECONDS = 2.0
SILENCE_SECONDS = 1.0
TOTAL = SPEECH_SECONDS + SILENCE_SECONDS


@pytest.fixture
def gapped_video(workspace: Workspace) -> str:
    """Vídeo de 3s: tom em 0-1s, silêncio em 1-2s, tom em 2-3s."""
    target = workspace.root / "aula.mp4"
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
            f"testsrc=size=320x240:rate=25:duration={TOTAL}",
            "-f",
            "lavfi",
            "-i",
            f"sine=frequency=440:duration={TOTAL}",
            "-af",
            "volume='if(between(t,1,2),0,1)':eval=frame",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            str(target),
        ],
        check=True,
    )
    return "aula.mp4"


def test_remove_silence_cuts_gap(runtime: Runtime, gapped_video: str) -> None:
    result = unwrap(remove_silence(runtime, gapped_video, margin=0.0))
    assert result["output"] == "aula_nosilence.mp4"
    assert result["silences_removed"] == 1
    assert len(result["segments"]) == 2
    assert abs(result["removed_duration"] - SILENCE_SECONDS) < 0.2
    probed = unwrap(probe_video(runtime, result["output"]))
    assert abs(probed["info"]["duration"] - SPEECH_SECONDS) < 0.3


def test_remove_silence_margin_keeps_more(runtime: Runtime, gapped_video: str) -> None:
    result = unwrap(remove_silence(runtime, gapped_video, margin=0.2))
    assert abs(result["duration"] - (SPEECH_SECONDS + 0.4)) < 0.2


def test_remove_silence_without_gaps(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(remove_silence(runtime, sample_video))
    assert result["silences_removed"] == 0
    assert len(result["segments"]) == 1


def test_remove_silence_invalid_threshold(runtime: Runtime, sample_video: str) -> None:
    result = remove_silence(runtime, sample_video, threshold_db=5.0)
    assert result.get("code") == "invalid_argument"


def test_remove_silence_invalid_min_silence(runtime: Runtime, sample_video: str) -> None:
    result = remove_silence(runtime, sample_video, min_silence=0.0)
    assert result.get("code") == "invalid_argument"


def test_remove_silence_missing_file(runtime: Runtime) -> None:
    result = remove_silence(runtime, "ghost.mp4")
    assert result.get("code") == "not_found"


def test_remove_silence_background(runtime: Runtime, gapped_video: str) -> None:
    submitted = unwrap_job(remove_silence(runtime, gapped_video, background=True))
    job = runtime.jobs.wait(submitted["job_id"], timeout=60)
    assert job.status == "done"
    assert job.result is not None
    assert job.result["output"] == "aula_nosilence.mp4"
