//! Configuração do servidor, lida de variáveis de ambiente.
//!
//! Nenhum outro módulo lê `std::env::var`. Tudo passa por [`Settings`].

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::str::FromStr;

use crate::core::errors::{ErrorCode, ToolError, ToolResult};

/// Nome de um domínio de tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DomainName {
    Audio,
    Files,
    Jobs,
    Media,
    Video,
}

/// Todos os domínios, em ordem alfabética (a ordem de registro).
pub const ALL_DOMAINS: [DomainName; 5] = [
    DomainName::Audio,
    DomainName::Files,
    DomainName::Jobs,
    DomainName::Media,
    DomainName::Video,
];

impl DomainName {
    /// Nome como aparece em `MCP_DOMAINS`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Files => "files",
            Self::Jobs => "jobs",
            Self::Media => "media",
            Self::Video => "video",
        }
    }
}

impl FromStr for DomainName {
    type Err = ();

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        ALL_DOMAINS
            .into_iter()
            .find(|domain| domain.as_str() == name)
            .ok_or(())
    }
}

impl std::fmt::Display for DomainName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

const DEFAULT_WORKSPACE: &str = "./workspace";
const DEFAULT_FFMPEG_TIMEOUT: f64 = 600.0;
const DEFAULT_JOB_WORKERS: usize = 2;
const DEFAULT_DOWNLOAD_TIMEOUT: f64 = 900.0;
// Chave da API do Freesound (https://freesound.org/apiv2/apply/), usada pelas
// tools de efeito sonoro. Embutida por decisão do projeto; FREESOUND_API_KEY
// no ambiente tem prioridade quando definida.
const DEFAULT_FREESOUND_API_KEY: &str = "xNwRdQFzMom4V2LYZjeeKelEJX1Dgr5Sv4FIh1Ek";

/// Parâmetros de execução do servidor.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub workspace_dir: PathBuf,
    pub ffmpeg_bin: String,
    pub ffprobe_bin: String,
    /// Executável do yt-dlp (`YTDLP_BIN`). Nome no PATH ou caminho.
    pub ytdlp_bin: String,
    pub ffmpeg_timeout: f64,
    pub job_workers: usize,
    pub download_timeout: f64,
    pub freesound_api_key: String,
    pub domains: BTreeSet<DomainName>,
    /// Pasta onde ficam binários, fontes e modelos baixados (`MCP_CACHE_DIR`).
    pub cache_dir: PathBuf,
    /// Permite baixar ffmpeg, yt-dlp e modelos sob demanda (`MCP_AUTO_DOWNLOAD`).
    pub auto_download: bool,
    pub server_name: String,
}

impl Settings {
    /// Monta as configurações a partir das variáveis de ambiente do processo.
    ///
    /// # Errors
    ///
    /// [`ToolError`] se algum valor for inválido.
    pub fn from_env() -> ToolResult<Self> {
        Self::from_source(|key| std::env::var(key).ok())
    }

    /// Monta as configurações a partir de um mapa, útil em testes.
    ///
    /// # Errors
    ///
    /// [`ToolError`] se algum valor for inválido.
    pub fn from_map(env: &HashMap<String, String>) -> ToolResult<Self> {
        Self::from_source(|key| env.get(key).cloned())
    }

    fn from_source(get: impl Fn(&str) -> Option<String>) -> ToolResult<Self> {
        let freesound = get("FREESOUND_API_KEY")
            .map(|v| v.trim().to_string())
            .unwrap_or_default();
        Ok(Self {
            workspace_dir: PathBuf::from(
                get("WORKSPACE_DIR").unwrap_or_else(|| DEFAULT_WORKSPACE.to_string()),
            ),
            ffmpeg_bin: get("FFMPEG_BIN").unwrap_or_else(|| "ffmpeg".to_string()),
            ffprobe_bin: get("FFPROBE_BIN").unwrap_or_else(|| "ffprobe".to_string()),
            ytdlp_bin: get("YTDLP_BIN").unwrap_or_else(|| "yt-dlp".to_string()),
            ffmpeg_timeout: parse_float(&get, "FFMPEG_TIMEOUT", DEFAULT_FFMPEG_TIMEOUT)?,
            job_workers: parse_int(&get, "JOB_WORKERS", DEFAULT_JOB_WORKERS)?,
            download_timeout: parse_float(&get, "DOWNLOAD_TIMEOUT", DEFAULT_DOWNLOAD_TIMEOUT)?,
            freesound_api_key: if freesound.is_empty() {
                DEFAULT_FREESOUND_API_KEY.to_string()
            } else {
                freesound
            },
            domains: parse_domains(get("MCP_DOMAINS").as_deref())?,
            cache_dir: get("MCP_CACHE_DIR")
                .filter(|v| !v.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(crate::core::binaries::default_cache_dir),
            auto_download: parse_bool(&get, "MCP_AUTO_DOWNLOAD", true)?,
            server_name: "mcp-tools-for-agents".to_string(),
        })
    }
}

fn parse_float(get: &impl Fn(&str) -> Option<String>, key: &str, default: f64) -> ToolResult<f64> {
    let Some(raw) = get(key).filter(|v| !v.trim().is_empty()) else {
        return Ok(default);
    };
    let value: f64 = raw.trim().parse().map_err(|_| {
        ToolError::new(
            format!("{key} deve ser numérico, recebido '{raw}'."),
            ErrorCode::InvalidArgument,
        )
    })?;
    if value <= 0.0 {
        return Err(ToolError::new(
            format!("{key} deve ser maior que zero."),
            ErrorCode::InvalidArgument,
        ));
    }
    Ok(value)
}

fn parse_int(
    get: &impl Fn(&str) -> Option<String>,
    key: &str,
    default: usize,
) -> ToolResult<usize> {
    let Some(raw) = get(key).filter(|v| !v.trim().is_empty()) else {
        return Ok(default);
    };
    let value: i64 = raw.trim().parse().map_err(|_| {
        ToolError::new(
            format!("{key} deve ser inteiro, recebido '{raw}'."),
            ErrorCode::InvalidArgument,
        )
    })?;
    if value <= 0 {
        return Err(ToolError::new(
            format!("{key} deve ser maior que zero."),
            ErrorCode::InvalidArgument,
        ));
    }
    Ok(value as usize)
}

fn parse_bool(get: &impl Fn(&str) -> Option<String>, key: &str, default: bool) -> ToolResult<bool> {
    let Some(raw) = get(key).filter(|v| !v.trim().is_empty()) else {
        return Ok(default);
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(ToolError::with_hint(
            format!("{key} deve ser true ou false, recebido '{raw}'."),
            ErrorCode::InvalidArgument,
            "Use true/false, 1/0, yes/no ou on/off.",
        )),
    }
}

fn parse_domains(raw: Option<&str>) -> ToolResult<BTreeSet<DomainName>> {
    let Some(raw) = raw.filter(|v| !v.trim().is_empty()) else {
        return Ok(ALL_DOMAINS.into_iter().collect());
    };
    let mut chosen = BTreeSet::new();
    for item in raw.split(',') {
        let name = item.trim().to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        let domain = DomainName::from_str(&name).map_err(|()| {
            ToolError::with_hint(
                format!("Domínio desconhecido em MCP_DOMAINS: '{name}'."),
                ErrorCode::InvalidArgument,
                format!(
                    "Valores aceitos: {}.",
                    ALL_DOMAINS
                        .iter()
                        .map(|d| d.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )
        })?;
        chosen.insert(domain);
    }
    Ok(chosen)
}
