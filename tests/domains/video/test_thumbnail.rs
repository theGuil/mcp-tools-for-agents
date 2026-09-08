use mcp_tools::core::errors::ErrorCode;
use mcp_tools::domains::video::probe::probe_video;
use mcp_tools::domains::video::thumbnail::{
    create_thumbnail, TextPosition, ThumbnailFormat, ThumbnailOptions,
};

use crate::conftest::{runtime, sample_video};
use crate::skip_without_ffmpeg;

#[test]
fn test_create_thumbnail_youtube() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let options = ThumbnailOptions {
        title: Some("Título de teste grande".to_string()),
        ..ThumbnailOptions::default()
    };
    let result = create_thumbnail(&rt.runtime, &sample, &options).unwrap();
    assert_eq!(result.output, "sample_thumb_youtube.jpg");
    assert_eq!((result.width, result.height), (1280, 720));
    assert_eq!(result.time, Some(1.5));
    assert!(result.size_bytes > 0);
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert_eq!(probed.info.width, Some(1280));
}

#[test]
fn test_create_thumbnail_vertical_custom() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let options = ThumbnailOptions {
        time: Some(0.2),
        title: Some("Sem darken".to_string()),
        thumbnail_format: ThumbnailFormat::Vertical,
        position: TextPosition::Bottom,
        text_color: "#FFD700".to_string(),
        darken: false,
        output: Some("capas/capa.png".to_string()),
        ..ThumbnailOptions::default()
    };
    let result = create_thumbnail(&rt.runtime, &sample, &options).unwrap();
    assert_eq!(result.output, "capas/capa.png");
    assert_eq!((result.width, result.height), (1080, 1920));
}

#[test]
fn test_create_thumbnail_from_image() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let base = create_thumbnail(
        &rt.runtime,
        &sample,
        &ThumbnailOptions {
            thumbnail_format: ThumbnailFormat::Original,
            ..ThumbnailOptions::default()
        },
    )
    .unwrap();
    assert_eq!((base.width, base.height), (320, 240));
    let result = create_thumbnail(
        &rt.runtime,
        &base.output,
        &ThumbnailOptions {
            title: Some("Capa".to_string()),
            ..ThumbnailOptions::default()
        },
    )
    .unwrap();
    assert_eq!(result.time, None);
}

#[test]
fn test_create_thumbnail_invalid() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let cases = [
        ThumbnailOptions {
            time: Some(99.0),
            ..ThumbnailOptions::default()
        },
        ThumbnailOptions {
            title: Some("x".repeat(81)),
            ..ThumbnailOptions::default()
        },
        ThumbnailOptions {
            text_color: "red".to_string(),
            ..ThumbnailOptions::default()
        },
        ThumbnailOptions {
            output: Some("capa.gif".to_string()),
            ..ThumbnailOptions::default()
        },
    ];
    for options in &cases {
        let error = create_thumbnail(&rt.runtime, &sample, options).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
}

#[test]
fn test_create_thumbnail_missing() {
    let rt = runtime();
    let error =
        create_thumbnail(&rt.runtime, "ghost.mp4", &ThumbnailOptions::default()).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}
