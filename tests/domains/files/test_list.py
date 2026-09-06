from domains import Runtime
from domains.files.delete import delete_file
from domains.files.listing import list_files
from tests.helpers import unwrap


def test_list_and_filter(runtime: Runtime) -> None:
    root = runtime.workspace.root
    (root / "a.mp4").write_bytes(b"1")
    (root / "sub").mkdir()
    (root / "sub" / "b.wav").write_bytes(b"22")
    (root / "notes.txt").write_bytes(b"333")

    everything = unwrap(list_files(runtime))
    assert [f["path"] for f in everything["files"]] == ["a.mp4", "notes.txt", "sub/b.wav"]
    assert everything["count"] == 3

    only_audio = unwrap(list_files(runtime, kind="audio"))
    assert [f["path"] for f in only_audio["files"]] == ["sub/b.wav"]

    shallow = unwrap(list_files(runtime, recursive=False))
    assert shallow["count"] == 2


def test_list_missing_dir(runtime: Runtime) -> None:
    assert list_files(runtime, "nope").get("code") == "not_found"


def test_delete(runtime: Runtime) -> None:
    target = runtime.workspace.root / "tmp.mp4"
    target.write_bytes(b"xyz")
    result = delete_file(runtime, "tmp.mp4")
    assert result.get("freed_bytes") == 3
    assert not target.exists()
    assert delete_file(runtime, "tmp.mp4").get("code") == "not_found"
