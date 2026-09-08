use mcp_tools::core::errors::ErrorCode;
use mcp_tools::domains::files::delete::delete_file;
use mcp_tools::domains::files::listing::{list_files, FileKind};

use crate::conftest::runtime;

#[test]
fn test_list_and_filter() {
    let rt = runtime();
    let root = rt.root().to_path_buf();
    std::fs::write(root.join("a.mp4"), b"1").unwrap();
    std::fs::create_dir(root.join("sub")).unwrap();
    std::fs::write(root.join("sub").join("b.wav"), b"22").unwrap();
    std::fs::write(root.join("notes.txt"), b"333").unwrap();

    let everything = list_files(&rt.runtime, ".", None, true).unwrap();
    let paths: Vec<&str> = everything.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["a.mp4", "notes.txt", "sub/b.wav"]);
    assert_eq!(everything.count, 3);

    let only_audio = list_files(&rt.runtime, ".", Some(FileKind::Audio), true).unwrap();
    let paths: Vec<&str> = only_audio.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["sub/b.wav"]);

    let shallow = list_files(&rt.runtime, ".", None, false).unwrap();
    assert_eq!(shallow.count, 2);
}

#[test]
fn test_list_missing_dir() {
    let rt = runtime();
    let error = list_files(&rt.runtime, "nope", None, true).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn test_delete() {
    let rt = runtime();
    let target = rt.root().join("tmp.mp4");
    std::fs::write(&target, b"xyz").unwrap();
    let result = delete_file(&rt.runtime, "tmp.mp4").unwrap();
    assert_eq!(result.freed_bytes, 3);
    assert!(!target.exists());
    let error = delete_file(&rt.runtime, "tmp.mp4").unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}
