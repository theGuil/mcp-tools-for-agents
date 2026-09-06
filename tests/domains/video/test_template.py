import subprocess

import pytest

from core.paths import Workspace
from domains import Runtime
from domains.video.probe import probe_video
from domains.video.template import TEMPLATES, apply_template, list_templates
from tests.helpers import unwrap, unwrap_job

pytestmark = pytest.mark.ffmpeg


@pytest.fixture
def logo(workspace: Workspace) -> str:
    target = workspace.root / "logo.png"
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
            "color=c=red:size=200x80:duration=0.1",
            "-frames:v",
            "1",
            str(target),
        ],
        check=True,
    )
    return "logo.png"


def test_list_templates(runtime: Runtime) -> None:
    result = unwrap(list_templates(runtime))
    assert [t["name"] for t in result["templates"]] == [t["name"] for t in TEMPLATES]


def test_shorts_with_title(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(apply_template(runtime, sample_video, "shorts", title="Meu Short"))
    assert result["output"] == "sample_shorts.mp4"
    probed = unwrap(probe_video(runtime, result["output"]))
    assert (probed["info"]["width"], probed["info"]["height"]) == (1080, 1920)
    assert probed["info"]["has_audio"]


def test_square_and_landscape(runtime: Runtime, sample_video: str) -> None:
    square = unwrap(apply_template(runtime, sample_video, "square"))
    info = unwrap(probe_video(runtime, square["output"]))["info"]
    assert (info["width"], info["height"]) == (1080, 1080)
    land = unwrap(apply_template(runtime, sample_video, "landscape", title="T"))
    info = unwrap(probe_video(runtime, land["output"]))["info"]
    assert (info["width"], info["height"]) == (1920, 1080)


def test_intro_title(runtime: Runtime, sample_video: str) -> None:
    result = unwrap(apply_template(runtime, sample_video, "intro_title", title="Abertura"))
    assert (result["width"], result["height"]) == (320, 240)


def test_intro_title_requires_title(runtime: Runtime, sample_video: str) -> None:
    assert apply_template(runtime, sample_video, "intro_title").get("code") == "invalid_argument"


def test_watermark(runtime: Runtime, sample_video: str, logo: str) -> None:
    result = unwrap(apply_template(runtime, sample_video, "watermark", logo_path=logo))
    assert result["output"] == "sample_watermark.mp4"


def test_watermark_requires_logo(runtime: Runtime, sample_video: str) -> None:
    assert apply_template(runtime, sample_video, "watermark").get("code") == "invalid_argument"
    result = apply_template(runtime, sample_video, "watermark", logo_path="ghost.png")
    assert result.get("code") == "not_found"


def test_unknown_template(runtime: Runtime, sample_video: str) -> None:
    result = apply_template(runtime, sample_video, "nope")  # type: ignore[arg-type]
    assert result.get("code") == "invalid_argument"


def test_template_background(runtime: Runtime, sample_video: str) -> None:
    submitted = unwrap_job(apply_template(runtime, sample_video, "square", background=True))
    job = runtime.jobs.wait(submitted["job_id"], timeout=120)
    assert job.status == "done"
