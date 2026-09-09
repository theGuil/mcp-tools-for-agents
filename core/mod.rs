//! Infraestrutura: ffmpeg, paths, jobs, errors, binários externos, rede.
//!
//! Nada aqui conhece MCP nem `config`. Domínio importa `core`; `core` nunca
//! importa domínio.

pub mod binaries;
pub mod downloader;
pub mod errors;
pub mod ffmpeg;
pub mod fonts;
pub mod freesound;
pub mod http;
pub mod jobs;
pub mod numbers;
pub mod paths;
pub mod process;
pub mod speech;
pub mod vad;
pub mod vision;
pub mod whisper;
