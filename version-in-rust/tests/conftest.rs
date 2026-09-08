//! Fixtures compartilhadas: workspace temporário e vídeo de exemplo.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use mcp_tools::config::Settings;
use mcp_tools::core::downloader::Downloader;
use mcp_tools::core::ffmpeg::FFmpeg;
use mcp_tools::core::freesound::Freesound;
use mcp_tools::core::jobs::JobManager;
use mcp_tools::core::paths::Workspace;
use mcp_tools::domains::Runtime;
use tempfile::TempDir;

pub const SAMPLE_DURATION: f64 = 3.0;

/// `true` quando ffmpeg e ffprobe estão instalados. Testes que precisam deles
/// chamam [`skip_without_ffmpeg!`] e saem cedo quando não estão.
pub fn ffmpeg_available() -> bool {
    FFmpeg::default().is_available()
}

/// Sai do teste com aviso quando o ffmpeg não está instalado, como o
/// `pytest.mark.ffmpeg` da versão Python.
#[macro_export]
macro_rules! skip_without_ffmpeg {
    () => {
        if !$crate::conftest::ffmpeg_available() {
            eprintln!("pulado: ffmpeg/ffprobe não instalados");
            return;
        }
    };
}

/// Workspace isolado por teste. O `TempDir` precisa viver enquanto o teste roda.
pub struct TestWorkspace {
    pub dir: TempDir,
    pub workspace: Workspace,
}

pub fn workspace() -> TestWorkspace {
    let dir = tempfile::tempdir().expect("tmp");
    let workspace = Workspace::at(dir.path().join("ws")).expect("workspace");
    TestWorkspace { dir, workspace }
}

/// Gerenciador de jobs com uma thread.
pub fn jobs() -> JobManager {
    JobManager::new(1)
}

/// Runtime completo apontando para o workspace temporário.
pub struct TestRuntime {
    pub dir: TempDir,
    pub runtime: Arc<Runtime>,
}

impl TestRuntime {
    pub fn root(&self) -> &Path {
        &self.runtime.workspace.root
    }
}

pub fn runtime() -> TestRuntime {
    let TestWorkspace { dir, workspace } = workspace();
    let mut env = HashMap::new();
    env.insert(
        "WORKSPACE_DIR".to_string(),
        workspace.root.to_string_lossy().into_owned(),
    );
    let settings = Settings::from_map(&env).expect("settings");
    let runtime = Runtime {
        settings,
        workspace,
        ffmpeg: FFmpeg::default(),
        jobs: jobs(),
        downloader: Downloader::default(),
        freesound: Freesound::new("chave-de-teste"),
    };
    TestRuntime {
        dir,
        runtime: Arc::new(runtime),
    }
}

/// Gera um vídeo sintético de 3s com áudio dentro do workspace. Devolve o
/// caminho relativo (`sample.mp4`).
pub fn sample_video(workspace: &Workspace) -> String {
    let target = workspace.root.join("sample.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=size=320x240:rate=25:duration={SAMPLE_DURATION}"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={SAMPLE_DURATION}"),
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-g",
            "12",
            "-c:a",
            "aac",
        ])
        .arg(&target)
        .status()
        .expect("ffmpeg");
    assert!(status.success(), "ffmpeg não gerou o vídeo de exemplo");
    "sample.mp4".to_string()
}
