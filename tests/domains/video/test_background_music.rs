use std::process::Command;
use std::time::Duration;

use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::jobs::JobStatus;
use mcp_tools::core::paths::Workspace;
use mcp_tools::core::speech::SpeechMethod;
use mcp_tools::domains::video::background_music::{
    add_background_music, duck_commands, gate_level, DuckingOptions,
};
use mcp_tools::domains::video::probe::probe_video;

use crate::conftest::{runtime, sample_video};
use crate::helpers::{unwrap, unwrap_job};
use crate::skip_without_ffmpeg;

fn ducking(method: SpeechMethod) -> DuckingOptions {
    DuckingOptions {
        enabled: true,
        duck_db: 12.0,
        method,
    }
}

fn no_ducking() -> DuckingOptions {
    DuckingOptions {
        enabled: false,
        ..DuckingOptions::default()
    }
}

/// Gera 1s de música sintética (`trilha.mp3`) no workspace.
fn music(workspace: &Workspace) -> String {
    let target = workspace.root.join("trilha.mp3");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=220:duration=1",
            "-c:a",
            "libmp3lame",
        ])
        .arg(&target)
        .status()
        .expect("ffmpeg");
    assert!(status.success());
    "trilha.mp3".to_string()
}

#[test]
fn test_add_background_music_with_ducking() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let track = music(&rt.runtime.workspace);
    let result = unwrap(add_background_music(
        &rt.runtime,
        &sample,
        &track,
        0.2,
        ducking(SpeechMethod::Db),
        1.0,
        2.0,
        0.0,
        true,
        false,
    ));
    assert_eq!(result.output, "sample_music.mp4");
    assert!(result.ducking);
    assert_eq!(result.ducking_method.as_deref(), Some("db"));
    assert_eq!(result.speech_segments, 1);
    assert!(result.looped);
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert!(probed.info.has_audio);
    assert!((probed.info.duration - 3.0).abs() < 0.3);
}

#[test]
fn test_add_background_music_no_ducking_no_loop() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let track = music(&rt.runtime.workspace);
    let result = unwrap(add_background_music(
        &rt.runtime,
        &sample,
        &track,
        0.2,
        no_ducking(),
        1.0,
        0.0,
        1.0,
        false,
        false,
    ));
    assert!(!result.ducking);
    assert!(!result.looped);
}

#[test]
fn test_add_background_music_invalid() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let track = music(&rt.runtime.workspace);
    let error = add_background_music(
        &rt.runtime,
        &sample,
        &track,
        0.0,
        ducking(SpeechMethod::Db),
        1.0,
        2.0,
        0.0,
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let error = add_background_music(
        &rt.runtime,
        &sample,
        &track,
        0.2,
        ducking(SpeechMethod::Db),
        -1.0,
        2.0,
        0.0,
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let error = add_background_music(
        &rt.runtime,
        &sample,
        &track,
        0.2,
        ducking(SpeechMethod::Db),
        1.0,
        2.0,
        10.0,
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_add_background_music_missing() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let error = add_background_music(
        &rt.runtime,
        &sample,
        "ghost.mp3",
        0.2,
        ducking(SpeechMethod::Db),
        1.0,
        2.0,
        0.0,
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
    let error = add_background_music(
        &rt.runtime,
        "ghost.mp4",
        &sample,
        0.2,
        ducking(SpeechMethod::Db),
        1.0,
        2.0,
        0.0,
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn test_add_background_music_background() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let track = music(&rt.runtime.workspace);
    let submitted = unwrap_job(add_background_music(
        &rt.runtime,
        &sample,
        &track,
        0.2,
        ducking(SpeechMethod::Db),
        1.0,
        2.0,
        0.0,
        true,
        true,
    ));
    let job = rt
        .runtime
        .jobs
        .wait(&submitted.job_id, Some(Duration::from_secs(60)))
        .unwrap();
    assert_eq!(job.status, JobStatus::Done);
}

#[test]
fn test_gate_level_and_commands() {
    // 12 dB de redução pede um sinal de controle perto do nível do sine (-18 dBFS).
    let level = gate_level(12.0);
    assert!((0.9..1.2).contains(&level), "{level}");
    assert!(gate_level(1.0) < gate_level(20.0));
    let commands = duck_commands(&[(0.5, 2.0), (2.9, 3.2)], 3.0);
    assert_eq!(
        commands,
        "0.350 volume@gate volume 1;\n2.000 volume@gate volume 0;\n\
         2.750 volume@gate volume 1;\n3.000 volume@gate volume 0;\n"
    );
}

#[test]
fn test_add_background_music_invalid_duck_db() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let track = music(&rt.runtime.workspace);
    let error = add_background_music(
        &rt.runtime,
        &sample,
        &track,
        0.2,
        DuckingOptions {
            duck_db: 0.0,
            ..DuckingOptions::default()
        },
        1.0,
        2.0,
        0.0,
        true,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_add_background_music_auto_without_voice_keeps_music_flat() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let track = music(&rt.runtime.workspace);
    let result = unwrap(add_background_music(
        &rt.runtime,
        &sample,
        &track,
        0.2,
        ducking(SpeechMethod::Auto),
        1.0,
        2.0,
        0.0,
        true,
        false,
    ));
    // Tom puro: sem voz para o VAD, a detecção cai para dB (que acha "fala" o tempo todo).
    assert!(result.ducking);
    assert_eq!(result.ducking_method.as_deref(), Some("db"));
}
