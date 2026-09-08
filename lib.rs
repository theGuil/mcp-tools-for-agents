//! Servidor MCP em Rust com tools de vídeo, áudio e arquivos para agentes de IA.
//!
//! A árvore de módulos espelha a versão Python do projeto:
//!
//! ```text
//! server.rs     cria o McpServer, monta o Runtime e registra os domínios
//! config.rs     único lugar que lê variáveis de ambiente (Settings)
//! core/         infraestrutura: ffmpeg, paths, jobs, errors. NÃO conhece MCP.
//! domains/      um módulo por área (video, audio, files, jobs, media), um arquivo por tool
//! tests/        espelha core/ e domains/
//! ```

pub mod config;
pub mod core;
pub mod domains;
pub mod server;
