//! Cliente tipado para a API do Freesound (<https://freesound.org>).
//!
//! Acervo gratuito de efeitos sonoros com licença Creative Commons. Com a chave
//! simples (`token`) dá para buscar sons e baixar o preview em MP3 de 128 kbps,
//! que é suficiente para efeito sonoro em vídeo curto. O arquivo original em alta
//! qualidade exige OAuth2 e não é usado aqui.
//!
//! Segundo ponto do projeto que conversa com a internet, ao lado do
//! `core/downloader.rs`. O restante recebe apenas metadados tipados e o caminho
//! do arquivo baixado.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::errors::{ErrorCode, ToolError, ToolResult};
use crate::core::numbers::format_g;

const API_BASE: &str = "https://freesound.org/apiv2";
const FIELDS: &str = "id,name,duration,previews,license,username,tags";
const PREVIEW_KEY: &str = "preview-hq-mp3";
const MAX_LIMIT: u32 = 30;
const MAX_PREVIEW_BYTES: u64 = 20_000_000;
const MAX_NAME: usize = 40;
const HTTP_UNAUTHORIZED: u16 = 401;
const HTTP_NOT_FOUND: u16 = 404;
const HTTP_TOO_MANY: u16 = 429;
// Muitos nomes no Freesound terminam com a extensão do original ("Boom.wav").
const AUDIO_EXTENSIONS: &[&str] = &["wav", "mp3", "ogg", "flac", "aif", "aiff", "m4a", "aac"];

// Nomes curtos de licença, mais legíveis para o agente que a URL completa.
const LICENSES: &[(&str, &str)] = &[
    ("publicdomain/zero", "CC0"),
    ("licenses/by-nc", "CC BY-NC"),
    ("licenses/by", "CC BY"),
    ("sampling+", "Sampling+"),
];

const KEY_HINT: &str =
    "Confira a chave em FREESOUND_API_KEY. Crie uma grátis em https://freesound.org/apiv2/apply/.";

/// Um som encontrado no Freesound.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SoundCandidate {
    pub sound_id: i64,
    pub name: String,
    pub duration: f64,
    pub license: String,
    pub author: String,
    pub tags: Vec<String>,
    pub preview_url: String,
}

/// Função que baixa os bytes de uma URL. Injetável para os testes não usarem rede.
pub type Fetcher = Arc<dyn Fn(&str) -> ToolResult<Vec<u8>> + Send + Sync>;

/// Busca e baixa efeitos sonoros do Freesound, com erros amigáveis.
#[derive(Clone)]
pub struct Freesound {
    pub api_key: String,
    pub timeout_seconds: f64,
    fetcher: Fetcher,
}

impl std::fmt::Debug for Freesound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Freesound")
            .field("timeout_seconds", &self.timeout_seconds)
            .finish()
    }
}

impl Freesound {
    /// Cliente real, com timeout de 30 segundos por requisição.
    pub fn new(api_key: impl Into<String>) -> Self {
        let timeout_seconds = 30.0;
        Self {
            api_key: api_key.into(),
            timeout_seconds,
            fetcher: Arc::new(move |url| http_get(url, timeout_seconds)),
        }
    }

    /// Cliente com transporte substituído, para testes.
    pub fn with_fetcher(api_key: impl Into<String>, fetcher: Fetcher) -> Self {
        Self {
            api_key: api_key.into(),
            timeout_seconds: 30.0,
            fetcher,
        }
    }

    /// Busca sons por texto, do mais relevante para o menos relevante.
    ///
    /// * `query`: termos de busca, em inglês de preferência (é o idioma do acervo).
    /// * `limit`: quantidade máxima de resultados, até 30.
    /// * `max_duration`: descarta sons mais longos que isso, em segundos.
    ///
    /// # Errors
    ///
    /// Se a consulta for inválida ou a API falhar.
    pub fn search(
        &self,
        query: &str,
        limit: u32,
        max_duration: Option<f64>,
    ) -> ToolResult<Vec<SoundCandidate>> {
        let cleaned = query.trim();
        if cleaned.is_empty() {
            return Err(ToolError::with_hint(
                "query não pode ser vazia.",
                ErrorCode::InvalidArgument,
                "Descreva o som em inglês, ex: 'vine boom', 'record scratch', 'ding'.",
            ));
        }
        if limit < 1 {
            return Err(ToolError::new(
                "limit deve ser >= 1.",
                ErrorCode::InvalidArgument,
            ));
        }
        if max_duration.is_some_and(|d| d <= 0.0) {
            return Err(ToolError::new(
                "max_duration deve ser maior que zero.",
                ErrorCode::InvalidArgument,
            ));
        }
        let mut params = vec![
            ("query", cleaned.to_string()),
            ("fields", FIELDS.to_string()),
            ("page_size", limit.min(MAX_LIMIT).to_string()),
            ("token", self.require_key()?),
        ];
        if let Some(max) = max_duration {
            params.push(("filter", format!("duration:[0 TO {}]", format_g(max))));
        }
        let data = self.get_json(&format!("{API_BASE}/search/text/?{}", urlencode(&params)))?;
        Ok(parse_search(&data))
    }

    /// Lê os metadados de um som pelo id.
    ///
    /// # Errors
    ///
    /// Se o som não existir ou a API falhar.
    pub fn sound(&self, sound_id: i64) -> ToolResult<SoundCandidate> {
        if sound_id <= 0 {
            return Err(ToolError::new(
                "sound_id deve ser um inteiro positivo.",
                ErrorCode::InvalidArgument,
            ));
        }
        let params = [
            ("fields", FIELDS.to_string()),
            ("token", self.require_key()?),
        ];
        let data = self.get_json(&format!(
            "{API_BASE}/sounds/{sound_id}/?{}",
            urlencode(&params)
        ))?;
        parse_sound(&data).ok_or_else(|| {
            ToolError::with_hint(
                format!("Som {sound_id} veio sem preview em MP3."),
                ErrorCode::DownloadFailed,
                "Escolha outro resultado de search_sound_effects.",
            )
        })
    }

    /// Baixa o preview MP3 de um som para `target_dir`.
    ///
    /// O nome do arquivo junta o nome do som e o id, então baixar o mesmo som
    /// duas vezes reaproveita o arquivo já existente.
    ///
    /// # Errors
    ///
    /// Se o download falhar.
    pub fn download_preview(
        &self,
        candidate: &SoundCandidate,
        target_dir: &Path,
    ) -> ToolResult<PathBuf> {
        let sound_id = candidate.sound_id;
        std::fs::create_dir_all(target_dir).map_err(|error| {
            ToolError::new(
                format!("Não foi possível criar {}: {error}.", target_dir.display()),
                ErrorCode::DownloadFailed,
            )
        })?;
        let target = target_dir.join(format!("{}_{sound_id}.mp3", safe_name(&candidate.name)));
        if target.is_file() && std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0) > 0 {
            return Ok(target);
        }
        let raw = self.get_bytes(&candidate.preview_url)?;
        if raw.is_empty() {
            return Err(ToolError::with_hint(
                format!("Preview do som {sound_id} veio vazio."),
                ErrorCode::DownloadFailed,
                "Tente outro resultado de search_sound_effects.",
            ));
        }
        std::fs::write(&target, raw).map_err(|error| {
            ToolError::new(
                format!("Não foi possível gravar {}: {error}.", target.display()),
                ErrorCode::DownloadFailed,
            )
        })?;
        Ok(target)
    }

    fn require_key(&self) -> ToolResult<String> {
        let key = self.api_key.trim();
        if key.is_empty() {
            return Err(ToolError::with_hint(
                "Freesound indisponível: chave de API não configurada.",
                ErrorCode::Unavailable,
                KEY_HINT,
            ));
        }
        Ok(key.to_string())
    }

    fn get_json(&self, url: &str) -> ToolResult<Value> {
        let raw = self.get_bytes(url)?;
        let data: Value = serde_json::from_slice(&raw).map_err(|_| {
            ToolError::with_hint(
                "Resposta do Freesound não é JSON válido.",
                ErrorCode::DownloadFailed,
                "Tente de novo em alguns segundos.",
            )
        })?;
        if !data.is_object() {
            return Err(ToolError::new(
                "Resposta do Freesound em formato inesperado.",
                ErrorCode::DownloadFailed,
            ));
        }
        Ok(data)
    }

    /// Baixa os bytes de uma URL https. Público para os testes, como `_get_bytes`.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::DownloadFailed`] se a URL não for https ou a rede falhar.
    pub fn get_bytes(&self, url: &str) -> ToolResult<Vec<u8>> {
        if !url.starts_with("https://") {
            return Err(ToolError::with_hint(
                format!("URL inesperada do Freesound: '{url}'."),
                ErrorCode::DownloadFailed,
                "Só endereços https são aceitos.",
            ));
        }
        (self.fetcher)(url)
    }
}

fn http_get(url: &str, timeout_seconds: f64) -> ToolResult<Vec<u8>> {
    let agent = crate::core::http::agent(
        Duration::from_secs_f64(timeout_seconds),
        "mcp-tools-for-agents",
    );
    let response = match agent.get(url).call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(status)) => return Err(http_error(status)),
        Err(error) => {
            return Err(ToolError::with_hint(
                format!("Não foi possível falar com o Freesound: {error}."),
                ErrorCode::DownloadFailed,
                "Confira a conexão com a internet e tente de novo.",
            ))
        }
    };
    let mut body = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(MAX_PREVIEW_BYTES)
        .read_to_end(&mut body)
        .map_err(|error| {
            ToolError::with_hint(
                format!("Não foi possível falar com o Freesound: {error}."),
                ErrorCode::DownloadFailed,
                "Confira a conexão com a internet e tente de novo.",
            )
        })?;
    Ok(body)
}

/// Converte um status HTTP de erro no `ToolError` correspondente.
pub fn http_error(status: u16) -> ToolError {
    match status {
        HTTP_UNAUTHORIZED => ToolError::with_hint(
            "Freesound recusou a chave de API.",
            ErrorCode::Unavailable,
            KEY_HINT,
        ),
        HTTP_NOT_FOUND => ToolError::with_hint(
            "Som não encontrado no Freesound.",
            ErrorCode::NotFound,
            "Use search_sound_effects para achar um sound_id válido.",
        ),
        HTTP_TOO_MANY => ToolError::with_hint(
            "Limite de requisições do Freesound atingido.",
            ErrorCode::DownloadFailed,
            "Aguarde um minuto antes de buscar de novo.",
        ),
        other => ToolError::with_hint(
            format!("Freesound respondeu HTTP {other}."),
            ErrorCode::DownloadFailed,
            "Tente de novo em alguns segundos.",
        ),
    }
}

/// Converte o nome do som em um pedaço seguro de nome de arquivo, sem extensão.
pub fn safe_name(name: &str) -> String {
    let trimmed = name.trim();
    let stem = match trimmed.rsplit_once('.') {
        Some((stem, ext)) if AUDIO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) => stem,
        _ => trimmed,
    };
    let mut cleaned = String::with_capacity(stem.len());
    let mut pending = false;
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            if pending {
                cleaned.push('_');
                pending = false;
            }
            cleaned.push(ch);
        } else {
            pending = true;
        }
    }
    let cleaned: String = cleaned.trim_matches('_').chars().take(MAX_NAME).collect();
    let cleaned = cleaned.trim_matches('_');
    if cleaned.is_empty() {
        "sound".to_string()
    } else {
        cleaned.to_string()
    }
}

/// Converte a resposta bruta de `/search/text/` em candidatos.
pub fn parse_search(data: &Value) -> Vec<SoundCandidate> {
    data.get("results")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_sound).collect())
        .unwrap_or_default()
}

/// Converte um som bruto da API em [`SoundCandidate`].
///
/// Devolve `None` quando o som não tem id ou preview em MP3, já que sem
/// preview não há o que baixar.
pub fn parse_sound(item: &Value) -> Option<SoundCandidate> {
    let sound_id = item.get("id")?.as_i64()?;
    let preview = item
        .get("previews")?
        .as_object()?
        .get(PREVIEW_KEY)?
        .as_str()?;
    if preview.is_empty() {
        return None;
    }
    let tags = item
        .get("tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Some(SoundCandidate {
        sound_id,
        name: non_empty_str(item.get("name")).unwrap_or_else(|| "sound".to_string()),
        duration: item.get("duration").and_then(Value::as_f64).unwrap_or(0.0),
        license: short_license(&non_empty_str(item.get("license")).unwrap_or_default()),
        author: non_empty_str(item.get("username")).unwrap_or_default(),
        tags,
        preview_url: preview.to_string(),
    })
}

fn non_empty_str(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Resume a URL da licença em um nome curto (`CC0`, `CC BY`...).
pub fn short_license(url: &str) -> String {
    for (fragment, label) in LICENSES {
        if url.contains(fragment) {
            return (*label).to_string();
        }
    }
    if url.is_empty() {
        "desconhecida".to_string()
    } else {
        url.to_string()
    }
}

/// Codifica pares chave/valor como `application/x-www-form-urlencoded`
/// (espaço vira `+`), igual ao `urlencode` do Python.
pub fn urlencode(params: &[(&str, String)]) -> String {
    params
        .iter()
        .map(|(key, value)| format!("{}={}", quote_plus(key), quote_plus(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn quote_plus(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}
