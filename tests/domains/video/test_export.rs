use std::time::Duration;

use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::jobs::JobStatus;
use std::collections::HashSet;

use mcp_tools::core::ffmpeg::parse_encoders;
use mcp_tools::domains::video::export::{
    choose_encoders, codec_warning, encoder_args, export_for_platform, fit_size, EncoderMode,
    ExportOptions, Platform, Quality, VideoCodec,
};
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
        ExportOptions::default(),
        None,
        false,
    ));
    assert_eq!(result.output, "sample_youtube.mp4");
    assert_eq!(result.video_codec, VideoCodec::H264);
    assert!(!result.encoder.is_empty());
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
        ExportOptions {
            quality: Quality::High,
            ..ExportOptions::default()
        },
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
        ExportOptions::default(),
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
        ExportOptions::default(),
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
        ExportOptions::default(),
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

const ENCODERS: &str = "Encoders:
 V..... = Video
 A..... = Audio
 ------
 V....D libx264              libx264 H.264 / AVC / MPEG-4 AVC / MPEG-4 part 10 (codec h264)
 V....D h264_nvenc           NVIDIA NVENC H.264 encoder (codec h264)
 V....D libx265              libx265 H.265 / HEVC (codec hevc)
 V..... libsvtav1            SVT-AV1 (codec av1)
 A....D aac                  AAC (Advanced Audio Coding)
";

#[test]
fn test_parse_encoders_keeps_only_video() {
    let encoders = parse_encoders(ENCODERS);
    assert!(encoders.contains("libx264"));
    assert!(encoders.contains("h264_nvenc"));
    assert!(!encoders.contains("aac"));
    assert!(!encoders.contains("="));
    assert_eq!(encoders.len(), 4);
}

#[test]
fn test_choose_encoders_orders_hardware_first_in_auto() {
    let available = parse_encoders(ENCODERS);
    let auto = choose_encoders(&available, VideoCodec::H264, EncoderMode::Auto).unwrap();
    let names: Vec<&str> = auto.iter().map(|c| c.name.as_str()).collect();
    if cfg!(target_os = "macos") {
        assert_eq!(names, ["libx264"]);
    } else {
        assert_eq!(names, ["h264_nvenc", "libx264"]);
        assert!(auto[0].hardware);
        assert!(!auto[1].hardware);
    }
    let software = choose_encoders(&available, VideoCodec::Av1, EncoderMode::Software).unwrap();
    assert_eq!(software[0].name, "libsvtav1");
    let error = choose_encoders(&available, VideoCodec::H265, EncoderMode::Hardware).unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    let error = choose_encoders(&HashSet::new(), VideoCodec::H264, EncoderMode::Auto).unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
}

#[test]
fn test_encoder_args_translate_quality() {
    let x264 = encoder_args("libx264", VideoCodec::H264, 20);
    assert!(x264.iter().any(|a| a == "-crf"));
    assert!(x264.iter().any(|a| a == "20"));
    let x265 = encoder_args("libx265", VideoCodec::H265, 20);
    assert!(x265.iter().any(|a| a == "25"));
    assert!(x265.iter().any(|a| a == "hvc1"));
    let nvenc = encoder_args("h264_nvenc", VideoCodec::H264, 20);
    assert!(nvenc.iter().any(|a| a == "-cq"));
    let vt = encoder_args("h264_videotoolbox", VideoCodec::H264, 20);
    assert!(vt.iter().any(|a| a == "60"));
}

#[test]
fn test_codec_warning_by_platform() {
    assert!(codec_warning(Platform::Twitter, VideoCodec::H265).is_some());
    assert!(codec_warning(Platform::Tiktok, VideoCodec::Av1).is_some());
    assert!(codec_warning(Platform::Youtube, VideoCodec::Av1).is_none());
    assert!(codec_warning(Platform::Tiktok, VideoCodec::H265).is_none());
}

#[test]
fn test_export_h265_software() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(export_for_platform(
        &rt.runtime,
        &sample,
        Platform::Youtube,
        ExportOptions {
            codec: VideoCodec::H265,
            encoder: EncoderMode::Software,
            ..ExportOptions::default()
        },
        Some("final/h265.mp4"),
        false,
    ));
    assert_eq!(result.video_codec, VideoCodec::H265);
    assert_eq!(result.encoder, "libx265");
    assert!(!result.hardware);
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert_eq!(probed.info.video_codec.as_deref(), Some("hevc"));
}

#[test]
fn test_export_auto_falls_back_to_software_without_gpu() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    // Sem placa de vídeo o encoder de hardware falha e o software assume.
    let result = unwrap(export_for_platform(
        &rt.runtime,
        &sample,
        Platform::Twitter,
        ExportOptions::default(),
        None,
        false,
    ));
    assert_eq!(result.video_codec, VideoCodec::H264);
    assert!(result.encode_seconds >= 0.0);
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert_eq!(probed.info.video_codec.as_deref(), Some("h264"));
}
