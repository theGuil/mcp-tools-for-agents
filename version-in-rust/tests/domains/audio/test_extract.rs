use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::ffmpeg::FFmpeg;
use mcp_tools::domains::audio::extract::{extract_audio, AudioFormat};

use crate::conftest::{runtime, sample_video};
use crate::helpers::unwrap;
use crate::skip_without_ffmpeg;

#[test]
fn test_extract_wav() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(extract_audio(&rt.runtime, &sample, AudioFormat::Wav, false));
    assert_eq!(result.output, "sample_audio.wav");
    let info = FFmpeg::default()
        .probe(&rt.root().join(&result.output))
        .unwrap();
    assert!(info.has_audio);
    assert!(!info.has_video);
}

#[test]
fn test_extract_missing() {
    let rt = runtime();
    let error = extract_audio(&rt.runtime, "nada.mp4", AudioFormat::Mp3, false).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}
