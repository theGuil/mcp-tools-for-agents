import subprocess
from pathlib import Path

import pytest

from core.freesound import Freesound, SoundCandidate
from core.paths import Workspace
from domains import Runtime
from domains.video.probe import probe_video
from domains.video.sound_effects import SoundEffect, add_sound_effects
from tests.helpers import unwrap, unwrap_job

pytestmark = pytest.mark.ffmpeg

BOOM = SoundCandidate(
    sound_id=785925,
    name="Drama Boom",
    duration=0.4,
    license="CC0",
    author="modusmogulus",
    tags=["boom"],
    preview_url="https://cdn.freesound.org/previews/785/785925-hq.mp3",
)
DING = SoundCandidate(
    sound_id=111,
    name="Ding",
    duration=0.3,
    license="CC BY",
    author="alguem",
    tags=["ding"],
    preview_url="https://cdn.freesound.org/previews/111/111-hq.mp3",
)


def _sine(target: Path, frequency: int, duration: float) -> None:
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
            f"sine=frequency={frequency}:duration={duration}",
            str(target),
        ],
        check=True,
    )


@pytest.fixture
def boom_file(workspace: Workspace) -> str:
    _sine(workspace.root / "boom.mp3", 110, 0.4)
    return "boom.mp3"


@pytest.fixture
def fake_freesound(monkeypatch: pytest.MonkeyPatch) -> list[str]:
    """Freesound falso: registra as chamadas e grava um MP3 sintético no lugar do preview."""
    calls: list[str] = []

    def search(
        _self: Freesound, query: str, *, limit: int = 5, max_duration: float | None = 10.0
    ) -> list[SoundCandidate]:
        del limit, max_duration
        calls.append(f"search:{query}")
        return [BOOM] if "boom" in query else []

    def sound(_self: Freesound, sound_id: int) -> SoundCandidate:
        calls.append(f"sound:{sound_id}")
        return DING

    def download(_self: Freesound, candidate: SoundCandidate, target_dir: Path) -> Path:
        calls.append(f"download:{candidate['sound_id']}")
        target_dir.mkdir(parents=True, exist_ok=True)
        target = target_dir / f"{candidate['name']}_{candidate['sound_id']}.mp3"
        if not target.exists():
            _sine(target, 880, candidate["duration"])
        return target

    monkeypatch.setattr(Freesound, "search", search)
    monkeypatch.setattr(Freesound, "sound", sound)
    monkeypatch.setattr(Freesound, "download_preview", download)
    return calls


def test_add_from_workspace_file(runtime: Runtime, sample_video: str, boom_file: str) -> None:
    effects = [SoundEffect(audio=boom_file, start=0.5), SoundEffect(audio=boom_file, start=2.0)]
    result = unwrap(add_sound_effects(runtime, sample_video, effects))
    assert result["output"] == "sample_sfx.mp4"
    assert result["original_volume"] == 1.0
    assert [e["start"] for e in result["effects"]] == [0.5, 2.0]
    assert result["effects"][0]["sound_id"] is None
    assert result["effects"][0]["duration"] > 0.3
    probed = unwrap(probe_video(runtime, result["output"]))
    assert probed["info"]["has_audio"]
    assert abs(probed["info"]["duration"] - 3.0) < 0.3


def test_add_from_query_and_sound_id(
    runtime: Runtime, sample_video: str, fake_freesound: list[str]
) -> None:
    effects = [
        SoundEffect(query="vine boom", start=1.0, volume=1.5),
        SoundEffect(sound_id=111, start=2.5),
    ]
    result = unwrap(add_sound_effects(runtime, sample_video, effects, original_volume=0.3))
    assert fake_freesound == ["search:vine boom", "download:785925", "sound:111", "download:111"]
    first, second = result["effects"]
    assert first["audio"] == "sfx/Drama Boom_785925.mp3"
    assert first["sound_id"] == 785925
    assert first["license"] == "CC0"
    assert first["volume"] == 1.5
    assert second["audio"] == "sfx/Ding_111.mp3"
    assert second["author"] == "alguem"
    assert (runtime.workspace.root / "sfx" / "Ding_111.mp3").is_file()


def test_add_without_original_audio(runtime: Runtime, sample_video: str, boom_file: str) -> None:
    result = unwrap(
        add_sound_effects(
            runtime, sample_video, [SoundEffect(audio=boom_file, start=1.0)], original_volume=0.0
        )
    )
    probed = unwrap(probe_video(runtime, result["output"]))
    assert probed["info"]["has_audio"]
    assert abs(probed["info"]["duration"] - 3.0) < 0.3


def test_add_background(runtime: Runtime, sample_video: str, boom_file: str) -> None:
    submitted = unwrap_job(
        add_sound_effects(
            runtime, sample_video, [SoundEffect(audio=boom_file, start=0.2)], background=True
        )
    )
    job = runtime.jobs.wait(submitted["job_id"], timeout=60)
    assert job.status == "done"
    assert job.result is not None
    assert job.result["output"] == "sample_sfx.mp4"


def test_add_query_without_results(
    runtime: Runtime, sample_video: str, fake_freesound: list[str]
) -> None:
    result = add_sound_effects(runtime, sample_video, [SoundEffect(query="xyz", start=1.0)])
    assert result.get("code") == "not_found"
    assert fake_freesound == ["search:xyz"]


def test_add_invalid(
    runtime: Runtime, sample_video: str, boom_file: str, fake_freesound: list[str]
) -> None:
    def code(effects: list[SoundEffect], original_volume: float = 1.0) -> str | None:
        result = add_sound_effects(runtime, sample_video, effects, original_volume=original_volume)
        return str(result.get("code")) if "code" in result else None

    assert code([]) == "invalid_argument"
    assert code([SoundEffect(audio=boom_file, start=-1)]) == "invalid_argument"
    assert code([SoundEffect(audio=boom_file, start=0, volume=0)]) == "invalid_argument"
    assert code([SoundEffect(audio=boom_file, start=0, volume=5)]) == "invalid_argument"
    assert code([SoundEffect(start=0)]) == "invalid_argument"
    assert code([SoundEffect(audio=boom_file, query="boom", start=0)]) == "invalid_argument"
    assert code([SoundEffect(audio=boom_file, start=0)], original_volume=2) == "invalid_argument"
    # start além do vídeo é barrado antes de qualquer busca na internet.
    assert code([SoundEffect(query="vine boom", start=50)]) == "invalid_argument"
    assert fake_freesound == []


def test_add_missing_files(runtime: Runtime, sample_video: str, boom_file: str) -> None:
    assert (
        add_sound_effects(runtime, "ghost.mp4", [SoundEffect(audio=boom_file, start=0)]).get("code")
        == "not_found"
    )
    assert (
        add_sound_effects(runtime, sample_video, [SoundEffect(audio="ghost.mp3", start=0)]).get(
            "code"
        )
        == "not_found"
    )
