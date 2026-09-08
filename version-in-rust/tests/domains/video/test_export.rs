use std::time::Duration;

use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::jobs::JobStatus;
use mcp_tools::domains::video::export::{export_for_platform, fit_size, Platform, Quality};
use mcp_tools::domains::video::probe::probe_video;

use crate::conftest::{runtime, sample_video};
use crate::helpers::{unwrap, unwrap_job};
use crate::skip_without_ffmpeg;

#[test]
fn test_fit_size() {
    assert_eq!(fit_size(3840, 2160, 1920, 1080), (1920, 1080));
    assert_eq!(fit_size(1080, 1920, 1080, 1920), (1080, 1920));
    assert_eq!(fit_size(640, 360, 1920, 1080), (640, 360));
    assert_eq!(fit_size(1500, 1000, 1080, 1920), (1080, 720));
}

#[test]
fn test_export_youtube() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(export_for_platform(
        &rt.runtime,
        &sample,
        Platform::Youtube,
        Quality::Standard,
        None,
        false,
    ));
    assert_eq!(result.output, "sample_youtube.mp4");
    assert_eq!((result.width, result.height), (320, 240));
    assert_eq!(result.fps, 25.0);
    assert!(result
        .warnings
        .iter()
        .any(|w| w.contains("Resolução baixa")));
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert_eq!(probed.info.video_codec.as_deref(), Some("h264"));
    assert_eq!(probed.info.audio_codec.as_deref(), Some("aac"));
}

#[test]
fn test_export_tiktok_warns_orientation() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(export_for_platform(
        &rt.runtime,
        &sample,
        Platform::Tiktok,
        Quality::High,
        Some("final/t.mp4"),
        false,
    ));
    assert_eq!(result.output, "final/t.mp4");
    assert!(result.warnings.iter().any(|w| w.contains("smart_crop")));
}

#[test]
fn test_export_invalid() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    // "vimeo" não existe no enum: a validação fica no parse do parâmetro.
    assert!(serde_json::from_str::<Platform>("\"vimeo\"").is_err());
    let error = export_for_platform(
        &rt.runtime,
        &sample,
        Platform::Youtube,
        Quality::Standard,
        Some("a.mkv"),
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_export_missing() {
    let rt = runtime();
    let error = export_for_platform(
        &rt.runtime,
        "ghost.mp4",
        Platform::Youtube,
        Quality::Standard,
        None,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn test_export_background() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let submitted = unwrap_job(export_for_platform(
        &rt.runtime,
        &sample,
        Platform::Youtube,
        Quality::Standard,
        None,
        true,
    ));
    let job = rt
        .runtime
        .jobs
        .wait(&submitted.job_id, Some(Duration::from_secs(60)))
        .unwrap();
    assert_eq!(job.status, JobStatus::Done);
}
