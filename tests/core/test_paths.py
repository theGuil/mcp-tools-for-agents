from pathlib import Path

import pytest

from core.errors import ToolError
from core.paths import Workspace


def test_resolve_inside(workspace: Workspace) -> None:
    assert workspace.resolve("a/b.mp4") == workspace.root / "a" / "b.mp4"


@pytest.mark.parametrize("bad", ["../fora.mp4", "/etc/passwd", "a/../../x"])
def test_resolve_rejects_escape(workspace: Workspace, bad: str) -> None:
    with pytest.raises(ToolError) as exc:
        workspace.resolve(bad)
    assert exc.value.code == "outside_workspace"


def test_existing_missing(workspace: Workspace) -> None:
    with pytest.raises(ToolError) as exc:
        workspace.existing("nao.mp4")
    assert exc.value.code == "not_found"


def test_output_for_is_unique(workspace: Workspace) -> None:
    src = workspace.root / "v.mp4"
    src.touch()
    first = workspace.output_for(src, "cut 0-10")
    assert first.name == "v_cut_0-10.mp4"
    first.touch()
    second = workspace.output_for(src, "cut 0-10")
    assert second.name == "v_cut_0-10_1.mp4"


def test_output_for_extension(workspace: Workspace) -> None:
    src = Path(workspace.root / "v.mp4")
    assert workspace.output_for(src, "frame", extension="png").suffix == ".png"
