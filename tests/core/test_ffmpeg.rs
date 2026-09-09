use mcp_tools::core::binaries::Binaries;
use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::ffmpeg::{parse_filters, parse_probe, FFmpeg};

use crate::conftest::{sample_video, workspace, SAMPLE_DURATION};
use crate::skip_without_ffmpeg;

#[test]
fn test_unavailable_binary() {
    let ff = FFmpeg::new(
        "nao-existe-ffmpeg",
        "nao-existe-ffprobe",
        600.0,
        Binaries::default(),
    );
    assert!(!ff.is_available());
    let error = ff.require().unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
}

#[test]
fn test_parse_probe_minimal() {
    let ws = workspace();
    let fake = ws.workspace.root.join("x.mp4");
    std::fs::write(&fake, b"abc").unwrap();
    let raw = r#"{"format": {"duration": "1.5", "format_name": "mp4"}, "streams": []}"#;
    let info = parse_probe(raw, &fake).unwrap();
    assert_eq!(info.duration, 1.5);
    assert!(!info.has_video);
    assert_eq!(info.size_bytes, 3);
}

#[test]
fn test_parse_probe_invalid() {
    let ws = workspace();
    assert!(parse_probe("not json", &ws.workspace.root.join("x")).is_err());
}

#[test]
fn test_probe_real_file() {
    skip_without_ffmpeg!();
    let ws = workspace();
    let sample = sample_video(&ws.workspace);
    let info = FFmpeg::default()
        .probe(&ws.workspace.root.join(sample))
        .unwrap();
    assert!((info.duration - SAMPLE_DURATION).abs() < 0.2);
    assert_eq!((info.width, info.height), (Some(320), Some(240)));
    assert_eq!(info.fps, Some(25.0));
    assert!(info.has_audio);
    assert_eq!(info.video_codec.as_deref(), Some("h264"));
}

#[test]
fn test_parse_filters() {
    let saida = "Filters:\n  T.. = Timeline support\n  ------\n \
                 .. abench            A->A       Benchmark part of a filtergraph.\n \
                 .. vidstabdetect     V->V       Extract relative transformations.\n \
                 .C vidstabtransform  V->V       Transform the frames.\n \
                 ... anullsrc         |->A       Null audio source.\n";
    let filtros = parse_filters(saida);
    assert!(filtros.contains("vidstabdetect"));
    assert!(filtros.contains("vidstabtransform"));
    assert!(filtros.contains("abench"));
    assert!(filtros.contains("anullsrc"));
    // Linhas de cabeçalho não viram filtro.
    assert!(!filtros.contains("="));
    assert!(!filtros.contains("Timeline"));
}
