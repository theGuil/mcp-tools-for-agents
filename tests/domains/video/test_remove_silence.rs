use std::process::Command;
use std::time::Duration;

use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::jobs::JobStatus;
use mcp_tools::core::paths::Workspace;
use mcp_tools::core::speech::SpeechMethod;
use mcp_tools::domains::video::probe::probe_video;
use mcp_tools::domains::video::remove_silence::{remove_silence, RemoveSilenceOptions};

use crate::conftest::{runtime, sample_video};
use crate::helpers::{unwrap, unwrap_job};
use crate::skip_without_ffmpeg;

const SPEECH_SECONDS: f64 = 2.0;
const SILENCE_SECONDS: f64 = 1.0;
const TOTAL: f64 = SPEECH_SECONDS + SILENCE_SECONDS;

const THRESHOLD_DB: f64 = -30.0;
const MIN_SILENCE: f64 = 0.5;
const MARGIN: f64 = 0.2;

/// Opções com limiar de dB: os vídeos de teste usam tom puro, não voz.
fn db(threshold_db: f64, min_silence: f64, margin: f64) -> RemoveSilenceOptions {
    RemoveSilenceOptions {
        method: SpeechMethod::Db,
        threshold_db,
        min_silence,
        margin,
    }
}

/// Vídeo de 3s: tom em 0-1s, silêncio em 1-2s, tom em 2-3s.
fn gapped_video(workspace: &Workspace) -> String {
    let target = workspace.root.join("aula.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=size=320x240:rate=25:duration={TOTAL}"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={TOTAL}"),
            "-af",
            "volume='if(between(t,1,2),0,1)':eval=frame",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
        ])
        .arg(&target)
        .status()
        .expect("ffmpeg");
    assert!(status.success());
    "aula.mp4".to_string()
}

#[test]
fn test_remove_silence_cuts_gap() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let video = gapped_video(&rt.runtime.workspace);
    let result = unwrap(remove_silence(
        &rt.runtime,
        &video,
        db(THRESHOLD_DB, MIN_SILENCE, 0.0),
        false,
    ));
    assert_eq!(result.output, "aula_nosilence.mp4");
    assert_eq!(result.method, "db");
    assert_eq!(result.silences_removed, 1);
    assert_eq!(result.segments.len(), 2);
    assert!((result.removed_duration - SILENCE_SECONDS).abs() < 0.2);
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert!((probed.info.duration - SPEECH_SECONDS).abs() < 0.3);
}

#[test]
fn test_remove_silence_margin_keeps_more() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let video = gapped_video(&rt.runtime.workspace);
    let result = unwrap(remove_silence(
        &rt.runtime,
        &video,
        db(THRESHOLD_DB, MIN_SILENCE, 0.2),
        false,
    ));
    assert!((result.duration - (SPEECH_SECONDS + 0.4)).abs() < 0.2);
}

#[test]
fn test_remove_silence_without_gaps() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(remove_silence(
        &rt.runtime,
        &sample,
        db(THRESHOLD_DB, MIN_SILENCE, MARGIN),
        false,
    ));
    assert_eq!(result.silences_removed, 0);
    assert_eq!(result.segments.len(), 1);
}

#[test]
fn test_remove_silence_invalid_threshold() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let error =
        remove_silence(&rt.runtime, &sample, db(5.0, MIN_SILENCE, MARGIN), false).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_remove_silence_invalid_min_silence() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let error =
        remove_silence(&rt.runtime, &sample, db(THRESHOLD_DB, 0.0, MARGIN), false).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_remove_silence_missing_file() {
    let rt = runtime();
    let error = remove_silence(
        &rt.runtime,
        "ghost.mp4",
        RemoveSilenceOptions::default(),
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn test_remove_silence_background() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let video = gapped_video(&rt.runtime.workspace);
    let submitted = unwrap_job(remove_silence(
        &rt.runtime,
        &video,
        db(THRESHOLD_DB, MIN_SILENCE, MARGIN),
        true,
    ));
    let job = rt
        .runtime
        .jobs
        .wait(&submitted.job_id, Some(Duration::from_secs(60)))
        .unwrap();
    assert_eq!(job.status, JobStatus::Done);
    let result = job.result.expect("resultado");
    assert_eq!(result["output"].as_str().unwrap(), "aula_nosilence.mp4");
}

/// Vídeo com voz real (fixture speech_gap.wav): fala em 0-2.5s e 4-6.5s.
fn voice_video(workspace: &Workspace) -> String {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/speech_gap.wav");
    let target = workspace.root.join("voz.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=6.5",
            "-i",
            fixture,
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(&target)
        .status()
        .expect("ffmpeg");
    assert!(status.success());
    "voz.mp4".to_string()
}

#[test]
fn test_remove_silence_auto_falls_back_to_db_on_pure_tone() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let video = gapped_video(&rt.runtime.workspace);
    let result = unwrap(remove_silence(
        &rt.runtime,
        &video,
        RemoveSilenceOptions {
            margin: 0.0,
            ..RemoveSilenceOptions::default()
        },
        false,
    ));
    // Tom puro não é voz: em "auto" a detecção cai para o limiar de dB e avisa.
    assert_eq!(result.method, "db");
    assert!(result.note.is_some());
    assert_eq!(result.segments.len(), 2);
}

#[test]
fn test_remove_silence_vad_on_real_voice() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let video = voice_video(&rt.runtime.workspace);
    let result = remove_silence(
        &rt.runtime,
        &video,
        RemoveSilenceOptions {
            method: SpeechMethod::Vad,
            min_silence: 0.5,
            margin: 0.1,
            ..RemoveSilenceOptions::default()
        },
        false,
    );
    if !mcp_tools::core::vad::is_available() {
        assert_eq!(result.unwrap_err().code, ErrorCode::Unavailable);
        return;
    }
    let result = unwrap(result);
    assert_eq!(result.method, "vad");
    assert!(result.note.is_none());
    assert!(
        result.removed_duration > 0.8,
        "a pausa de 1.5s deveria ser cortada: {result:?}"
    );
    assert!(result
        .segments
        .iter()
        .all(|seg| !(seg.start < 2.7 && seg.end > 3.8)));
}
