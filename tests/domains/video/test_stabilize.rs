use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::ffmpeg::FFmpeg;
use mcp_tools::domains::video::probe::probe_video;
use mcp_tools::domains::video::stabilize::{stabilize_video, CropMode};

use crate::conftest::{runtime, sample_video};
use crate::helpers::{unwrap, unwrap_job};
use crate::skip_without_ffmpeg;

/// `true` quando este ffmpeg foi compilado com o vid.stab. Builds em LGPL não
/// trazem o filtro, e aí só os testes de validação fazem sentido.
fn vidstab_available() -> bool {
    FFmpeg::default()
        .filters()
        .is_ok_and(|filters| filters.contains("vidstabdetect"))
}

/// Sai do teste com aviso quando o ffmpeg não tem o vid.stab.
macro_rules! skip_without_vidstab {
    () => {
        if !vidstab_available() {
            eprintln!("pulado: ffmpeg sem vid.stab");
            return;
        }
    };
}

#[test]
fn test_stabilize_video() {
    skip_without_ffmpeg!();
    skip_without_vidstab!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(stabilize_video(
        &rt.runtime,
        &sample,
        12,
        6,
        None,
        CropMode::Black,
        false,
    ));
    assert_eq!(result.output, "sample_stabilized.mp4");
    assert_eq!(result.smoothing, 12);
    assert_eq!(result.shakiness, 6);
    assert_eq!(result.crop, CropMode::Black);
    assert_eq!(result.zoom, None);
    // A estabilização não muda resolução nem duração.
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert_eq!(probed.info.width, Some(320));
    assert_eq!(probed.info.height, Some(240));
    assert!((probed.info.duration - 3.0).abs() < 0.3);
}

#[test]
fn test_stabilize_video_zoom_fixo() {
    skip_without_ffmpeg!();
    skip_without_vidstab!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(stabilize_video(
        &rt.runtime,
        &sample,
        20,
        8,
        Some(5.0),
        CropMode::Keep,
        false,
    ));
    assert_eq!(result.zoom, Some(5.0));
    assert_eq!(result.crop, CropMode::Keep);
    assert_eq!(result.smoothing, 20);
}

#[test]
fn test_stabilize_video_background() {
    skip_without_ffmpeg!();
    skip_without_vidstab!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let job = unwrap_job(stabilize_video(
        &rt.runtime,
        &sample,
        12,
        6,
        None,
        CropMode::Black,
        true,
    ));
    assert_eq!(job.tool, "stabilize_video");
}

#[test]
fn test_stabilize_video_invalid() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    // smoothing fora da faixa, shakiness fora da faixa e zoom fora da faixa.
    let cases: [(i64, i64, Option<f64>); 4] = [
        (0, 6, None),
        (200, 6, None),
        (12, 0, None),
        (12, 6, Some(90.0)),
    ];
    for (smoothing, shakiness, zoom) in cases {
        let error = stabilize_video(
            &rt.runtime,
            &sample,
            smoothing,
            shakiness,
            zoom,
            CropMode::Black,
            false,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.hint.is_some());
    }
}

#[test]
fn test_stabilize_video_arquivo_inexistente() {
    let rt = runtime();
    let error = stabilize_video(
        &rt.runtime,
        "nao_existe.mp4",
        12,
        6,
        None,
        CropMode::Black,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}
