use std::time::Duration;

use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::jobs::JobStatus;
use mcp_tools::domains::video::banner::{add_banner, BannerPosition};

use crate::conftest::{runtime, sample_video};
use crate::helpers::{unwrap, unwrap_job};
use crate::skip_without_ffmpeg;

const TEXT: &str = "Nesta página mostramos apenas artistas sem auto-tune";

/// Chamada com os mesmos defaults do Python, variando só o que o teste pede.
struct Args {
    position: BannerPosition,
    color: &'static str,
    height_ratio: f64,
    offset_ratio: f64,
    font_size: Option<i64>,
    start: f64,
    end: Option<f64>,
    background: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            position: BannerPosition::Top,
            color: "blue",
            height_ratio: 0.07,
            offset_ratio: 0.0,
            font_size: None,
            start: 0.0,
            end: None,
            background: false,
        }
    }
}

fn call(
    rt: &crate::conftest::TestRuntime,
    path: &str,
    text: &str,
    args: Args,
) -> mcp_tools::core::errors::ToolResult<
    mcp_tools::core::jobs::MaybeJob<mcp_tools::domains::video::banner::AddBannerResult>,
> {
    add_banner(
        &rt.runtime,
        path,
        text,
        args.position,
        args.color,
        "white",
        args.height_ratio,
        args.offset_ratio,
        args.font_size,
        args.start,
        args.end,
        args.background,
    )
}

#[test]
fn test_banner_creates_file() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(call(
        &rt,
        &sample,
        TEXT,
        Args {
            color: "#1E40AF",
            font_size: Some(14),
            ..Args::default()
        },
    ));
    assert_eq!(result.output, "sample_banner.mp4");
    assert_eq!(result.band_height_px, 24);
    assert_eq!(result.offset_px, 0);
    assert_eq!(result.font_size, 14);
    let info = rt
        .runtime
        .ffmpeg
        .probe(&rt.runtime.workspace.existing(&result.output).unwrap())
        .unwrap();
    assert_eq!((info.width, info.height), (Some(320), Some(240)));
    assert!(info.has_audio);
}

#[test]
fn test_banner_offset_two_lines() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(call(
        &rt,
        &sample,
        "Nesta página mostramos apenas\nartistas sem auto-tune",
        Args {
            height_ratio: 0.2,
            offset_ratio: 0.15,
            ..Args::default()
        },
    ));
    assert_eq!(result.band_height_px, 48);
    assert_eq!(result.offset_px, 36);
    let size = std::fs::metadata(rt.runtime.workspace.existing(&result.output).unwrap())
        .unwrap()
        .len();
    assert!(size > 0);
}

#[test]
fn test_banner_bottom_interval_background() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let job = unwrap_job(call(
        &rt,
        &sample,
        "Rodapé: 100% ao vivo",
        Args {
            position: BannerPosition::Bottom,
            height_ratio: 0.2,
            offset_ratio: 0.25,
            start: 0.5,
            end: Some(2.0),
            background: true,
            ..Args::default()
        },
    ));
    let finished = rt
        .runtime
        .jobs
        .wait(&job.job_id, Some(Duration::from_secs(60)))
        .unwrap();
    assert_eq!(finished.status, JobStatus::Done);
    let result = finished.result.expect("resultado");
    assert_eq!(result["band_height_px"], 48);
    assert_eq!(result["offset_px"], 60);
}

#[test]
fn test_banner_empty_text() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let error = call(&rt, &sample, "   ", Args::default()).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_banner_bad_ratio() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let args = Args {
        height_ratio: 0.9,
        ..Args::default()
    };
    let error = call(&rt, &sample, TEXT, args).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_banner_bad_offset() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let args = Args {
        offset_ratio: 0.9,
        ..Args::default()
    };
    let error = call(&rt, &sample, TEXT, args).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_banner_bad_color() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let args = Args {
        color: "blue;rm",
        ..Args::default()
    };
    let error = call(&rt, &sample, TEXT, args).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_banner_bad_interval() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let args = Args {
        start: 2.0,
        end: Some(1.0),
        ..Args::default()
    };
    let error = call(&rt, &sample, TEXT, args).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_banner_start_after_end_of_video() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let args = Args {
        start: 50.0,
        ..Args::default()
    };
    let error = call(&rt, &sample, TEXT, args).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_banner_missing_file() {
    let rt = runtime();
    let error = call(&rt, "ghost.mp4", TEXT, Args::default()).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}
