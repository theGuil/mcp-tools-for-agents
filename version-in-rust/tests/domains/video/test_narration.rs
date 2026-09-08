use std::process::Command;

use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::paths::Workspace;
use mcp_tools::domains::video::narration::add_narration;
use mcp_tools::domains::video::probe::probe_video;

use crate::conftest::{runtime, sample_video};
use crate::helpers::unwrap;
use crate::skip_without_ffmpeg;

/// Gera 1,5s de narração sintética (`voz.m4a`) no workspace.
fn narration(workspace: &Workspace) -> String {
    let target = workspace.root.join("voz.m4a");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:duration=1.5",
            "-c:a",
            "aac",
        ])
        .arg(&target)
        .status()
        .expect("ffmpeg");
    assert!(status.success());
    "voz.m4a".to_string()
}

#[test]
fn test_add_narration_mix() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let voice = narration(&rt.runtime.workspace);
    let result = unwrap(add_narration(&rt.runtime, &sample, &voice, 0.5, 0.2, false));
    assert_eq!(result.output, "sample_narrated.mp4");
    assert!(!result.replaced_audio);
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert!(probed.info.has_audio);
    assert!((probed.info.duration - 3.0).abs() < 0.3);
}

#[test]
fn test_add_narration_replace() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let voice = narration(&rt.runtime.workspace);
    let result = unwrap(add_narration(&rt.runtime, &sample, &voice, 0.0, 0.0, false));
    assert!(result.replaced_audio);
}

#[test]
fn test_add_narration_invalid() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let voice = narration(&rt.runtime.workspace);
    let error = add_narration(&rt.runtime, &sample, &voice, -1.0, 0.2, false).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let error = add_narration(&rt.runtime, &sample, &voice, 0.0, 2.0, false).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let error = add_narration(&rt.runtime, &sample, &voice, 50.0, 0.2, false).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_add_narration_missing() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let error = add_narration(&rt.runtime, &sample, "ghost.mp3", 0.0, 0.2, false).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}
