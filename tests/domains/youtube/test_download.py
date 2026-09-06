import shutil
from pathlib import Path

import pytest

from core.youtube import VideoQuality, YouTube
from domains import Runtime
from domains.youtube.download import download_youtube_video
from tests.helpers import unwrap, unwrap_job

pytestmark = pytest.mark.ffmpeg


def _fake_download(runtime: Runtime, sample_video: str, monkeypatch: pytest.MonkeyPatch) -> None:
    sample = runtime.workspace.existing(sample_video)

    def fake(_self: YouTube, _url: str, target_dir: Path, _quality: VideoQuality) -> Path:
        target_dir.mkdir(parents=True, exist_ok=True)
        target = target_dir / "Video_teste_abc.mp4"
        shutil.copy(sample, target)
        return target

    monkeypatch.setattr(YouTube, "download", fake)


def test_download_success(
    runtime: Runtime, sample_video: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    _fake_download(runtime, sample_video, monkeypatch)
    result = unwrap(download_youtube_video(runtime, "https://youtu.be/abc"))
    assert result["output"] == "downloads/Video_teste_abc.mp4"
    assert result["duration"] > 2.5
    assert result["quality"] == "720p"


def test_download_background(
    runtime: Runtime, sample_video: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    _fake_download(runtime, sample_video, monkeypatch)
    submitted = unwrap_job(download_youtube_video(runtime, "https://youtu.be/abc", background=True))
    job = runtime.jobs.wait(submitted["job_id"], timeout=30)
    assert job.status == "done"
    assert job.result is not None
    assert str(job.result["output"]).endswith(".mp4")


def test_download_invalid_url(runtime: Runtime) -> None:
    result = download_youtube_video(runtime, "https://example.com/v")
    assert result.get("code") == "invalid_argument"


def test_download_folder_outside_workspace(runtime: Runtime) -> None:
    result = download_youtube_video(runtime, "https://youtu.be/abc", folder="../fora")
    assert result.get("code") == "outside_workspace"


def test_download_folder_is_file(runtime: Runtime, sample_video: str) -> None:
    result = download_youtube_video(runtime, "https://youtu.be/abc", folder=sample_video)
    assert result.get("code") == "invalid_argument"
