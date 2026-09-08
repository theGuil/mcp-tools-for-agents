use mcp_tools::core::errors::ErrorCode;
use mcp_tools::domains::video::fade::{add_fade, FadeColor};
use mcp_tools::domains::video::probe::probe_video;

use crate::conftest::{runtime, sample_video};
use crate::helpers::unwrap;
use crate::skip_without_ffmpeg;

#[test]
fn test_add_fade() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(add_fade(
        &rt.runtime,
        &sample,
        0.5,
        0.5,
        FadeColor::Black,
        true,
        false,
    ));
    assert_eq!(result.output, "sample_fade.mp4");
    assert!(result.audio_faded);
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert!((probed.info.duration - 3.0).abs() < 0.3);
}

#[test]
fn test_add_fade_only_out_white() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(add_fade(
        &rt.runtime,
        &sample,
        0.0,
        1.0,
        FadeColor::White,
        true,
        false,
    ));
    assert_eq!(result.color, FadeColor::White);
    assert_eq!(result.fade_in, 0.0);
}

#[test]
fn test_add_fade_invalid() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let error = add_fade(
        &rt.runtime,
        &sample,
        0.0,
        0.0,
        FadeColor::Black,
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let error = add_fade(
        &rt.runtime,
        &sample,
        -1.0,
        1.0,
        FadeColor::Black,
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let error = add_fade(
        &rt.runtime,
        &sample,
        2.0,
        2.0,
        FadeColor::Black,
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_add_fade_missing() {
    let rt = runtime();
    let error = add_fade(
        &rt.runtime,
        "ghost.mp4",
        0.5,
        1.0,
        FadeColor::Black,
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}
