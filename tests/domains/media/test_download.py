import shutil
from pathlib import Path

import pytest

from core.downloader import Downloader, VideoQuality
from domains import Runtime
from domains.media.download import download_video
from tests.helpers import unwrap, unwrap_job


def _fake_download(runtime: Runtime, sample_video: str, monkeypatch: pytest.MonkeyPatch) -> None:
    sample = runtime.workspace.existing(sample_video)

    def fake(_self: Downloader, _url: str, target_dir: Path, _quality: VideoQuality) -> Path:
        target_dir.mkdir(parents=True, exist_ok=True)
        target = target_dir / "Aula_de_matematica_2345.mp4"
        shutil.copy(sample, target)
        return target

    monkeypatch.setattr(Downloader, "download", fake)


@pytest.mark.ffmpeg
def test_download_success(
    runtime: Runtime, sample_video: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    _fake_download(runtime, sample_video, monkeypatch)
    result = unwrap(download_video(runtime, "https://eaulas.usp.br/portal/video?idItem=2345"))
    assert result["output"] == "downloads/Aula_de_matematica_2345.mp4"
    assert result["duration"] > 2.5
    assert result["quality"] == "720p"


@pytest.mark.ffmpeg
def test_download_background(
    runtime: Runtime, sample_video: str, monkeypatch: pytest.MonkeyPatch
) -> None:
    _fake_download(runtime, sample_video, monkeypatch)
    submitted = unwrap_job(download_video(runtime, "https://youtu.be/abc", background=True))
    job = runtime.jobs.wait(submitted["job_id"], timeout=30)
    assert job.status == "done"
    assert job.result is not None
    assert str(job.result["output"]).endswith(".mp4")


def test_download_aceita_qualquer_site(runtime: Runtime, monkeypatch: pytest.MonkeyPatch) -> None:
    vistas: list[str] = []

    def espia(_self: Downloader, url: str, target_dir: Path, _quality: VideoQuality) -> Path:
        vistas.append(url)
        target_dir.mkdir(parents=True, exist_ok=True)
        target = target_dir / "arquivo.mp4"
        target.touch()
        return target

    monkeypatch.setattr(Downloader, "download", espia)
    monkeypatch.setattr(
        "core.ffmpeg.FFmpeg.probe", lambda _self, _path: {"duration": 1.0, "size_bytes": 10}
    )
    for url in [
        "https://eaulas.usp.br/portal/video?idItem=2345",
        "https://vimeo.com/123",
        "https://cdn.exemplo.com/aula.mp4",
    ]:
        assert unwrap(download_video(runtime, url))["output"] == "downloads/arquivo.mp4"
    assert len(vistas) == 3


def test_download_invalid_url(runtime: Runtime) -> None:
    result = download_video(runtime, "ftp://exemplo.com/v")
    assert result.get("code") == "invalid_argument"


def test_download_folder_outside_workspace(runtime: Runtime) -> None:
    result = download_video(runtime, "https://youtu.be/abc", folder="../fora")
    assert result.get("code") == "outside_workspace"


@pytest.mark.ffmpeg
def test_download_folder_is_file(runtime: Runtime, sample_video: str) -> None:
    result = download_video(runtime, "https://youtu.be/abc", folder=sample_video)
    assert result.get("code") == "invalid_argument"
