//! Testes de `domains/media/search_sfx.rs`, com o transporte do Freesound substituído.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mcp_tools::config::Settings;
use mcp_tools::core::downloader::Downloader;
use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::ffmpeg::FFmpeg;
use mcp_tools::core::freesound::Freesound;
use mcp_tools::domains::media::search_sfx::search_sound_effects;
use mcp_tools::domains::Runtime;
use serde_json::json;

use crate::conftest::{jobs, workspace, TestRuntime};

/// Runtime cujo Freesound responde a busca com o "Drama Boom" e registra as URLs pedidas.
fn runtime_with_freesound() -> (TestRuntime, Arc<Mutex<Vec<String>>>) {
    let urls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&urls);
    let freesound = Freesound::with_fetcher(
        "abc",
        Arc::new(move |url: &str| {
            seen.lock().unwrap().push(url.to_string());
            let body = json!({"results": [{
                "id": 785925,
                "name": "Drama Boom",
                "duration": 4.4,
                "license": "http://creativecommons.org/publicdomain/zero/1.0/",
                "username": "modusmogulus",
                "tags": ["boom"],
                "previews": {"preview-hq-mp3": "https://cdn.freesound.org/previews/785/785925-hq.mp3"},
            }]});
            Ok(serde_json::to_vec(&body).unwrap())
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
    (
        TestRuntime {
            dir: ws.dir,
            runtime: Arc::new(runtime),
        },
        urls,
    )
}

#[test]
fn test_search_success() {
    let (rt, urls) = runtime_with_freesound();
    let result = search_sound_effects(&rt.runtime, " vine boom ", 3, 6.0).unwrap();
    assert_eq!(result.query, "vine boom");
    assert_eq!(result.count, 1);
    assert_eq!(result.sounds[0].sound_id, 785925);
    // A query, o limite e a duração máxima chegam ao Freesound como foram passados.
    let urls = urls.lock().unwrap();
    assert_eq!(urls.len(), 1);
    assert!(urls[0].contains("query=vine+boom"));
    assert!(urls[0].contains("page_size=3"));
    assert!(urls[0].contains("filter=duration%3A%5B0+TO+6%5D"));
}

#[test]
fn test_search_empty_query() {
    let (rt, _) = runtime_with_freesound();
    let err = search_sound_effects(&rt.runtime, "  ", 5, 10.0).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_search_invalid_limit() {
    let (rt, _) = runtime_with_freesound();
    let err = search_sound_effects(&rt.runtime, "boom", 0, 10.0).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
}
