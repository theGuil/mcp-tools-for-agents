use mcp_tools::core::errors::ErrorCode;

use crate::conftest::workspace;

#[test]
fn test_resolve_inside() {
    let ws = workspace();
    let resolved = ws.workspace.resolve("a/b.mp4").unwrap();
    assert_eq!(resolved, ws.workspace.root.join("a").join("b.mp4"));
}

#[test]
fn test_resolve_rejects_escape() {
    let ws = workspace();
    for bad in ["../fora.mp4", "/etc/passwd", "a/../../x"] {
        let error = ws.workspace.resolve(bad).unwrap_err();
        assert_eq!(error.code, ErrorCode::OutsideWorkspace, "{bad}");
    }
}

#[test]
fn test_existing_missing() {
    let ws = workspace();
    let error = ws.workspace.existing("nao.mp4").unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn test_output_for_is_unique() {
    let ws = workspace();
    let src = ws.workspace.root.join("v.mp4");
    std::fs::write(&src, b"").unwrap();
    let first = ws.workspace.output_for(&src, "cut 0-10", None);
    assert_eq!(first.file_name().unwrap(), "v_cut_0-10.mp4");
    std::fs::write(&first, b"").unwrap();
    let second = ws.workspace.output_for(&src, "cut 0-10", None);
    assert_eq!(second.file_name().unwrap(), "v_cut_0-10_1.mp4");
}

#[test]
fn test_output_for_extension() {
    let ws = workspace();
    let src = ws.workspace.root.join("v.mp4");
    let out = ws.workspace.output_for(&src, "frame", Some("png"));
    assert_eq!(out.extension().unwrap(), "png");
}
