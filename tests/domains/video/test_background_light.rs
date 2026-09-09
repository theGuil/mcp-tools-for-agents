use mcp_tools::core::errors::ErrorCode;
use mcp_tools::domains::video::background_light::{
    apply_studio_background_light, falloff_expression, glow_expression, parse_hex, LightBlend,
    LightColor, LightShape,
};
use mcp_tools::domains::video::probe::probe_video;

use crate::conftest::{runtime, sample_video};
use crate::helpers::unwrap;
use crate::skip_without_ffmpeg;

#[test]
fn test_falloff_expression() {
    // A vírgula precisa sair escapada: o valor entra dentro de um filtro do ffmpeg.
    let radial = falloff_expression(LightShape::Radial, 0.8);
    assert_eq!(
        radial,
        "exp(-pow(hypot((X-W/2)/(W/2)\\,(Y-H/2)/(H/2))/0.8000\\,2))"
    );
    assert!(!radial.contains(", "));
    assert_eq!(
        falloff_expression(LightShape::Top, 1.0),
        "exp(-pow(Y/H/1.0000\\,2))"
    );
    assert_eq!(
        falloff_expression(LightShape::Bottom, 1.0),
        "exp(-pow((H-Y)/H/1.0000\\,2))"
    );
    assert_eq!(
        falloff_expression(LightShape::Sides, 0.5),
        "exp(-pow(min(X\\,W-X)/(W/2)/0.5000\\,2))"
    );
}

#[test]
fn test_glow_expression() {
    let glow = glow_expression(LightColor::CyanBlue.rgb(), LightShape::Radial, 0.8);
    assert!(glow.starts_with("geq=r='20*"));
    assert!(glow.contains(":g='140*"));
    assert!(glow.contains(":b='255*"));
}

#[test]
fn test_parse_hex() {
    assert_eq!(parse_hex("#1E90FF").unwrap(), (30, 144, 255));
    assert_eq!(parse_hex("ff0000").unwrap(), (255, 0, 0));
    for invalid in ["#12345", "#GGGGGG", "", "azul", "#1234567"] {
        let error = parse_hex(invalid).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
}

#[test]
fn test_apply_studio_background_light() {
    skip_without_ffmpeg!();
    for shape in [
        LightShape::Radial,
        LightShape::Top,
        LightShape::Bottom,
        LightShape::Sides,
    ] {
        let rt = runtime();
        let sample = sample_video(&rt.runtime.workspace);
        let result = unwrap(apply_studio_background_light(
            &rt.runtime,
            &sample,
            LightColor::CyanBlue,
            None,
            shape,
            LightBlend::Screen,
            0.6,
            0.8,
            false,
        ));
        assert_eq!(result.output, "sample_light_cyan_blue.mp4");
        assert_eq!(result.hex, "#148CFF");
        assert_eq!(result.shape, shape);
        assert_eq!(result.width, 320);
        assert_eq!(result.height, 240);
        // Resolução, duração e áudio seguem iguais aos do original.
        let probed = probe_video(&rt.runtime, &result.output).unwrap();
        assert_eq!(probed.info.width, Some(320));
        assert_eq!(probed.info.height, Some(240));
        assert!(probed.info.has_audio);
        assert!((probed.info.duration - 3.0).abs() < 0.3);
    }
}

#[test]
fn test_apply_studio_background_light_custom_color() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(apply_studio_background_light(
        &rt.runtime,
        &sample,
        LightColor::CyanBlue,
        Some("#1E90FF".to_string()),
        LightShape::Radial,
        LightBlend::Softlight,
        0.9,
        1.4,
        false,
    ));
    // custom_color vence o preset e o arquivo sai com o sufixo próprio.
    assert_eq!(result.output, "sample_light_custom.mp4");
    assert_eq!(result.hex, "#1E90FF");
    assert_eq!(result.blend, LightBlend::Softlight);
}

#[test]
fn test_apply_studio_background_light_invalid() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    // (intensity, spread, custom_color)
    let cases: [(f64, f64, Option<&str>); 5] = [
        (0.0, 0.8, None),
        (1.5, 0.8, None),
        (0.6, 0.1, None),
        (0.6, 3.0, None),
        (0.6, 0.8, Some("roxo")),
    ];
    for (intensity, spread, custom) in cases {
        let error = apply_studio_background_light(
            &rt.runtime,
            &sample,
            LightColor::NeonPurple,
            custom.map(str::to_string),
            LightShape::Radial,
            LightBlend::Screen,
            intensity,
            spread,
            false,
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.hint.is_some());
    }
}

#[test]
fn test_apply_studio_background_light_missing() {
    let rt = runtime();
    let error = apply_studio_background_light(
        &rt.runtime,
        "ghost.mp4",
        LightColor::CyanBlue,
        None,
        LightShape::Radial,
        LightBlend::Screen,
        0.6,
        0.8,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}
