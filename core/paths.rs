//! Resolução e validação de caminhos dentro do workspace.
//!
//! Todo caminho que o agente informa é relativo ao workspace. Nada fora dele
//! é lido ou escrito, mesmo que o agente peça com `..` ou caminho absoluto.

use std::path::{Component, Path, PathBuf};

use crate::core::errors::{ErrorCode, ToolError, ToolResult};

/// Diretório raiz onde o agente pode ler e escrever.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub root: PathBuf,
}

impl Workspace {
    /// Cria o workspace garantindo que o diretório exista.
    ///
    /// # Errors
    ///
    /// Falha de E/S ao criar ou canonicalizar o diretório. É erro de
    /// configuração, não de uso: quem chama decide como reportar.
    pub fn at(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let expanded = expand_user(path.as_ref());
        std::fs::create_dir_all(&expanded)?;
        Ok(Self {
            root: expanded.canonicalize()?,
        })
    }

    /// Converte um caminho informado pelo agente em caminho absoluto seguro.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::OutsideWorkspace`] se o caminho sair do workspace.
    pub fn resolve(&self, relative: &str) -> ToolResult<PathBuf> {
        let candidate = resolve_lexical(&self.root, Path::new(relative));
        let candidate = resolve_symlinks(candidate);
        if candidate != self.root && !candidate.starts_with(&self.root) {
            return Err(ToolError::with_hint(
                format!("Caminho '{relative}' está fora do workspace."),
                ErrorCode::OutsideWorkspace,
                "Use caminhos relativos à raiz do workspace, sem '..' nem caminho absoluto.",
            ));
        }
        Ok(candidate)
    }

    /// Resolve o caminho e garante que o arquivo exista.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::NotFound`] se o arquivo não existir.
    pub fn existing(&self, relative: &str) -> ToolResult<PathBuf> {
        let path = self.resolve(relative)?;
        if !path.is_file() {
            return Err(ToolError::with_hint(
                format!("Arquivo '{relative}' não encontrado no workspace."),
                ErrorCode::NotFound,
                "Use list_files para ver os arquivos disponíveis.",
            ));
        }
        Ok(path)
    }

    /// Caminho relativo ao workspace, em formato POSIX, para devolver ao agente.
    pub fn relative(&self, path: &Path) -> String {
        let stripped = path.strip_prefix(&self.root).unwrap_or(path);
        stripped
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Gera um caminho de saída único ao lado do arquivo de origem.
    ///
    /// * `source`: arquivo de origem.
    /// * `suffix_tag`: marcador adicionado ao nome, ex: `cut_0-10`.
    /// * `extension`: extensão final. Mantém a do original quando omitida.
    pub fn output_for(&self, source: &Path, suffix_tag: &str, extension: Option<&str>) -> PathBuf {
        let tag = safe_stem(suffix_tag);
        let ext = match extension {
            Some(ext) if ext.is_empty() || ext.starts_with('.') => ext.to_string(),
            Some(ext) => format!(".{ext}"),
            None => source
                .extension()
                .map(|ext| format!(".{}", ext.to_string_lossy()))
                .unwrap_or_default(),
        };
        let stem = source
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let parent = source.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut candidate = parent.join(format!("{stem}_{tag}{ext}"));
        let mut counter = 1;
        while candidate.exists() {
            candidate = parent.join(format!("{stem}_{tag}_{counter}{ext}"));
            counter += 1;
        }
        candidate
    }
}

/// Troca caracteres fora de `[A-Za-z0-9._-]` por `_` e remove `_` das pontas.
fn safe_stem(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len());
    let mut pending = false;
    for ch in tag.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            if pending {
                out.push('_');
                pending = false;
            }
            out.push(ch);
        } else {
            pending = true;
        }
    }
    out.trim_matches('_').to_string()
}

/// Expande `~` no início do caminho, como `Path.expanduser()` do Python.
fn expand_user(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if raw == "~" || raw.starts_with("~/") || raw.starts_with("~\\") {
        if let Some(home) = home_dir() {
            return home.join(raw.trim_start_matches('~').trim_start_matches(['/', '\\']));
        }
    }
    path.to_path_buf()
}

/// Diretório home do usuário, sem depender de crate externa.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Junta `root` e `relative` e normaliza `.` e `..` sem tocar no disco.
///
/// Um `relative` absoluto substitui `root`, como `Path.__truediv__` do Python:
/// é assim que `/etc/passwd` chega ao teste de fronteira e é rejeitado.
fn resolve_lexical(root: &Path, relative: &Path) -> PathBuf {
    let joined = root.join(relative);
    let mut out = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => out.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    out
}

/// Resolve links simbólicos no maior prefixo existente, como `Path.resolve()`
/// não estrito: o que ainda não existe é anexado sem alteração.
fn resolve_symlinks(path: PathBuf) -> PathBuf {
    if let Ok(real) = path.canonicalize() {
        return real;
    }
    let mut existing = path.clone();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    while !existing.exists() {
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name.to_os_string());
                existing = parent.to_path_buf();
            }
            _ => return path,
        }
    }
    let mut out = existing.canonicalize().unwrap_or(existing);
    for part in tail.iter().rev() {
        out.push(part);
    }
    out
}
