import pytest

from core.errors import ToolError
from core.ffmpeg import FFmpeg, _parse_probe
from core.paths import Workspace
from tests.conftest import SAMPLE_DURATION


def test_unavailable_binary() -> None:
    ff = FFmpeg(ffmpeg_bin="nao-existe-ffmpeg", ffprobe_bin="nao-existe-ffprobe")
    assert not ff.is_available()
    with pytest.raises(ToolError) as exc:
        ff.require()
    assert exc.value.code == "unavailable"


def test_parse_probe_minimal(workspace: Workspace) -> None:
    fake = workspace.root / "x.mp4"
    fake.write_bytes(b"abc")
    raw = '{"format": {"duration": "1.5", "format_name": "mp4"}, "streams": []}'
    info = _parse_probe(raw, fake)
    assert info["duration"] == 1.5
    assert info["has_video"] is False
    assert info["size_bytes"] == 3


def test_parse_probe_invalid(workspace: Workspace) -> None:
    with pytest.raises(ToolError):
        _parse_probe("not json", workspace.root / "x")


@pytest.mark.ffmpeg
def test_probe_real_file(workspace: Workspace, sample_video: str) -> None:
    info = FFmpeg().probe(workspace.root / sample_video)
    assert abs(info["duration"] - SAMPLE_DURATION) < 0.2
    assert (info["width"], info["height"]) == (320, 240)
    assert info["fps"] == 25.0
    assert info["has_audio"] is True
    assert info["video_codec"] == "h264"
