use std::process::Command;
use std::time::Duration;

use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::jobs::JobStatus;
use mcp_tools::core::paths::Workspace;
use mcp_tools::domains::video::background_music::add_background_music;
use mcp_tools::domains::video::probe::probe_video;

use crate::conftest::{runtime, sample_video};
use crate::helpers::{unwrap, unwrap_job};
use crate::skip_without_ffmpeg;

/// Gera 1s de música sintética (`trilha.mp3`) no workspace.
fn music(workspace: &Workspace) -> String {
    let target = workspace.root.join("trilha.mp3");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=220:duration=1",
            "-c:a",
            "libmp3lame",
        ])
        .arg(&target)
        .status()
        .expect("ffmpeg");
    assert!(status.success());
    "trilha.mp3".to_string()
}

#[test]
fn test_add_background_music_with_ducking() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let track = music(&rt.runtime.workspace);
    let result = unwrap(add_background_music(
        &rt.runtime,
        &sample,
        &track,
        0.2,
        true,
        1.0,
        2.0,
        0.0,
        true,
        false,
    ));
    assert_eq!(result.output, "sample_music.mp4");
    assert!(result.ducking);
    assert!(result.looped);
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert!(probed.info.has_audio);
    assert!((probed.info.duration - 3.0).abs() < 0.3);
}

#[test]
fn test_add_background_music_no_ducking_no_loop() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let track = music(&rt.runtime.workspace);
    let result = unwrap(add_background_music(
        &rt.runtime,
        &sample,
        &track,
        0.2,
        false,
        1.0,
        0.0,
        1.0,
        false,
        false,
    ));
    assert!(!result.ducking);
    assert!(!result.looped);
}

#[test]
fn test_add_background_music_invalid() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let track = music(&rt.runtime.workspace);
    let error = add_background_music(
        &rt.runtime,
        &sample,
        &track,
        0.0,
        true,
        1.0,
        2.0,
        0.0,
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let error = add_background_music(
        &rt.runtime,
        &sample,
        &track,
        0.2,
        true,
        -1.0,
        2.0,
        0.0,
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let error = add_background_music(
        &rt.runtime,
        &sample,
        &track,
        0.2,
        true,
        1.0,
        2.0,
        10.0,
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_add_background_music_missing() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let error = add_background_music(
        &rt.runtime,
        &sample,
        "ghost.mp3",
        0.2,
        true,
        1.0,
        2.0,
        0.0,
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
    let error = add_background_music(
        &rt.runtime,
        "ghost.mp4",
        &sample,
        0.2,
        true,
        1.0,
        2.0,
        0.0,
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn test_add_background_music_background() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let track = music(&rt.runtime.workspace);
    let submitted = unwrap_job(add_background_music(
        &rt.runtime,
        &sample,
        &track,
        0.2,
        true,
        1.0,
        2.0,
        0.0,
        true,
        true,
    ));
    let job = rt
        .runtime
        .jobs
        .wait(&submitted.job_id, Some(Duration::from_secs(60)))
        .unwrap();
    assert_eq!(job.status, JobStatus::Done);
}
