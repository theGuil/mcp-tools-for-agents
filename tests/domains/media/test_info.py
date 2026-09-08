from __future__ import annotations

from typing import TYPE_CHECKING

from core.downloader import Downloader, VideoInfo
from core.errors import ToolError
from domains import Runtime
from domains.media.info import get_video_info
from tests.helpers import unwrap

if TYPE_CHECKING:
    import pytest

FAKE = VideoInfo(
    id="2345",
    title="Aula de matemática",
    extractor="Generic",
    channel=None,
    duration=90.0,
    view_count=None,
    upload_date=None,
    description="descrição",
    thumbnail=None,
    chapters=[],
)


def test_info_success(runtime: Runtime, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(Downloader, "info", lambda _self, _url: FAKE)
    result = unwrap(get_video_info(runtime, "https://eaulas.usp.br/portal/video?idItem=2345"))
    assert result["title"] == "Aula de matemática"
    assert result["extractor"] == "Generic"
    assert result["duration"] == 90.0


def test_info_aceita_qualquer_site(runtime: Runtime, monkeypatch: pytest.MonkeyPatch) -> None:
    vistas: list[str] = []

    def espia(_self: Downloader, url: str) -> VideoInfo:
        vistas.append(url)
        return FAKE

    monkeypatch.setattr(Downloader, "info", espia)
    unwrap(get_video_info(runtime, "https://vimeo.com/1"))
    unwrap(get_video_info(runtime, "https://exemplo.com.br/aula"))
    assert vistas == ["https://vimeo.com/1", "https://exemplo.com.br/aula"]


def test_info_invalid_url(runtime: Runtime) -> None:
    result = get_video_info(runtime, "ftp://exemplo.com/1")
    assert result.get("code") == "invalid_argument"


def test_info_download_error(runtime: Runtime, monkeypatch: pytest.MonkeyPatch) -> None:
    def boom(_self: Downloader, _url: str) -> VideoInfo:
        raise ToolError("privado", code="download_failed")

    monkeypatch.setattr(Downloader, "info", boom)
    result = get_video_info(runtime, "https://youtu.be/abc")
    assert result.get("code") == "download_failed"
