from typing import TYPE_CHECKING

from core.freesound import Freesound, SoundCandidate
from domains import Runtime
from domains.media.search_sfx import search_sound_effects
from tests.helpers import unwrap

if TYPE_CHECKING:
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


def test_search_success(runtime: Runtime, monkeypatch: pytest.MonkeyPatch) -> None:
    seen: list[tuple[str, int, float | None]] = []

    def fake(
        _self: Freesound, query: str, *, limit: int = 5, max_duration: float | None = 10.0
    ) -> list[SoundCandidate]:
        seen.append((query, limit, max_duration))
        return [BOOM]

    monkeypatch.setattr(Freesound, "search", fake)
    result = unwrap(search_sound_effects(runtime, " vine boom ", limit=3, max_duration=6))
    assert result["query"] == "vine boom"
    assert result["count"] == 1
    assert result["sounds"][0]["sound_id"] == 785925
    assert seen == [(" vine boom ", 3, 6)]


def test_search_empty_query(runtime: Runtime) -> None:
    assert search_sound_effects(runtime, "  ").get("code") == "invalid_argument"


def test_search_invalid_limit(runtime: Runtime) -> None:
    assert search_sound_effects(runtime, "boom", limit=0).get("code") == "invalid_argument"
