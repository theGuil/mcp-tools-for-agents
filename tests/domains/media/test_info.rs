//! Testes de `domains/media/info.rs`, com o yt-dlp substituído por um backend de mentira.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mcp_tools::config::Settings;
use mcp_tools::core::downloader::{Downloader, YtdlRequest};
use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::ffmpeg::FFmpeg;
use mcp_tools::core::freesound::Freesound;
use mcp_tools::domains::media::info::get_video_info;
use mcp_tools::domains::Runtime;
use serde_json::{json, Value};

use crate::conftest::{jobs, workspace, TestRuntime};

fn fake_json() -> Value {
    json!({
        "id": "2345",
        "title": "Aula de matemática",
        "extractor_key": "Generic",
        "duration": 90.0,
        "description": "descrição",
        "chapters": [],
    })
}

/// Runtime cujo `Downloader` responde com `reply` e registra as URLs vistas.
fn runtime_with(reply: Result<Value, String>) -> (TestRuntime, Arc<Mutex<Vec<String>>>) {
    let vistas: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&vistas);
    let downloader = Downloader::default()
        .with_backend(Arc::new(move |request: &YtdlRequest| {
            seen.lock().unwrap().push(request.url.clone());
            reply.clone()
        }))
        .with_page_fetcher(Arc::new(|_url: &str, _timeout: f64| None));
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
        downloader,
        freesound: Freesound::new("chave-de-teste"),
    };
    (
        TestRuntime {
            dir: ws.dir,
            runtime: Arc::new(runtime),
        },
        vistas,
    )
}

#[test]
fn test_info_success() {
    let (rt, _) = runtime_with(Ok(fake_json()));
    let result = get_video_info(
        &rt.runtime,
        "https://eaulas.usp.br/portal/video?idItem=2345",
    )
    .unwrap();
    assert_eq!(result.title, "Aula de matemática");
    assert_eq!(result.extractor, "Generic");
    assert!((result.duration - 90.0).abs() < f64::EPSILON);
    assert_eq!(result.url, "https://eaulas.usp.br/portal/video?idItem=2345");
}

#[test]
fn test_info_aceita_qualquer_site() {
    let (rt, vistas) = runtime_with(Ok(fake_json()));
    get_video_info(&rt.runtime, "https://vimeo.com/1").unwrap();
    get_video_info(&rt.runtime, "https://exemplo.com.br/aula").unwrap();
    assert_eq!(
        *vistas.lock().unwrap(),
        vec![
            "https://vimeo.com/1".to_string(),
            "https://exemplo.com.br/aula".to_string()
        ]
    );
}

#[test]
fn test_info_invalid_url() {
    let (rt, _) = runtime_with(Ok(fake_json()));
    let err = get_video_info(&rt.runtime, "ftp://exemplo.com/1").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_info_download_error() {
    // "Private video" não ganha segunda tentativa: o erro sobe direto.
    let (rt, _) = runtime_with(Err("Private video. Sign in to continue".to_string()));
    let err = get_video_info(&rt.runtime, "https://youtu.be/abc").unwrap_err();
    assert_eq!(err.code, ErrorCode::DownloadFailed);
}
