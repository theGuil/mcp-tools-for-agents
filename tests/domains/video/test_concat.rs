use mcp_tools::core::errors::ErrorCode;
use mcp_tools::domains::video::concat::{concat_videos, transition_offsets, TransitionName};
use mcp_tools::domains::video::cut::cut_video;
use mcp_tools::domains::video::probe::probe_video;

use crate::conftest::{runtime, sample_video, TestRuntime};
use crate::helpers::unwrap;
use crate::skip_without_ffmpeg;

#[test]
fn test_transition_offsets() {
    assert_eq!(transition_offsets(&[3.0, 3.0, 3.0], 0.5), vec![2.5, 5.0]);
}

fn clips(rt: &TestRuntime) -> Vec<String> {
    let sample = sample_video(&rt.runtime.workspace);
    let first = unwrap(cut_video(&rt.runtime, &sample, 0.0, 1.5, true, false));
    let second = unwrap(cut_video(&rt.runtime, &sample, 1.5, 3.0, true, false));
    vec![first.output, second.output]
}

#[test]
fn test_concat_copy() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let clips = clips(&rt);
    let result = unwrap(concat_videos(&rt.runtime, &clips, None, None, 0.5, false));
    assert_eq!(result.count, 2);
    assert!(result.transition.is_none());
    assert!(result.has_audio);
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert!((probed.info.duration - 3.0).abs() < 0.3);
}

#[test]
fn test_concat_with_transition() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let clips = clips(&rt);
    let result = unwrap(concat_videos(
        &rt.runtime,
        &clips,
        Some("f.mp4"),
        Some(TransitionName::Fade),
        0.5,
        false,
    ));
    assert_eq!(result.output, "f.mp4");
    assert_eq!(result.transition, Some(TransitionName::Fade));
    assert!(result.has_audio);
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert!((probed.info.duration - 2.5).abs() < 0.3);
}

#[test]
fn test_concat_transition_invalid() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let clips = clips(&rt);
    let error = concat_videos(
        &rt.runtime,
        &clips,
        None,
        Some(TransitionName::Fade),
        2.0,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let error = concat_videos(&rt.runtime, &clips[..1], None, None, 0.5, false).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_concat_missing() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let paths = vec![sample, "ghost.mp4".to_string()];
    let error = concat_videos(&rt.runtime, &paths, None, None, 0.5, false).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}
