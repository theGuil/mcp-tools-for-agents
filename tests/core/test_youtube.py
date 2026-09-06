import pytest

from core.errors import ToolError
from core.youtube import parse_info, safe_title, validate_url


@pytest.mark.parametrize(
    "url",
    [
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        "https://youtu.be/dQw4w9WgXcQ",
        "  https://m.youtube.com/watch?v=abc  ",
        "https://www.youtube.com/shorts/abc123",
    ],
)
def test_validate_url_accepts_youtube(url: str) -> None:
    assert validate_url(url) == url.strip()


@pytest.mark.parametrize("url", ["https://vimeo.com/123", "ftp://youtube.com/x", "abc", ""])
def test_validate_url_rejects_others(url: str) -> None:
    with pytest.raises(ToolError) as exc:
        validate_url(url)
    assert exc.value.code == "invalid_argument"


def test_safe_title() -> None:
    assert safe_title("Olá, mundo! (2024) / teste") == "Ol_mundo_2024_teste"
    assert safe_title("///") == "video"
    assert len(safe_title("a" * 200)) == 60


def test_parse_info_full() -> None:
    info = parse_info(
        {
            "id": "abc",
            "title": "Título",
            "channel": "Canal",
            "duration": 120,
            "view_count": 10,
            "upload_date": "20240101",
            "description": "desc",
            "thumbnail": "http://x/y.jpg",
            "chapters": [{"title": "Intro", "start_time": 0, "end_time": 10}, "lixo"],
        }
    )
    assert info["title"] == "Título"
    assert info["duration"] == 120.0
    assert info["chapters"] == [{"title": "Intro", "start": 0.0, "end": 10.0}]


def test_parse_info_missing_fields() -> None:
    info = parse_info({})
    assert info["title"] == "video"
    assert info["channel"] is None
    assert info["duration"] == 0.0
    assert info["chapters"] == []
