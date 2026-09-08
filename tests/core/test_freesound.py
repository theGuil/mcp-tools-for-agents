from pathlib import Path
from urllib.error import HTTPError

import pytest

from core.errors import ToolError
from core.freesound import (
    Freesound,
    SoundCandidate,
    parse_search,
    parse_sound,
    safe_name,
    short_license,
)

RAW_SOUND: dict[str, object] = {
    "id": 785925,
    "name": "Drama Boom",
    "tags": ["boom", "comedy", 42],
    "license": "http://creativecommons.org/publicdomain/zero/1.0/",
    "duration": 4.42308,
    "username": "modusmogulus",
    "previews": {
        "preview-hq-mp3": "https://cdn.freesound.org/previews/785/785925_15956618-hq.mp3",
        "preview-lq-mp3": "https://cdn.freesound.org/previews/785/785925_15956618-lq.mp3",
    },
}


def test_parse_sound() -> None:
    candidate = parse_sound(RAW_SOUND)
    assert candidate is not None
    assert candidate["sound_id"] == 785925
    assert candidate["name"] == "Drama Boom"
    assert candidate["license"] == "CC0"
    assert candidate["author"] == "modusmogulus"
    assert candidate["tags"] == ["boom", "comedy"]
    assert candidate["preview_url"].endswith("-hq.mp3")
    assert abs(candidate["duration"] - 4.42308) < 1e-6


def test_parse_sound_sem_preview() -> None:
    assert parse_sound({"id": 1, "previews": {}}) is None
    assert parse_sound({"id": "x", "previews": {"preview-hq-mp3": "https://a/b.mp3"}}) is None


def test_parse_search_ignora_lixo() -> None:
    data: dict[str, object] = {"results": [RAW_SOUND, "lixo", {"id": 2, "previews": {}}]}
    assert [c["sound_id"] for c in parse_search(data)] == [785925]
    assert parse_search({"results": None}) == []


def test_short_license() -> None:
    assert short_license("http://creativecommons.org/licenses/by/4.0/") == "CC BY"
    assert short_license("http://creativecommons.org/licenses/by-nc/4.0/") == "CC BY-NC"
    assert short_license("") == "desconhecida"


def test_safe_name() -> None:
    assert safe_name("Vine Boom (loud!) v2.wav") == "Vine_Boom_loud_v2"
    assert safe_name("Soft Bell - LowDing.mp3") == "Soft_Bell_-_LowDing"
    assert safe_name("///") == "sound"


def test_search_monta_url(monkeypatch: pytest.MonkeyPatch) -> None:
    urls: list[str] = []

    def fake_get_json(_self: Freesound, url: str) -> dict[str, object]:
        urls.append(url)
        return {"results": [RAW_SOUND]}

    monkeypatch.setattr(Freesound, "_get_json", fake_get_json)
    found = Freesound(api_key="abc").search("vine boom", limit=3, max_duration=8)
    assert found[0]["sound_id"] == 785925
    assert len(urls) == 1
    assert urls[0].startswith("https://freesound.org/apiv2/search/text/?")
    assert "query=vine+boom" in urls[0]
    assert "page_size=3" in urls[0]
    assert "token=abc" in urls[0]
    assert "filter=duration%3A%5B0+TO+8%5D" in urls[0]


def test_search_argumentos_invalidos() -> None:
    client = Freesound(api_key="abc")
    with pytest.raises(ToolError) as excinfo:
        client.search("   ")
    assert excinfo.value.code == "invalid_argument"
    with pytest.raises(ToolError):
        client.search("boom", limit=0)
    with pytest.raises(ToolError):
        client.search("boom", max_duration=0)


def test_sem_chave() -> None:
    with pytest.raises(ToolError) as excinfo:
        Freesound(api_key="  ").search("boom")
    assert excinfo.value.code == "unavailable"


def test_sound_id_invalido() -> None:
    with pytest.raises(ToolError) as excinfo:
        Freesound(api_key="abc").sound(0)
    assert excinfo.value.code == "invalid_argument"


def test_download_preview_grava_e_reaproveita(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    calls: list[str] = []

    def fake_get_bytes(_self: Freesound, url: str) -> bytes:
        calls.append(url)
        return b"mp3-fake"

    monkeypatch.setattr(Freesound, "_get_bytes", fake_get_bytes)
    candidate = parse_sound(RAW_SOUND)
    assert candidate is not None
    client = Freesound(api_key="abc")
    first = client.download_preview(candidate, tmp_path / "sfx")
    assert first == tmp_path / "sfx" / "Drama_Boom_785925.mp3"
    assert first.read_bytes() == b"mp3-fake"
    second = client.download_preview(candidate, tmp_path / "sfx")
    assert second == first
    assert calls == [candidate["preview_url"]]


def test_download_preview_vazio(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(Freesound, "_get_bytes", lambda _self, _url: b"")
    candidate = SoundCandidate(
        sound_id=1,
        name="x",
        duration=1.0,
        license="CC0",
        author="a",
        tags=[],
        preview_url="https://cdn.freesound.org/x.mp3",
    )
    with pytest.raises(ToolError) as excinfo:
        Freesound(api_key="abc").download_preview(candidate, tmp_path)
    assert excinfo.value.code == "download_failed"


def test_rejeita_url_sem_https() -> None:
    with pytest.raises(ToolError) as excinfo:
        Freesound(api_key="abc")._get_bytes("file:///etc/passwd")  # noqa: SLF001
    assert excinfo.value.code == "download_failed"


@pytest.mark.parametrize(
    ("status", "code"),
    [(401, "unavailable"), (404, "not_found"), (429, "download_failed"), (500, "download_failed")],
)
def test_erros_http(status: int, code: str, monkeypatch: pytest.MonkeyPatch) -> None:
    def boom(*_args: object, **_kwargs: object) -> object:
        raise HTTPError("https://freesound.org", status, "erro", {}, None)  # type: ignore[arg-type]

    monkeypatch.setattr("core.freesound.urlopen", boom)
    with pytest.raises(ToolError) as excinfo:
        Freesound(api_key="abc").sound(123)
    assert excinfo.value.code == code
