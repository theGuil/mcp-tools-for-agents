//! Testes de `domains/media/download_sfx.rs`, com o transporte do Freesound substituído.

use std::collections::HashMap;
use std::sync::Arc;

use mcp_tools::config::Settings;
use mcp_tools::core::downloader::Downloader;
use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::ffmpeg::FFmpeg;
use mcp_tools::core::freesound::{http_error, Freesound};
use mcp_tools::domains::media::download_sfx::download_sound_effect;
use mcp_tools::domains::Runtime;
use serde_json::json;

use crate::conftest::{jobs, workspace, TestRuntime};

const BOOM_ID: i64 = 785925;
const PREVIEW: &str = "https://cdn.freesound.org/previews/785/785925-hq.mp3";

/// Runtime cujo Freesound conhece só o som 785925 ("Drama Boom") e entrega
/// bytes fixos no preview. Qualquer outro id responde 404.
fn runtime_with_freesound() -> TestRuntime {
    let freesound = Freesound::with_fetcher(
        "abc",
        Arc::new(|url: &str| {
            if url == PREVIEW {
                return Ok(b"mp3".to_vec());
            }
            if url.starts_with(&format!("https://freesound.org/apiv2/sounds/{BOOM_ID}/")) {
                let body = json!({
                    "id": BOOM_ID,
                    "name": "Drama Boom",
                    "duration": 4.4,
                    "license": "http://creativecommons.org/publicdomain/zero/1.0/",
                    "username": "modusmogulus",
                    "tags": ["boom"],
                    "previews": {"preview-hq-mp3": PREVIEW},
                });
                return Ok(serde_json::to_vec(&body).unwrap());
            }
            Err(http_error(404))
        }),
    );
    let ws = workspace();
    let mut env = HashMap::new();
    env.insert(
        "WORKSPACE_DIR".to_string(),
        ws.workspace.root.to_string_lossy().into_owned(),
    );
    let runtime = Runtime {
        settings: Settings::from_map(&env).expect("settings"),
        workspace: ws.workspace,
        ffmpeg: FFmpeg::default(),
        jobs: jobs(),
        downloader: Downloader::default(),
        freesound,
    };
    TestRuntime {
        dir: ws.dir,
        runtime: Arc::new(runtime),
    }
}

#[test]
fn test_download_success() {
    let rt = runtime_with_freesound();
    let result = download_sound_effect(&rt.runtime, BOOM_ID, "sfx").unwrap();
    assert_eq!(result.output, "sfx/Drama_Boom_785925.mp3");
    assert_eq!(result.name, "Drama Boom");
    assert_eq!(result.license, "CC0");
    assert_eq!(result.author, "modusmogulus");
    assert!(rt
        .root()
        .join("sfx")
        .join("Drama_Boom_785925.mp3")
        .is_file());
}

#[test]
fn test_download_custom_folder() {
    let rt = runtime_with_freesound();
    let result = download_sound_effect(&rt.runtime, BOOM_ID, "sons/memes").unwrap();
    assert_eq!(result.output, "sons/memes/Drama_Boom_785925.mp3");
}

#[test]
fn test_download_not_found() {
    let rt = runtime_with_freesound();
    let err = download_sound_effect(&rt.runtime, 1, "sfx").unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn test_download_invalid_id() {
    let rt = runtime_with_freesound();
    let err = download_sound_effect(&rt.runtime, -5, "sfx").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_download_folder_outside_workspace() {
    let rt = runtime_with_freesound();
    let err = download_sound_effect(&rt.runtime, BOOM_ID, "../fora").unwrap_err();
    assert_eq!(err.code, ErrorCode::OutsideWorkspace);
}

#[test]
fn test_download_folder_is_file() {
    let rt = runtime_with_freesound();
    std::fs::write(rt.root().join("arquivo.txt"), "x").unwrap();
    let err = download_sound_effect(&rt.runtime, BOOM_ID, "arquivo.txt").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
}
