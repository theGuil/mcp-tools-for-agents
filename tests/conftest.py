"""Fixtures compartilhadas: workspace temporário e vídeo de exemplo."""

from __future__ import annotations

import subprocess
from collections.abc import Generator
from pathlib import Path

import pytest

from config import Settings
from core.ffmpeg import FFmpeg
from core.jobs import JobManager
from core.paths import Workspace
from core.youtube import YouTube
from domains import Runtime

SAMPLE_DURATION = 3.0


def pytest_collection_modifyitems(config: pytest.Config, items: list[pytest.Item]) -> None:
    """Pula testes marcados com ``ffmpeg`` quando o binário não está instalado."""
    del config
    if FFmpeg().is_available():
        return
    skip = pytest.mark.skip(reason="ffmpeg/ffprobe não instalados")
    for item in items:
        if "ffmpeg" in item.keywords:
            item.add_marker(skip)


@pytest.fixture
def workspace(tmp_path: Path) -> Workspace:
    """Workspace isolado por teste."""
    return Workspace.at(tmp_path / "ws")


@pytest.fixture
def jobs() -> Generator[JobManager]:
    """Gerenciador de jobs com uma thread."""
    manager = JobManager(workers=1)
    yield manager
    manager.shutdown()


@pytest.fixture
def runtime(workspace: Workspace, jobs: JobManager) -> Runtime:
    """Runtime completo apontando para o workspace temporário."""
    settings = Settings.from_env({"WORKSPACE_DIR": str(workspace.root)})
    return Runtime(
        settings=settings, workspace=workspace, ffmpeg=FFmpeg(), jobs=jobs, youtube=YouTube()
    )


@pytest.fixture
def sample_video(workspace: Workspace) -> str:
    """Gera um vídeo sintético de 3s com áudio dentro do workspace."""
    target = workspace.root / "sample.mp4"
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
            f"testsrc=size=320x240:rate=25:duration={SAMPLE_DURATION}",
            "-f",
            "lavfi",
            "-i",
            f"sine=frequency=440:duration={SAMPLE_DURATION}",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-g",
            "12",
            "-c:a",
            "aac",
            str(target),
        ],
        check=True,
    )
    return "sample.mp4"
