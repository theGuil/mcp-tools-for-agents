import json
import subprocess

import pytest

from domains import Runtime
from domains.video.metadata import set_video_metadata
from tests.helpers import unwrap

pytestmark = pytest.mark.ffmpeg


def test_set_metadata(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(
        set_video_metadata(
            runtime, sample_video, title="Meu título", description="Descrição longa", author="Eu"
        )
    )
    assert result["output"] == "sample_meta.mp4"
    raw = subprocess.run(
        [
            "ffprobe",
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            str(runtime.workspace.existing(result["output"])),
        ],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    tags = json.loads(raw)["format"]["tags"]
    assert tags["title"] == "Meu título"
    assert tags["description"] == "Descrição longa"
    assert tags["artist"] == "Eu"


def test_set_metadata_nothing(runtime: Runtime, sample_video: str) -> None:
    assert set_video_metadata(runtime, sample_video).get("code") == "invalid_argument"


def test_set_metadata_too_long(runtime: Runtime, sample_video: str) -> None:
    result = set_video_metadata(runtime, sample_video, description="x" * 6000)
    assert result.get("code") == "invalid_argument"


def test_set_metadata_missing(runtime: Runtime) -> None:
    assert set_video_metadata(runtime, "ghost.mp4", title="x").get("code") == "not_found"
