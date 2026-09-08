from typing import TYPE_CHECKING

from core.errors import ToolError
from core.freesound import Freesound, SoundCandidate
from domains import Runtime
from domains.media.download_sfx import download_sound_effect
from tests.helpers import unwrap

if TYPE_CHECKING:
    from pathlib import Path

    import pytest

BOOM = SoundCandidate(
    sound_id=785925,
    name="Drama Boom",
    duration=4.4,
    license="CC0",
    author="modusmogulus",
    tags=["boom"],
    preview_url="https://cdn.freesound.org/previews/785/785925-hq.mp3",
)


def _fake_freesound(monkeypatch: pytest.MonkeyPatch) -> None:
    def sound(_self: Freesound, sound_id: int) -> SoundCandidate:
        if sound_id != BOOM["sound_id"]:
            raise ToolError("Som não encontrado no Freesound.", code="not_found")
        return BOOM

    def download(_self: Freesound, candidate: SoundCandidate, target_dir: Path) -> Path:
        target_dir.mkdir(parents=True, exist_ok=True)
        target = target_dir / f"Drama_Boom_{candidate['sound_id']}.mp3"
        target.write_bytes(b"mp3")
        return target

    monkeypatch.setattr(Freesound, "sound", sound)
    monkeypatch.setattr(Freesound, "download_preview", download)


def test_download_success(runtime: Runtime, monkeypatch: pytest.MonkeyPatch) -> None:
    _fake_freesound(monkeypatch)
    result = unwrap(download_sound_effect(runtime, 785925))
    assert result["output"] == "sfx/Drama_Boom_785925.mp3"
    assert result["name"] == "Drama Boom"
    assert result["license"] == "CC0"
    assert (runtime.workspace.root / "sfx" / "Drama_Boom_785925.mp3").is_file()


def test_download_custom_folder(runtime: Runtime, monkeypatch: pytest.MonkeyPatch) -> None:
    _fake_freesound(monkeypatch)
    result = unwrap(download_sound_effect(runtime, 785925, folder="sons/memes"))
    assert result["output"] == "sons/memes/Drama_Boom_785925.mp3"


def test_download_not_found(runtime: Runtime, monkeypatch: pytest.MonkeyPatch) -> None:
    _fake_freesound(monkeypatch)
    assert download_sound_effect(runtime, 1).get("code") == "not_found"


def test_download_invalid_id(runtime: Runtime) -> None:
    assert download_sound_effect(runtime, -5).get("code") == "invalid_argument"


def test_download_folder_outside_workspace(runtime: Runtime) -> None:
    assert download_sound_effect(runtime, 785925, folder="../fora").get("code") == (
        "outside_workspace"
    )


def test_download_folder_is_file(runtime: Runtime, monkeypatch: pytest.MonkeyPatch) -> None:
    _fake_freesound(monkeypatch)
    (runtime.workspace.root / "arquivo.txt").write_text("x")
    assert download_sound_effect(runtime, 785925, folder="arquivo.txt").get("code") == (
        "invalid_argument"
    )
