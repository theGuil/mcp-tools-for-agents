use mcp_tools::core::vad::{detect_speech, is_available, pcm16_to_f32, timestamps, VadOptions};

use crate::conftest::speech_samples;

#[test]
fn test_timestamps_from_probabilities() {
    // 40 janelas: fala nas 10 primeiras e nas 10 últimas, pausa no meio.
    let mut probabilities = vec![0.9f32; 10];
    probabilities.extend(vec![0.05f32; 20]);
    probabilities.extend(vec![0.9f32; 10]);
    let options = VadOptions {
        speech_pad: 0.0,
        ..VadOptions::default()
    };
    let segments = timestamps(&probabilities, 40 * 512, &options);
    assert_eq!(segments.len(), 2);
    assert!((segments[0].0 - 0.0).abs() < 0.001);
    assert!((segments[0].1 - 0.32).abs() < 0.001);
    assert!((segments[1].0 - 0.96).abs() < 0.001);
    assert!((segments[1].1 - 1.28).abs() < 0.001);
}

#[test]
fn test_timestamps_min_silence_merges_short_pause() {
    let mut probabilities = vec![0.9f32; 10];
    probabilities.extend(vec![0.05f32; 2]);
    probabilities.extend(vec![0.9f32; 10]);
    let options = VadOptions {
        min_silence: 0.5,
        speech_pad: 0.0,
        ..VadOptions::default()
    };
    let segments = timestamps(&probabilities, 22 * 512, &options);
    assert_eq!(segments.len(), 1);
}

#[test]
fn test_pcm16_to_f32() {
    let bytes = [0x00, 0x00, 0xFF, 0x7F, 0x00, 0x80];
    let samples = pcm16_to_f32(&bytes);
    assert_eq!(samples.len(), 3);
    assert_eq!(samples[0], 0.0);
    assert!((samples[1] - 1.0).abs() < 0.001);
    assert_eq!(samples[2], -1.0);
}

#[test]
fn test_detect_speech_on_real_voice() {
    let samples = speech_samples();
    if !is_available() {
        let error = detect_speech(&samples, &VadOptions::default()).unwrap_err();
        assert_eq!(error.code, mcp_tools::core::errors::ErrorCode::Unavailable);
        return;
    }
    let segments = detect_speech(&samples, &VadOptions::default()).unwrap();
    eprintln!("segmentos: {segments:?}");
    // Fala em 0-2.5s e 4-6.5s, pausa de 1.5s no meio.
    assert!(!segments.is_empty(), "nenhuma fala detectada");
    let first = segments.first().unwrap();
    let last = segments.last().unwrap();
    assert!(first.0 < 0.5, "fala deveria começar no início: {first:?}");
    assert!(last.1 > 6.0, "fala deveria ir até o fim: {last:?}");
    assert!(
        segments.iter().all(|(a, b)| !(a < &2.7 && b > &3.8)),
        "a pausa de 2.5-4s deveria ser cortada: {segments:?}"
    );
}
