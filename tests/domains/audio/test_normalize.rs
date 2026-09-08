use mcp_tools::core::errors::ErrorCode;
use mcp_tools::domains::audio::extract::{extract_audio, AudioFormat};
use mcp_tools::domains::audio::normalize::{normalize_audio, LoudnessPreset};
use mcp_tools::domains::video::probe::probe_video;

use crate::conftest::{runtime, sample_video};
use crate::helpers::unwrap;
use crate::skip_without_ffmpeg;

#[test]
fn test_normalize_video() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(normalize_audio(
        &rt.runtime,
        &sample,
        Some(LoudnessPreset::Tiktok),
        None,
        false,
    ));
    assert_eq!(result.output, "sample_normalized.mp4");
    assert!((result.target_lufs - -14.0).abs() < f64::EPSILON);
    assert!(result.is_video);
    assert!(result.measured_lufs.is_some());
    assert!(result.gain_db.is_some());
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert!(probed.info.has_video);
    assert!(probed.info.has_audio);
}

#[test]
fn test_normalize_audio_file_custom_target() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let audio = unwrap(extract_audio(&rt.runtime, &sample, AudioFormat::Mp3, false));
    let result = unwrap(normalize_audio(
        &rt.runtime,
        &audio.output,
        None,
        Some(-16.0),
        false,
    ));
    assert!(!result.is_video);
    assert!((result.target_lufs - -16.0).abs() < f64::EPSILON);
}

#[test]
fn test_normalize_invalid() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let error = normalize_audio(&rt.runtime, &sample, None, Some(5.0), false).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_normalize_missing() {
    let rt = runtime();
    let error = normalize_audio(&rt.runtime, "ghost.mp4", None, None, false).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}
