//! Localiza (ou baixa sob demanda) os binários externos: ffmpeg, ffprobe e yt-dlp.
//!
//! É o que torna o servidor portátil: o usuário baixa só o `mcp-tools` e, na
//! primeira chamada que precisar de ffmpeg ou yt-dlp, o binário certo para o
//! sistema é buscado em uma pasta de cache. A ordem de busca é sempre:
//!
//! 1. o valor configurado (`FFMPEG_BIN`, `FFPROBE_BIN`, `YTDLP_BIN`), se for um
//!    caminho;
//! 2. a pasta onde o `mcp-tools` está (distribuição em zip com tudo junto);
//! 3. o `PATH` do sistema;
//! 4. a pasta de cache (`MCP_CACHE_DIR`), onde os downloads ficam;
//! 5. download, quando `MCP_AUTO_DOWNLOAD` permite.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::core::errors::{ErrorCode, ToolError, ToolResult};

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
const USER_AGENT: &str = "mcp-tools-for-agents";

/// Binário externo que o projeto sabe localizar e baixar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExternalTool {
    Ffmpeg,
    Ffprobe,
    YtDlp,
}

impl ExternalTool {
    /// Nome do executável, sem extensão.
    pub fn name(self) -> &'static str {
        match self {
            Self::Ffmpeg => "ffmpeg",
            Self::Ffprobe => "ffprobe",
            Self::YtDlp => "yt-dlp",
        }
    }

    /// Nome do arquivo no sistema atual (`.exe` no Windows).
    pub fn file_name(self) -> String {
        if cfg!(windows) {
            format!("{}.exe", self.name())
        } else {
            self.name().to_string()
        }
    }
}

/// Política de localização e download de binários externos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binaries {
    /// Pasta onde os downloads ficam (`<cache>/bin`).
    pub cache_dir: PathBuf,
    /// Permite baixar o que não for encontrado.
    pub auto_download: bool,
}

impl Default for Binaries {
    /// Cache padrão do sistema e **sem** download automático: é o que os testes
    /// usam. O servidor liga o download a partir de `Settings`.
    fn default() -> Self {
        Self {
            cache_dir: default_cache_dir(),
            auto_download: false,
        }
    }
}

impl Binaries {
    /// Cria a política com a pasta de cache e a permissão de download dadas.
    pub fn new(cache_dir: impl Into<PathBuf>, auto_download: bool) -> Self {
        Self {
            cache_dir: cache_dir.into(),
            auto_download,
        }
    }

    /// Pasta onde os binários baixados ficam.
    pub fn bin_dir(&self) -> PathBuf {
        self.cache_dir.join("bin")
    }

    /// Procura o binário sem baixar. `configured` é o valor da variável de
    /// ambiente correspondente (`"ffmpeg"` ou um caminho).
    pub fn locate(&self, tool: ExternalTool, configured: &str) -> Option<PathBuf> {
        let configured = configured.trim();
        if !configured.is_empty() && is_path_like(configured) {
            let path = PathBuf::from(configured);
            return path.is_file().then_some(path);
        }
        let names = candidate_names(tool, configured);
        for dir in search_dirs(self) {
            for name in &names {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        which_in_path(&names)
    }

    /// Localiza o binário, baixando se for permitido e necessário.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::Unavailable`] quando não há binário nem como baixar;
    /// [`ErrorCode::DownloadFailed`] quando o download falha.
    pub fn ensure(&self, tool: ExternalTool, configured: &str) -> ToolResult<PathBuf> {
        if let Some(found) = self.locate(tool, configured) {
            return Ok(found);
        }
        if !self.auto_download {
            return Err(unavailable(tool));
        }
        self.download(tool)?;
        self.locate(tool, "").ok_or_else(|| unavailable(tool))
    }

    fn download(&self, tool: ExternalTool) -> ToolResult<()> {
        let bin_dir = self.bin_dir();
        fs::create_dir_all(&bin_dir).map_err(|error| {
            download_failed(format!(
                "não foi possível criar {}: {error}",
                bin_dir.display()
            ))
        })?;
        match tool {
            ExternalTool::YtDlp => download_ytdlp(&bin_dir),
            ExternalTool::Ffmpeg | ExternalTool::Ffprobe => download_ffmpeg(&bin_dir),
        }
    }
}

fn unavailable(tool: ExternalTool) -> ToolError {
    let name = tool.name();
    ToolError::with_hint(
        format!("{name} não encontrado."),
        ErrorCode::Unavailable,
        format!(
            "Coloque o executável {name} ao lado do mcp-tools ou no PATH, aponte a variável \
             de ambiente correspondente para ele, ou deixe MCP_AUTO_DOWNLOAD=true para o \
             servidor baixar sozinho."
        ),
    )
}

fn download_failed(message: impl Into<String>) -> ToolError {
    ToolError::with_hint(
        message.into(),
        ErrorCode::DownloadFailed,
        "Confira a conexão com a internet ou instale o binário manualmente e aponte a \
         variável de ambiente para ele.",
    )
}

fn is_path_like(value: &str) -> bool {
    value.contains('/') || value.contains('\\')
}

fn candidate_names(tool: ExternalTool, configured: &str) -> Vec<String> {
    let base = if configured.is_empty() {
        tool.name().to_string()
    } else {
        configured.to_string()
    };
    if cfg!(windows) {
        let mut names = vec![base.clone()];
        if !base.to_ascii_lowercase().ends_with(".exe") {
            names.push(format!("{base}.exe"));
        }
        names
    } else {
        vec![base]
    }
}

fn search_dirs(binaries: &Binaries) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(exe_dir) = exe_dir() {
        dirs.push(exe_dir);
    }
    dirs.push(binaries.bin_dir());
    dirs
}

/// Pasta onde o executável do servidor está.
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
}

/// Equivalente de `shutil.which` para uma lista de nomes candidatos.
pub fn which_in_path(names: &[String]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Pasta de cache padrão do sistema para este projeto.
pub fn default_cache_dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        crate::core::paths::home_dir().map(|home| home.join("Library").join("Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| crate::core::paths::home_dir().map(|home| home.join(".cache")))
    };
    base.unwrap_or_else(std::env::temp_dir)
        .join("mcp-tools-for-agents")
}

// ---------------------------------------------------------------------------
// Download
// ---------------------------------------------------------------------------

fn download_ytdlp(bin_dir: &Path) -> ToolResult<()> {
    let asset = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "yt-dlp_linux",
        ("linux", "aarch64") => "yt-dlp_linux_aarch64",
        ("linux", "arm") => "yt-dlp_linux_armv7l",
        ("macos", _) => "yt-dlp_macos",
        ("windows", "x86") => "yt-dlp_x86.exe",
        ("windows", _) => "yt-dlp.exe",
        (os, arch) => {
            return Err(download_failed(format!(
                "não há build do yt-dlp para {os}/{arch}; instale manualmente."
            )))
        }
    };
    let url = format!("https://github.com/yt-dlp/yt-dlp/releases/latest/download/{asset}");
    let bytes = fetch(&url)?;
    let target = bin_dir.join(ExternalTool::YtDlp.file_name());
    write_executable(&target, &bytes)
}

fn download_ffmpeg(bin_dir: &Path) -> ToolResult<()> {
    match std::env::consts::OS {
        "linux" => {
            let arch = match std::env::consts::ARCH {
                "x86_64" => "linux64",
                "aarch64" => "linuxarm64",
                other => {
                    return Err(download_failed(format!(
                        "não há build estático do ffmpeg para linux/{other}; instale manualmente."
                    )))
                }
            };
            let url = format!(
                "https://github.com/BtbN/FFmpeg-Builds/releases/latest/download/\
                 ffmpeg-master-latest-{arch}-gpl.tar.xz"
            );
            extract_tar_xz(&fetch(&url)?, bin_dir)
        }
        "windows" => {
            let url = "https://github.com/BtbN/FFmpeg-Builds/releases/latest/download/\
                       ffmpeg-master-latest-win64-gpl.zip";
            extract_zip(&fetch(url)?, bin_dir)
        }
        "macos" => {
            extract_zip(
                &fetch("https://evermeet.cx/ffmpeg/getrelease/zip")?,
                bin_dir,
            )?;
            extract_zip(
                &fetch("https://evermeet.cx/ffmpeg/getrelease/ffprobe/zip")?,
                bin_dir,
            )
        }
        other => Err(download_failed(format!(
            "não há build estático do ffmpeg para {other}; instale manualmente."
        ))),
    }
}

fn fetch(url: &str) -> ToolResult<Vec<u8>> {
    let agent = crate::core::http::agent(DOWNLOAD_TIMEOUT, USER_AGENT);
    let response = agent
        .get(url)
        .call()
        .map_err(|error| download_failed(format!("falha ao baixar {url}: {error}")))?;
    let mut body = Vec::new();
    response
        .into_body()
        .into_reader()
        .read_to_end(&mut body)
        .map_err(|error| download_failed(format!("falha ao ler {url}: {error}")))?;
    if body.is_empty() {
        return Err(download_failed(format!("{url} devolveu um arquivo vazio.")));
    }
    Ok(body)
}

fn wanted_file(name: &str) -> Option<ExternalTool> {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    match stem {
        "ffmpeg" => Some(ExternalTool::Ffmpeg),
        "ffprobe" => Some(ExternalTool::Ffprobe),
        _ => None,
    }
}

fn extract_tar_xz(compressed: &[u8], bin_dir: &Path) -> ToolResult<()> {
    let mut tar_bytes = Vec::new();
    lzma_rs::xz_decompress(&mut std::io::Cursor::new(compressed), &mut tar_bytes)
        .map_err(|error| download_failed(format!("pacote xz inválido: {error}")))?;
    let mut archive = tar::Archive::new(std::io::Cursor::new(tar_bytes));
    let entries = archive
        .entries()
        .map_err(|error| download_failed(format!("pacote tar inválido: {error}")))?;
    let mut found = 0;
    for entry in entries {
        let mut entry =
            entry.map_err(|error| download_failed(format!("pacote tar inválido: {error}")))?;
        let path = entry
            .path()
            .map_err(|error| download_failed(format!("pacote tar inválido: {error}")))?
            .into_owned();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(tool) = wanted_file(name) else {
            continue;
        };
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| download_failed(format!("pacote tar inválido: {error}")))?;
        write_executable(&bin_dir.join(tool.file_name()), &bytes)?;
        found += 1;
    }
    if found == 0 {
        return Err(download_failed(
            "o pacote do ffmpeg não continha ffmpeg nem ffprobe.",
        ));
    }
    Ok(())
}

fn extract_zip(compressed: &[u8], bin_dir: &Path) -> ToolResult<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(compressed))
        .map_err(|error| download_failed(format!("pacote zip inválido: {error}")))?;
    let mut found = 0;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| download_failed(format!("pacote zip inválido: {error}")))?;
        if file.is_dir() {
            continue;
        }
        let name = file
            .name()
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
            .to_string();
        let Some(tool) = wanted_file(&name) else {
            continue;
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| download_failed(format!("pacote zip inválido: {error}")))?;
        write_executable(&bin_dir.join(tool.file_name()), &bytes)?;
        found += 1;
    }
    if found == 0 {
        return Err(download_failed(
            "o pacote do ffmpeg não continha ffmpeg nem ffprobe.",
        ));
    }
    Ok(())
}

fn write_executable(target: &Path, bytes: &[u8]) -> ToolResult<()> {
    let temp = target.with_extension("part");
    {
        let mut file = fs::File::create(&temp).map_err(|error| {
            download_failed(format!(
                "não foi possível gravar {}: {error}",
                temp.display()
            ))
        })?;
        file.write_all(bytes).map_err(|error| {
            download_failed(format!(
                "não foi possível gravar {}: {error}",
                temp.display()
            ))
        })?;
    }
    fs::rename(&temp, target).map_err(|error| {
        download_failed(format!(
            "não foi possível gravar {}: {error}",
            target.display()
        ))
    })?;
    mark_executable(target)
}

#[cfg(unix)]
fn mark_executable(target: &Path) -> ToolResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(target, fs::Permissions::from_mode(0o755)).map_err(|error| {
        download_failed(format!(
            "não foi possível dar permissão a {}: {error}",
            target.display()
        ))
    })
}

#[cfg(not(unix))]
fn mark_executable(_target: &Path) -> ToolResult<()> {
    Ok(())
}
