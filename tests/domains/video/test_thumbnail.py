import pytest

from domains import Runtime
from domains.video.probe import probe_video
from domains.video.thumbnail import create_thumbnail
from tests.helpers import unwrap

pytestmark = pytest.mark.ffmpeg


def test_create_thumbnail_youtube(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(create_thumbnail(runtime, sample_video, title="Título de teste grande"))
    assert result["output"] == "sample_thumb_youtube.jpg"
    assert (result["width"], result["height"]) == (1280, 720)
    assert result["time"] == 1.5
    assert result["size_bytes"] > 0
    probed = unwrap(probe_video(runtime, result["output"]))
    assert probed["info"]["width"] == 1280


def test_create_thumbnail_vertical_custom(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(
        create_thumbnail(
            runtime,
            sample_video,
            time=0.2,
            title="Sem darken",
            thumbnail_format="vertical",
            position="bottom",
            text_color="#FFD700",
            darken=False,
            output="capas/capa.png",
        )
    )
    assert result["output"] == "capas/capa.png"
    assert (result["width"], result["height"]) == (1080, 1920)


def test_create_thumbnail_from_image(runtime: Runtime, sample_video: str) -> None:
    base = unwrap(create_thumbnail(runtime, sample_video, thumbnail_format="original"))
    assert (base["width"], base["height"]) == (320, 240)
    result = unwrap(create_thumbnail(runtime, base["output"], title="Capa"))
    assert result["time"] is None


def test_create_thumbnail_invalid(runtime: Runtime, sample_video: str) -> None:
    assert create_thumbnail(runtime, sample_video, time=99).get("code") == "invalid_argument"
    assert create_thumbnail(runtime, sample_video, title="x" * 81).get("code") == (
        "invalid_argument"
    )
    assert create_thumbnail(runtime, sample_video, text_color="red").get("code") == (
        "invalid_argument"
    )
    assert create_thumbnail(runtime, sample_video, output="capa.gif").get("code") == (
        "invalid_argument"
    )


def test_create_thumbnail_missing(runtime: Runtime) -> None:
    assert create_thumbnail(runtime, "ghost.mp4").get("code") == "not_found"
