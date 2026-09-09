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

/// Amostras 16 kHz mono do fixture `tests/fixtures/speech_gap.wav`: voz real
/// em 0-2.5 s e 4-6.5 s, com 1.5 s de silêncio no meio.
pub fn speech_samples() -> Vec<f32> {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/speech_gap.wav"
    ))
    .expect("fixture speech_gap.wav");
    mcp_tools::core::vad::pcm16_to_f32(wav_data(&bytes))
}

/// Localiza o chunk `data` de um WAV PCM.
fn wav_data(bytes: &[u8]) -> &[u8] {
    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        if id == b"data" {
            return &bytes[offset + 8..(offset + 8 + size).min(bytes.len())];
        }
        offset += 8 + size + (size % 2);
    }
    panic!("WAV sem chunk data");
}
