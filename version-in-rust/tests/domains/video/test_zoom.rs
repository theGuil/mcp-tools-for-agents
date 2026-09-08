use mcp_tools::core::errors::ErrorCode;
use mcp_tools::domains::video::probe::probe_video;
use mcp_tools::domains::video::zoom::{zoom_expression, zoom_video, ZoomMode};

use crate::conftest::{runtime, sample_video};
use crate::helpers::unwrap;
use crate::skip_without_ffmpeg;

#[test]
fn test_zoom_expression() {
    assert_eq!(
        zoom_expression(ZoomMode::Punch, 1.5, 1.0, 2.0),
        "if(between(t\\,1.000\\,2.000)\\,1.5000\\,1)"
    );
    assert!(zoom_expression(ZoomMode::In, 1.5, 0.0, 1.0).contains("(1+(1.5000-1)*"));
    assert!(zoom_expression(ZoomMode::Out, 1.5, 0.0, 1.0).contains("(1.5000-(1.5000-1)*"));
}

#[test]
fn test_zoom_video() {
    skip_without_ffmpeg!();
    for mode in [ZoomMode::Punch, ZoomMode::In, ZoomMode::Out] {
        let rt = runtime();
        let sample = sample_video(&rt.runtime.workspace);
        let result = unwrap(zoom_video(
            &rt.runtime,
            &sample,
            0.5,
            2.0,
            mode,
            1.3,
            0.5,
            0.3,
            false,
        ));
        assert_eq!(result.output, format!("sample_zoom_{}.mp4", mode.as_str()));
        let probed = probe_video(&rt.runtime, &result.output).unwrap();
        assert_eq!(probed.info.width, Some(320));
        assert_eq!(probed.info.height, Some(240));
        assert!((probed.info.duration - 3.0).abs() < 0.3);
    }
}

#[test]
fn test_zoom_video_invalid() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let punch = ZoomMode::Punch;
    let cases: [(f64, f64, f64, f64); 4] = [
        (2.0, 1.0, 1.3, 0.5),
        (0.0, 1.0, 10.0, 0.5),
        (0.0, 1.0, 1.3, 2.0),
        (50.0, 51.0, 1.3, 0.5),
    ];
    for (start, end, zoom, focus_x) in cases {
        let error = zoom_video(
            &rt.runtime,
            &sample,
            start,
            end,
            punch,
            zoom,
            focus_x,
            0.5,
            false,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
}

#[test]
fn test_zoom_video_missing() {
    let rt = runtime();
    let error = zoom_video(
        &rt.runtime,
        "ghost.mp4",
        0.0,
        1.0,
        ZoomMode::Punch,
        1.3,
        0.5,
        0.5,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}
