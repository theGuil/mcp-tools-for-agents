use mcp_tools::core::errors::ErrorCode;
use mcp_tools::domains::video::burn_subtitles::{burn_subtitles, BurnOptions};
use mcp_tools::domains::video::probe::probe_video;
use mcp_tools::domains::video::subtitles::{create_subtitles, format_timestamp, SubtitleSegment};

use crate::conftest::{runtime, sample_video};
use crate::helpers::unwrap;
use crate::skip_without_ffmpeg;

fn segments() -> Vec<SubtitleSegment> {
    vec![
        SubtitleSegment {
            start: 0.0,
            end: 1.2,
            text: "Primeira frase".to_string(),
        },
        SubtitleSegment {
            start: 1.2,
            end: 2.8,
            text: "Segunda frase".to_string(),
        },
    ]
}

#[test]
fn test_format_timestamp() {
    assert_eq!(format_timestamp(0.0), "00:00:00,000");
    assert_eq!(format_timestamp(3661.5), "01:01:01,500");
}

#[test]
fn test_create_subtitles() {
    let rt = runtime();
    let result = create_subtitles(&rt.runtime, &segments(), "legendas/teste.srt").unwrap();
    assert_eq!(result.output, "legendas/teste.srt");
    assert_eq!(result.segments, 2);
    let path = rt.runtime.workspace.existing("legendas/teste.srt").unwrap();
    let content = std::fs::read_to_string(path).unwrap();
    assert!(content.contains("00:00:01,200 --> 00:00:02,800\nSegunda frase"));
}

#[test]
fn test_create_subtitles_invalid() {
    let rt = runtime();
    let error = create_subtitles(&rt.runtime, &[], "a.srt").unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let error = create_subtitles(&rt.runtime, &segments(), "a.txt").unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let bad = [SubtitleSegment {
        start: 2.0,
        end: 1.0,
        text: "x".to_string(),
    }];
    let error = create_subtitles(&rt.runtime, &bad, "a.srt").unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let empty = [SubtitleSegment {
        start: 0.0,
        end: 1.0,
        text: "  ".to_string(),
    }];
    let error = create_subtitles(&rt.runtime, &empty, "a.srt").unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_create_subtitles_outside() {
    let rt = runtime();
    let error = create_subtitles(&rt.runtime, &segments(), "../a.srt").unwrap_err();
    assert_eq!(error.code, ErrorCode::OutsideWorkspace);
}

#[test]
fn test_burn_subtitles() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    create_subtitles(&rt.runtime, &segments(), "sample.srt").unwrap();
    let result = unwrap(burn_subtitles(
        &rt.runtime,
        &sample,
        "sample.srt",
        BurnOptions::default(),
        false,
    ));
    assert_eq!(result.output, "sample_subtitled.mp4");
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert!((probed.info.duration - 3.0).abs() < 0.2);
}

#[test]
fn test_burn_subtitles_wrong_extension() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let error =
        burn_subtitles(&rt.runtime, &sample, &sample, BurnOptions::default(), false).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_burn_subtitles_missing() {
    let rt = runtime();
    let error = burn_subtitles(
        &rt.runtime,
        "ghost.mp4",
        "x.srt",
        BurnOptions::default(),
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}
