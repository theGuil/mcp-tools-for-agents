//! Testes de `domains/media/download.rs`, com o yt-dlp substituído por um backend
//! que copia o vídeo de exemplo para a pasta de destino.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mcp_tools::config::Settings;
use mcp_tools::core::downloader::{Downloader, VideoQuality, YtdlRequest};
use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::ffmpeg::FFmpeg;
use mcp_tools::core::freesound::Freesound;
use mcp_tools::core::jobs::JobStatus;
use mcp_tools::domains::media::download::download_video;
use mcp_tools::domains::Runtime;
use serde_json::json;

use crate::conftest::{jobs, sample_video, workspace, TestRuntime, TestWorkspace};
use crate::helpers::{unwrap, unwrap_job};
use crate::skip_without_ffmpeg;

/// Pasta de destino, lida do `-o` que o `Downloader` passa ao yt-dlp.
fn target_dir_of(request: &YtdlRequest) -> PathBuf {
    let position = request
        .extra_args
        .iter()
        .position(|arg| arg == &OsString::from("-o"))
        .expect("-o");
    Path::new(&request.extra_args[position + 1])
        .parent()
        .expect("pasta")
        .to_path_buf()
}

/// Runtime sobre `ws` cujo download "baixa" copiando `source` (ou criando um
/// arquivo vazio) como `file_name` dentro da pasta de destino. Registra as URLs vistas.
fn runtime_with(
    ws: TestWorkspace,
    source: Option<PathBuf>,
    file_name: &'static str,
) -> (TestRuntime, Arc<Mutex<Vec<String>>>) {
    let vistas: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&vistas);
    let downloader = Downloader::default()
        .with_backend(Arc::new(move |request: &YtdlRequest| {
            seen.lock().unwrap().push(request.url.clone());
            let target_dir = target_dir_of(request);
            std::fs::create_dir_all(&target_dir).unwrap();
            let target = target_dir.join(file_name);
            match &source {
                Some(sample) => {
                    std::fs::copy(sample, &target).unwrap();
                }
                None => std::fs::write(&target, b"").unwrap(),
            }
            Ok(json!({"requested_downloads": [{"filepath": target.to_string_lossy()}]}))
        }))
        .with_page_fetcher(Arc::new(|_url: &str, _timeout: f64| None));
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

/// Runtime com o vídeo de exemplo no workspace e o download falso copiando-o
/// como `file_name`. Precisa de ffmpeg.
fn fake_download(file_name: &'static str) -> (TestRuntime, Arc<Mutex<Vec<String>>>) {
    let ws = workspace();
    let sample = sample_video(&ws.workspace);
    let source = ws.workspace.existing(&sample).unwrap();
    runtime_with(ws, Some(source), file_name)
}

#[test]
fn test_download_success() {
    skip_without_ffmpeg!();
    let (rt, _) = fake_download("Aula_de_matematica_2345.mp4");
    let result = unwrap(download_video(
        &rt.runtime,
        "https://eaulas.usp.br/portal/video?idItem=2345",
        VideoQuality::P720,
        "downloads",
        false,
    ));
    assert_eq!(result.output, "downloads/Aula_de_matematica_2345.mp4");
    assert!(result.duration > 2.5);
    assert_eq!(result.quality, VideoQuality::P720);
    assert_eq!(result.title, "Aula_de_matematica_2345");
}

#[test]
fn test_download_background() {
    skip_without_ffmpeg!();
    let (rt, _) = fake_download("Aula_de_matematica_2345.mp4");
    let submitted = unwrap_job(download_video(
        &rt.runtime,
        "https://youtu.be/abc",
        VideoQuality::P720,
        "downloads",
        true,
    ));
    let job = rt
        .runtime
        .jobs
        .wait(&submitted.job_id, Some(Duration::from_secs(30)))
        .unwrap();
    assert_eq!(job.status, JobStatus::Done);
    let result = job.result.expect("resultado");
    assert!(result["output"].as_str().unwrap().ends_with(".mp4"));
}

#[test]
fn test_download_aceita_qualquer_site() {
    // O Python substitui o probe por um dicionário fixo; aqui o probe real lê o
    // vídeo de exemplo copiado, então o teste precisa de ffmpeg.
    skip_without_ffmpeg!();
    let (rt, vistas) = fake_download("arquivo.mp4");
    for url in [
        "https://eaulas.usp.br/portal/video?idItem=2345",
        "https://vimeo.com/123",
        "https://cdn.exemplo.com/aula.mp4",
    ] {
        let result = unwrap(download_video(
            &rt.runtime,
            url,
            VideoQuality::P720,
            "downloads",
            false,
        ));
        assert_eq!(result.output, "downloads/arquivo.mp4");
    }
    assert_eq!(vistas.lock().unwrap().len(), 3);
}

#[test]
fn test_download_invalid_url() {
    let (rt, _) = runtime_with(workspace(), None, "arquivo.mp4");
    let err = download_video(
        &rt.runtime,
        "ftp://exemplo.com/v",
        VideoQuality::P720,
        "downloads",
        false,
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_download_folder_outside_workspace() {
    let (rt, _) = runtime_with(workspace(), None, "arquivo.mp4");
    let err = download_video(
        &rt.runtime,
        "https://youtu.be/abc",
        VideoQuality::P720,
        "../fora",
        false,
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::OutsideWorkspace);
}

#[test]
fn test_download_folder_is_file() {
    skip_without_ffmpeg!();
    let (rt, _) = runtime_with(workspace(), None, "arquivo.mp4");
    let sample = sample_video(&rt.runtime.workspace);
    let err = download_video(
        &rt.runtime,
        "https://youtu.be/abc",
        VideoQuality::P720,
        &sample,
        false,
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
}
