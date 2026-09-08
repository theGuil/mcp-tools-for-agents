//! Execução de processos externos com timeout.
//!
//! Único lugar do projeto que chama `std::process`. `core/ffmpeg.rs` e
//! `core/downloader.rs` usam daqui; tool nenhuma monta comando.

use std::ffi::OsStr;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Saída capturada de um processo que terminou sozinho.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub status_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    /// `true` quando o processo saiu com código zero.
    pub fn success(&self) -> bool {
        self.status_code == Some(0)
    }
}

/// Por que o processo não devolveu saída.
#[derive(Debug)]
pub enum ProcessError {
    /// O executável não pôde ser iniciado.
    Spawn(std::io::Error),
    /// O processo foi morto por exceder o tempo limite.
    Timeout(Duration),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "não foi possível iniciar o processo: {error}"),
            Self::Timeout(limit) => write!(f, "processo excedeu {:.0}s", limit.as_secs_f64()),
        }
    }
}

impl std::error::Error for ProcessError {}

/// Roda `program` com `args`, capturando stdout e stderr como texto UTF-8.
///
/// `stdin` fica fechado (`null`), como o `-nostdin` do ffmpeg. Se `timeout`
/// estourar, o processo é morto e [`ProcessError::Timeout`] é devolvido.
///
/// # Errors
///
/// [`ProcessError`] quando o processo não inicia ou excede o timeout.
pub fn run<I, S>(program: &Path, args: I, timeout: Duration) -> Result<Output, ProcessError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(ProcessError::Spawn)?;

    let stdout = child.stdout.take().map(spawn_reader);
    let stderr = child.stderr.take().map(spawn_reader);

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(ProcessError::Spawn(error)),
        }
    };

    let stdout = stdout.map(join_reader).unwrap_or_default();
    let stderr = stderr.map(join_reader).unwrap_or_default();
    match status {
        Some(status) => Ok(Output {
            status_code: status.code(),
            stdout,
            stderr,
        }),
        None => Err(ProcessError::Timeout(timeout)),
    }
}

fn spawn_reader<R: Read + Send + 'static>(mut reader: R) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = reader.read_to_end(&mut buffer);
        String::from_utf8_lossy(&buffer).into_owned()
    })
}

fn join_reader(handle: thread::JoinHandle<String>) -> String {
    handle.join().unwrap_or_default()
}
