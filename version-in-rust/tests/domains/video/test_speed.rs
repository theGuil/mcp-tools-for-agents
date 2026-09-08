use mcp_tools::core::errors::ErrorCode;
use mcp_tools::domains::video::probe::probe_video;
use mcp_tools::domains::video::speed::{atempo_chain, change_speed};

use crate::conftest::{runtime, sample_video};
use crate::helpers::unwrap;
use crate::skip_without_ffmpeg;

#[test]
fn test_atempo_chain() {
    assert_eq!(atempo_chain(2.0), "atempo=2");
    assert_eq!(atempo_chain(0.25), "atempo=0.5,atempo=0.5");
    assert_eq!(atempo_chain(0.4), "atempo=0.5,atempo=0.8");
}

#[test]
fn test_change_speed_whole() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(change_speed(&rt.runtime, &sample, 2.0, None, None, false));
    assert_eq!(result.output, "sample_speed_2x.mp4");
    assert_eq!(result.new_duration, 1.5);
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert!((probed.info.duration - 1.5).abs() < 0.3);
}

#[test]
fn test_change_speed_segment() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(change_speed(
        &rt.runtime,
        &sample,
        0.5,
        Some(1.0),
        Some(2.0),
        false,
    ));
    assert_eq!(result.new_duration, 4.0);
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert!((probed.info.duration - 4.0).abs() < 0.4);
}

#[test]
fn test_change_speed_invalid() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let cases: [(f64, Option<f64>, Option<f64>); 5] = [
        (1.0, None, None),
        (20.0, None, None),
        (2.0, Some(1.0), None),
        (2.0, Some(2.0), Some(1.0)),
        (2.0, Some(0.0), Some(99.0)),
    ];
    for (factor, start, end) in cases {
        let error = change_speed(&rt.runtime, &sample, factor, start, end, false).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
}

#[test]
fn test_change_speed_missing() {
    let rt = runtime();
    let error = change_speed(&rt.runtime, "ghost.mp4", 2.0, None, None, false).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}
