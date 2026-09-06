from __future__ import annotations

from typing import TYPE_CHECKING

from core.errors import ToolError
from core.youtube import VideoInfo, YouTube
from domains import Runtime
from domains.youtube.info import get_youtube_info
from tests.helpers import unwrap

if TYPE_CHECKING:
    import pytest

FAKE = VideoInfo(
    id="abc",
    title="Vídeo teste",
    channel="Canal",
    duration=90.0,
    view_count=1,
    upload_date="20240101",
    description="descrição",
    thumbnail=None,
    chapters=[],
)


def test_info_success(runtime: Runtime, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(YouTube, "info", lambda _self, _url: FAKE)
    result = unwrap(get_youtube_info(runtime, "https://youtu.be/abc"))
    assert result["title"] == "Vídeo teste"
    assert result["duration"] == 90.0


def test_info_invalid_url(runtime: Runtime) -> None:
    result = get_youtube_info(runtime, "https://vimeo.com/1")
    assert result.get("code") == "invalid_argument"


def test_info_download_error(runtime: Runtime, monkeypatch: pytest.MonkeyPatch) -> None:
    def boom(_self: YouTube, _url: str) -> VideoInfo:
        raise ToolError("privado", code="download_failed")

    monkeypatch.setattr(YouTube, "info", boom)
    result = get_youtube_info(runtime, "https://youtu.be/abc")
    assert result.get("code") == "download_failed"
