//! Tool `list_files`: lista arquivos do workspace.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::domains::{McpServer, Runtime};

/// Tipo de arquivo, pela extensão.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    Video,
    Audio,
    Image,
    Other,
}

const KINDS: &[(&str, FileKind)] = &[
    (".mp4", FileKind::Video),
    (".mov", FileKind::Video),
    (".mkv", FileKind::Video),
    (".webm", FileKind::Video),
    (".avi", FileKind::Video),
    (".mp3", FileKind::Audio),
    (".wav", FileKind::Audio),
    (".aac", FileKind::Audio),
    (".flac", FileKind::Audio),
    (".m4a", FileKind::Audio),
    (".png", FileKind::Image),
    (".jpg", FileKind::Image),
    (".jpeg", FileKind::Image),
    (".webp", FileKind::Image),
];
const MAX_ENTRIES: usize = 500;

/// Um arquivo do workspace.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FileEntry {
    pub path: String,
    pub kind: FileKind,
    pub size_bytes: u64,
}

/// Arquivos encontrados.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ListFilesResult {
    pub directory: String,
    pub files: Vec<FileEntry>,
    pub count: usize,
    pub truncated: bool,
}

/// Classifica um arquivo pela extensão (com ponto, ex: `.mp4`).
pub fn kind_of(suffix: &str) -> FileKind {
    let lower = suffix.to_ascii_lowercase();
    KINDS
        .iter()
        .find(|(ext, _)| *ext == lower)
        .map_or(FileKind::Other, |(_, kind)| *kind)
}

fn suffix_of(path: &Path) -> String {
    path.extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default()
}

fn collect_files(base: &Path, recursive: bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            out.push(path);
        } else if recursive && path.is_dir() {
            collect_files(&path, recursive, out);
        }
    }
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// [`ErrorCode::NotFound`] se o diretório não existir.
pub fn list_files(
    runtime: &Runtime,
    directory: &str,
    kind: Option<FileKind>,
    recursive: bool,
) -> ToolResult<ListFilesResult> {
    let base = runtime.workspace.resolve(directory)?;
    if !base.is_dir() {
        return Err(ToolError::new(
            format!("Diretório '{directory}' não existe."),
            ErrorCode::NotFound,
        ));
    }
    let mut paths = Vec::new();
    collect_files(&base, recursive, &mut paths);
    paths.sort();
    let mut entries = Vec::new();
    let mut truncated = false;
    for path in paths {
        let entry_kind = kind_of(&suffix_of(&path));
        if kind.is_some_and(|wanted| wanted != entry_kind) {
            continue;
        }
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        entries.push(FileEntry {
            path: runtime.workspace.relative(&path),
            kind: entry_kind,
            size_bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        });
    }
    let directory = runtime.workspace.relative(&base);
    Ok(ListFilesResult {
        directory: if directory.is_empty() {
            ".".to_string()
        } else {
            directory
        },
        count: entries.len(),
        files: entries,
        truncated,
    })
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Subdiretório do workspace. Raiz por padrão.
    #[serde(default = "default_directory")]
    pub directory: String,
    /// Filtra por tipo: video, audio, image ou other.
    #[serde(default)]
    pub kind: Option<FileKind>,
    /// Inclui subdiretórios.
    #[serde(default = "default_true")]
    pub recursive: bool,
}

fn default_directory() -> String {
    ".".to_string()
}

fn default_true() -> bool {
    true
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "list_files",
        "Lista os arquivos disponíveis no workspace.\n\n\
         Chame primeiro para descobrir com o que trabalhar. Não altera nada.",
        move |params: Params| {
            guarded(list_files(
                &runtime,
                &params.directory,
                params.kind,
                params.recursive,
            ))
        },
    );
}
