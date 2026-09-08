use std::sync::Arc;

use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::vision::load_face_detector;
use mcp_tools::domains::video::probe::probe_video;
use mcp_tools::domains::video::smart_crop::{
    crop_size, smart_crop, smart_crop_with_tracker, smooth_positions, CropAspect, CropMode,
    FaceTracker,
};

use crate::conftest::{runtime, sample_video};
use crate::helpers::unwrap;
use crate::skip_without_ffmpeg;

#[test]
fn test_crop_size() {
    assert_eq!(crop_size(1920, 1080, CropAspect::Vertical), (608, 1080));
    assert_eq!(crop_size(1920, 1080, CropAspect::Square), (1080, 1080));
    assert_eq!(crop_size(1080, 1920, CropAspect::Landscape), (1080, 608));
    assert_eq!(crop_size(320, 240, CropAspect::Vertical), (134, 240));
}

#[test]
fn test_smooth_positions() {
    let positions = smooth_positions(
        &[Some(100.0), None, Some(100.0), Some(101.0), Some(500.0)],
        50.0,
        10.0,
    );
    assert_eq!(positions[0], 100.0);
    assert_eq!(positions[1], 100.0);
    assert_eq!(positions[3], 100.0); // dentro da zona morta
    assert!(100.0 < positions[4] && positions[4] < 500.0);
}

#[test]
fn test_smart_crop_center() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(smart_crop(
        &rt.runtime,
        &sample,
        CropAspect::Vertical,
        CropMode::Center,
        0.5,
        false,
    ));
    assert_eq!(result.output, "sample_crop_9x16.mp4");
    assert_eq!(result.mode_used, CropMode::Center);
    assert_eq!((result.width, result.height), (134, 240));
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert_eq!(probed.info.width, Some(134));
    assert!(probed.info.has_audio);
}

#[test]
fn test_smart_crop_face_fallback() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let outcome = smart_crop(
        &rt.runtime,
        &sample,
        CropAspect::Square,
        CropMode::Face,
        0.5,
        false,
    );
    // Sem a feature `vision` o detector não existe: equivale ao importorskip("cv2").
    if let Err(error) = &outcome {
        if error.code == ErrorCode::Unavailable {
            eprintln!("pulado: binário compilado sem a feature vision");
            return;
        }
    }
    let result = unwrap(outcome);
    assert_eq!(result.mode_used, CropMode::Center);
    assert!(result.frames_analyzed > 0);
    assert_eq!(result.frames_with_face, 0);
}

#[test]
fn test_smart_crop_face_tracking() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let fake_track: FaceTracker = Arc::new(|_runtime, _source, _width, _interval| {
        Ok((
            vec![
                Some(60.0),
                Some(80.0),
                None,
                Some(200.0),
                Some(260.0),
                Some(300.0),
            ],
            5,
        ))
    });
    let result = unwrap(smart_crop_with_tracker(
        &rt.runtime,
        &sample,
        CropAspect::Vertical,
        CropMode::Face,
        0.5,
        false,
        fake_track,
    ));
    assert_eq!(result.mode_used, CropMode::Face);
    assert_eq!(result.frames_with_face, 5);
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert_eq!(probed.info.width, Some(134));
    assert!((probed.info.duration - 3.0).abs() < 0.3);
}

#[test]
fn test_smart_crop_invalid() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    // "2:3" e "tracking" não existem nos enums: a validação fica no parse do parâmetro.
    assert!(serde_json::from_str::<CropAspect>("\"2:3\"").is_err());
    assert!(serde_json::from_str::<CropMode>("\"tracking\"").is_err());
    let error = smart_crop(
        &rt.runtime,
        &sample,
        CropAspect::Vertical,
        CropMode::Face,
        0.0,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_smart_crop_missing() {
    let rt = runtime();
    let error = smart_crop(
        &rt.runtime,
        "ghost.mp4",
        CropAspect::Vertical,
        CropMode::Face,
        0.5,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn test_face_detector_loads_and_ignores_frames_without_face() {
    let Ok(detector) = load_face_detector() else {
        eprintln!("pulado: binário compilado sem a feature vision");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    // Sem arquivo o detector não tem o que ler: no Python o cv2 devolve None.
    let missing = dir.path().join("nada.jpg");
    assert!(matches!(detector.largest_face(&missing), Ok(None) | Err(_)));
    // Um frame sem rosto de verdade também não devolve nada.
    let blank = dir.path().join("blank.png");
    std::fs::write(&blank, blank_png()).unwrap();
    assert_eq!(detector.largest_face(&blank).unwrap(), None);
}

/// PNG 1x1 cinza, gerado à mão para não depender de crate de imagem no teste.
fn blank_png() -> Vec<u8> {
    fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
        out
    }
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
    fn adler32(bytes: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for byte in bytes {
            a = (a + u32::from(*byte)) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]); // 8 bits, tons de cinza
    png.extend(chunk(b"IHDR", &ihdr));
    let raw = [0u8, 128]; // filtro 0 + um pixel
    let mut idat = vec![0x78, 0x01, 0x01, 2, 0, 0xFD, 0xFF];
    idat.extend_from_slice(&raw);
    idat.extend_from_slice(&adler32(&raw).to_be_bytes());
    png.extend(chunk(b"IDAT", &idat));
    png.extend(chunk(b"IEND", &[]));
    png
}
