use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mcp_tools::config::Settings;
use mcp_tools::core::downloader::Downloader;
use mcp_tools::core::errors::{ErrorCode, ToolResult};
use mcp_tools::core::ffmpeg::FFmpeg;
use mcp_tools::core::freesound::Freesound;
use mcp_tools::core::jobs::{JobStatus, MaybeJob};
use mcp_tools::core::paths::Workspace;
use mcp_tools::domains::video::probe::probe_video;
use mcp_tools::domains::video::sound_effects::{
    add_sound_effects, AddSoundEffectsResult, SoundEffect,
};
use mcp_tools::domains::Runtime;

use crate::conftest::{jobs, runtime, sample_video, workspace, TestRuntime};
use crate::helpers::{unwrap, unwrap_job};
use crate::skip_without_ffmpeg;

const BOOM_ID: i64 = 785_925;
const DING_ID: i64 = 111;
const BOOM_PREVIEW: &str = "https://cdn.freesound.org/previews/785/785925-hq.mp3";
const DING_PREVIEW: &str = "https://cdn.freesound.org/previews/111/111-hq.mp3";

fn sine(target: &Path, frequency: u32, duration: f64) {
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency={frequency}:duration={duration}"),
        ])
        .arg(target)
        .status()
        .expect("ffmpeg");
    assert!(status.success());
}

fn boom_file(workspace: &Workspace) -> String {
    sine(&workspace.root.join("boom.mp3"), 110, 0.4);
    "boom.mp3".to_string()
}

fn sound_json(
    id: i64,
    name: &str,
    duration: f64,
    license: &str,
    user: &str,
    preview: &str,
) -> String {
    serde_json::json!({
        "id": id,
        "name": name,
        "duration": duration,
        "license": license,
        "username": user,
        "tags": ["x"],
        "previews": {"preview-hq-mp3": preview},
    })
    .to_string()
}

fn boom_json() -> String {
    sound_json(
        BOOM_ID,
        "Drama Boom",
        0.4,
        "https://creativecommons.org/publicdomain/zero/1.0/",
        "modusmogulus",
        BOOM_PREVIEW,
    )
}

fn ding_json() -> String {
    sound_json(
        DING_ID,
        "Ding",
        0.3,
        "https://creativecommons.org/licenses/by/4.0/",
        "alguem",
        DING_PREVIEW,
    )
}

/// Bytes de um MP3 sintético, o que o "download" do preview devolve.
fn fake_mp3(duration: f64) -> Vec<u8> {
    let dir = tempfile::tempdir().expect("tmp");
    let target = dir.path().join("preview.mp3");
    sine(&target, 880, duration);
    std::fs::read(&target).expect("mp3")
}

/// Runtime com um Freesound falso que registra as chamadas e devolve JSON e
/// MP3 sintéticos no lugar da rede. Espelha o `fake_freesound` do Python.
fn fake_runtime() -> (TestRuntime, Arc<Mutex<Vec<String>>>) {
    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&calls);
    let fetcher = Arc::new(move |url: &str| -> ToolResult<Vec<u8>> {
        if url.contains("/search/text/") {
            let query = url
                .split('?')
                .nth(1)
                .unwrap_or("")
                .split('&')
                .find_map(|pair| pair.strip_prefix("query="))
                .unwrap_or("")
                .replace('+', " ");
            log.lock().unwrap().push(format!("search:{query}"));
            let results = if query.contains("boom") {
                format!("[{}]", boom_json())
            } else {
                "[]".to_string()
            };
            return Ok(format!("{{\"results\": {results}}}").into_bytes());
        }
        if let Some(rest) = url.strip_prefix("https://freesound.org/apiv2/sounds/") {
            let id = rest.split('/').next().unwrap_or("");
            log.lock().unwrap().push(format!("sound:{id}"));
            return Ok(ding_json().into_bytes());
        }
        if url == BOOM_PREVIEW {
            log.lock().unwrap().push(format!("download:{BOOM_ID}"));
            return Ok(fake_mp3(0.4));
        }
        if url == DING_PREVIEW {
            log.lock().unwrap().push(format!("download:{DING_ID}"));
            return Ok(fake_mp3(0.3));
        }
        panic!("URL inesperada: {url}");
    });
    let ws = workspace();
    let mut env = HashMap::new();
    env.insert(
        "WORKSPACE_DIR".to_string(),
        ws.workspace.root.to_string_lossy().into_owned(),
    );
    let settings = Settings::from_map(&env).expect("settings");
    let runtime = Runtime {
        settings,
        workspace: ws.workspace,
        ffmpeg: FFmpeg::default(),
        jobs: jobs(),
        downloader: Downloader::default(),
        freesound: Freesound::with_fetcher("chave-de-teste", fetcher),
    };
    (
        TestRuntime {
            dir: ws.dir,
            runtime: Arc::new(runtime),
        },
        calls,
    )
}

fn calls_of(calls: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    calls.lock().unwrap().clone()
}

fn run(
    rt: &TestRuntime,
    path: &str,
    effects: &[SoundEffect],
    original_volume: f64,
    background: bool,
) -> ToolResult<MaybeJob<AddSoundEffectsResult>> {
    add_sound_effects(
        &rt.runtime,
        path,
        effects,
        original_volume,
        "sfx",
        background,
    )
}

#[test]
fn test_add_from_workspace_file() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let boom = boom_file(&rt.runtime.workspace);
    let effects = [
        SoundEffect::from_audio(&boom, 0.5),
        SoundEffect::from_audio(&boom, 2.0),
    ];
    let result = unwrap(run(&rt, &sample, &effects, 1.0, false));
    assert_eq!(result.output, "sample_sfx.mp4");
    assert_eq!(result.original_volume, 1.0);
    let starts: Vec<f64> = result.effects.iter().map(|e| e.start).collect();
    assert_eq!(starts, [0.5, 2.0]);
    assert_eq!(result.effects[0].sound_id, None);
    assert!(result.effects[0].duration > 0.3);
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert!(probed.info.has_audio);
    assert!((probed.info.duration - 3.0).abs() < 0.3);
}

#[test]
fn test_add_from_query_and_sound_id() {
    skip_without_ffmpeg!();
    let (rt, calls) = fake_runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let effects = [
        SoundEffect::from_query("vine boom", 1.0).with_volume(1.5),
        SoundEffect::from_sound_id(DING_ID, 2.5),
    ];
    let result = unwrap(run(&rt, &sample, &effects, 0.3, false));
    assert_eq!(
        calls_of(&calls),
        [
            "search:vine boom",
            "download:785925",
            "sound:111",
            "download:111"
        ]
    );
    let first = &result.effects[0];
    let second = &result.effects[1];
    assert_eq!(first.audio, "sfx/Drama_Boom_785925.mp3");
    assert_eq!(first.sound_id, Some(BOOM_ID));
    assert_eq!(first.license.as_deref(), Some("CC0"));
    assert_eq!(first.volume, 1.5);
    assert_eq!(second.audio, "sfx/Ding_111.mp3");
    assert_eq!(second.author.as_deref(), Some("alguem"));
    assert!(rt.root().join("sfx").join("Ding_111.mp3").is_file());
}

#[test]
fn test_add_without_original_audio() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let boom = boom_file(&rt.runtime.workspace);
    let effects = [SoundEffect::from_audio(&boom, 1.0)];
    let result = unwrap(run(&rt, &sample, &effects, 0.0, false));
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert!(probed.info.has_audio);
    assert!((probed.info.duration - 3.0).abs() < 0.3);
}

#[test]
fn test_add_background() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let boom = boom_file(&rt.runtime.workspace);
    let effects = [SoundEffect::from_audio(&boom, 0.2)];
    let submitted = unwrap_job(run(&rt, &sample, &effects, 1.0, true));
    let job = rt
        .runtime
        .jobs
        .wait(&submitted.job_id, Some(Duration::from_secs(60)))
        .unwrap();
    assert_eq!(job.status, JobStatus::Done);
    let result = job.result.expect("resultado");
    assert_eq!(result["output"], "sample_sfx.mp4");
}

#[test]
fn test_add_query_without_results() {
    skip_without_ffmpeg!();
    let (rt, calls) = fake_runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let effects = [SoundEffect::from_query("xyz", 1.0)];
    let error = run(&rt, &sample, &effects, 1.0, false).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(calls_of(&calls), ["search:xyz"]);
}

#[test]
fn test_add_invalid() {
    skip_without_ffmpeg!();
    let (rt, calls) = fake_runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let boom = boom_file(&rt.runtime.workspace);
    let code = |effects: &[SoundEffect], original_volume: f64| {
        run(&rt, &sample, effects, original_volume, false)
            .unwrap_err()
            .code
    };
    assert_eq!(code(&[], 1.0), ErrorCode::InvalidArgument);
    assert_eq!(
        code(&[SoundEffect::from_audio(&boom, -1.0)], 1.0),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        code(&[SoundEffect::from_audio(&boom, 0.0).with_volume(0.0)], 1.0),
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        code(&[SoundEffect::from_audio(&boom, 0.0).with_volume(5.0)], 1.0),
        ErrorCode::InvalidArgument
    );
    let bare = SoundEffect {
        start: 0.0,
        audio: None,
        query: None,
        sound_id: None,
        volume: None,
    };
    assert_eq!(code(&[bare], 1.0), ErrorCode::InvalidArgument);
    let both = SoundEffect {
        query: Some("boom".to_string()),
        ..SoundEffect::from_audio(&boom, 0.0)
    };
    assert_eq!(code(&[both], 1.0), ErrorCode::InvalidArgument);
    assert_eq!(
        code(&[SoundEffect::from_audio(&boom, 0.0)], 2.0),
        ErrorCode::InvalidArgument
    );
    // start além do vídeo é barrado antes de qualquer busca na internet.
    assert_eq!(
        code(&[SoundEffect::from_query("vine boom", 50.0)], 1.0),
        ErrorCode::InvalidArgument
    );
    assert!(calls_of(&calls).is_empty());
}

#[test]
fn test_add_missing_files() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let boom = boom_file(&rt.runtime.workspace);
    let error = run(
        &rt,
        "ghost.mp4",
        &[SoundEffect::from_audio(&boom, 0.0)],
        1.0,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
    let error = run(
        &rt,
        &sample,
        &[SoundEffect::from_audio("ghost.mp3", 0.0)],
        1.0,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}
