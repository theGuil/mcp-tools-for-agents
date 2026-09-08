use std::time::Duration;

use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::jobs::JobStatus;
use mcp_tools::domains::video::probe::probe_video;
use mcp_tools::domains::video::text_overlay::{add_text_overlay, OverlayOptions};

use crate::conftest::{runtime, sample_video};
use crate::helpers::{unwrap, unwrap_job};
use crate::skip_without_ffmpeg;

#[test]
fn test_overlay_creates_file() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let options = OverlayOptions {
        end: Some(2.0),
        ..OverlayOptions::default()
    };
    let result = unwrap(add_text_overlay(
        &rt.runtime,
        &sample,
        "Olá: 'mundo'\\n100%",
        options,
        false,
    ));
    assert_eq!(result.output, "sample_text.mp4");
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert!((probed.info.duration - 3.0).abs() < 0.2);
    assert!(probed.info.has_audio);
}

#[test]
fn test_overlay_empty_text() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let error = add_text_overlay(
        &rt.runtime,
        &sample,
        "   ",
        OverlayOptions::default(),
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_overlay_bad_interval() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let options = OverlayOptions {
        start: 2.0,
        end: Some(1.0),
        ..OverlayOptions::default()
    };
    let error = add_text_overlay(&rt.runtime, &sample, "x", options, false).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let options = OverlayOptions {
        start: 99.0,
        ..OverlayOptions::default()
    };
    let error = add_text_overlay(&rt.runtime, &sample, "x", options, false).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_overlay_missing_file() {
    let rt = runtime();
    let error = add_text_overlay(
        &rt.runtime,
        "ghost.mp4",
        "x",
        OverlayOptions::default(),
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn test_overlay_background() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let submitted = unwrap_job(add_text_overlay(
        &rt.runtime,
        &sample,
        "bg",
        OverlayOptions::default(),
        true,
    ));
    let job = rt
        .runtime
        .jobs
        .wait(&submitted.job_id, Some(Duration::from_secs(60)))
        .unwrap();
    assert_eq!(job.status, JobStatus::Done);
}
