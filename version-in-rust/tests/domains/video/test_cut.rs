use std::time::Duration;

use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::jobs::JobStatus;
use mcp_tools::domains::video::cut::cut_video;
use mcp_tools::domains::video::probe::probe_video;

use crate::conftest::{runtime, sample_video};
use crate::helpers::{unwrap, unwrap_job};
use crate::skip_without_ffmpeg;

#[test]
fn test_cut_creates_file() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(cut_video(&rt.runtime, &sample, 0.5, 2.0, true, false));
    assert_eq!(result.output, "sample_cut_0.5-2.mp4");
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert!((probed.info.duration - 1.5).abs() < 0.2);
}

#[test]
fn test_cut_invalid_range() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let error = cut_video(&rt.runtime, &sample, 2.0, 1.0, false, false).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_cut_beyond_duration() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let error = cut_video(&rt.runtime, &sample, 0.0, 99.0, false, false).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_cut_missing_file() {
    let rt = runtime();
    let error = cut_video(&rt.runtime, "ghost.mp4", 0.0, 1.0, false, false).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn test_cut_background() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let submitted = unwrap_job(cut_video(&rt.runtime, &sample, 0.0, 1.0, false, true));
    let job = rt
        .runtime
        .jobs
        .wait(&submitted.job_id, Some(Duration::from_secs(30)))
        .unwrap();
    assert_eq!(job.status, JobStatus::Done);
    let result = job.result.expect("resultado");
    assert!(result["output"]
        .as_str()
        .unwrap()
        .starts_with("sample_cut_"));
}
