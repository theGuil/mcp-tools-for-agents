//! Wrapper tipado para o `yt-dlp`.
//!
//! Junto com `core/freesound.rs`, é onde o projeto conversa com a internet. O
//! restante recebe apenas metadados tipados e o caminho do arquivo baixado.
//!
//! O yt-dlp é chamado como executável ao lado do servidor: não existe porte
//! dele para Rust, e o binário standalone que o projeto publica no GitHub roda
//! sem Python instalado. `core/binaries.rs` o localiza ou baixa sob demanda.
//!
//! Aceita qualquer URL http(s). O yt-dlp tem extractor nativo para mais de mil
//! sites e, quando nenhum reconhece a página, o extractor genérico baixa o HTML e
//! procura sozinho a fonte do vídeo: tag `<video>`, `<source>`, manifesto HLS
//! `.m3u8`, DASH `.mpd`, player embutido ou JSON-LD.
//!
//! São três tentativas, nesta ordem: o extractor nativo do site, o genérico e, se
//! nenhum achar nada, uma varredura própria do HTML que desce um nível nos iframes
//! -- é lá que players caseiros, como os de portais de aula, escondem o arquivo.

use std::collections::HashSet;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

use crate::core::binaries::{Binaries, ExternalTool};
use crate::core::errors::{ErrorCode, ToolError, ToolResult};
use crate::core::process::{self, ProcessError};

/// Qualidade pedida ao yt-dlp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
pub enum VideoQuality {
    #[serde(rename = "best")]
    Best,
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "720p")]
    P720,
    #[serde(rename = "480p")]
    P480,
    #[serde(rename = "360p")]
    P360,
    #[serde(rename = "audio")]
    Audio,
}

impl VideoQuality {
    /// Nome como o agente escreve: `best`, `720p`, `audio`...
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Best => "best",
            Self::P1080 => "1080p",
            Self::P720 => "720p",
            Self::P480 => "480p",
            Self::P360 => "360p",
            Self::Audio => "audio",
        }
    }

    /// Seletor de formato do yt-dlp. O segundo nível de cada cadeia (sem filtro
    /// de extensão) é o que faz sites genéricos funcionarem: HLS e players
    /// caseiros raramente entregam mp4/m4a puros.
    pub fn format(self) -> String {
        match self {
            Self::Best => {
                "bestvideo[ext=mp4]+bestaudio[ext=m4a]/bestvideo+bestaudio/best[ext=mp4]/best"
                    .to_string()
            }
            Self::P1080 => capped(1080),
            Self::P720 => capped(720),
            Self::P480 => capped(480),
            Self::P360 => capped(360),
            Self::Audio => "bestaudio[ext=m4a]/bestaudio/best".to_string(),
        }
    }
}

/// Formato preferindo MP4, aceitando qualquer container e caindo para o melhor disponível.
fn capped(height: u32) -> String {
    format!(
        "bestvideo[height<={height}][ext=mp4]+bestaudio[ext=m4a]/\
         bestvideo[height<={height}]+bestaudio/\
         best[height<={height}]/best"
    )
}

const MAX_TITLE: usize = 60;
// O id do YouTube tem 11 caracteres, mas o extractor genérico usa a URL
// inteira como id. Sem corte, o nome do arquivo estoura o limite do sistema.
const MAX_ID: usize = 40;
const PAGE_TIMEOUT: f64 = 30.0;
const MAX_PAGE_BYTES: u64 = 2_000_000;
const MAX_FRAMES: usize = 3;
const MAX_CANDIDATES: usize = 6;
// Alguns sistemas de arquivos guardam mtime com menos precisão que o relógio,
// e o arquivo recém-gravado parece anterior ao início.
const MTIME_SLACK: f64 = 2.0;
// Muitos portais devolvem uma página vazia para cliente que não parece navegador.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

const DEFAULT_HINT: &str = "Confira se o link abre no navegador sem login. Se abrir, copie a URL \
                            direta do vídeo (.mp4 ou .m3u8) e tente com ela.";

// Casos em que insistir não adianta: o hint manda o agente parar em vez de
// gastar tentativas. Ordem importa, a primeira que casar vence.
const FAILURE_HINTS: &[(&str, &str)] = &[
    (
        r"\bdrm\b|widevine|fairplay|playready",
        "O vídeo é protegido por DRM e nenhuma ferramenta consegue baixá-lo. \
         Não tente de novo: peça o arquivo ao usuário.",
    ),
    (
        r"sign in|log ?in|cookies|private|members.only|premium|subscriber|purchase|paid",
        "A página exige login. Não tente de novo com esta URL: peça ao usuário \
         um link público ou o arquivo já baixado.",
    ),
    (
        r"unavailable|removed|deleted|not found|404|terminated|geo.?block|geo.?restrict|not available in your country",
        "O vídeo não existe mais ou está bloqueado nesta região. Confira o link \
         no navegador antes de tentar de novo.",
    ),
    (
        r"unsupported url|unable to extract|no video|no media|found no|no formats",
        "Nenhum vídeo foi encontrado nessa página. Ela pode montar o player por \
         JavaScript: abra o link no navegador, copie a URL direta do arquivo \
         (.mp4 ou .m3u8) ou a URL do player embutido, e tente com ela.",
    ),
];

// Falha de DRM, login ou vídeo removido não muda de resultado com outro
// extractor. Qualquer outra vale uma segunda tentativa com o genérico.
const NO_RETRY: usize = 3;

fn failure_patterns() -> &'static Vec<(Regex, &'static str)> {
    static PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        FAILURE_HINTS
            .iter()
            .map(|(pattern, hint)| {
                let compiled = RegexBuilder::new(pattern)
                    .case_insensitive(true)
                    .build()
                    .expect("padrão de falha inválido");
                (compiled, *hint)
            })
            .collect()
    })
}

fn media_url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(
            r#"https?://[^\s"'<>\\]+?\.(?:mp4|m3u8|mpd|webm|mov|m4v)(?:\?[^\s"'<>\\]*)?"#,
        )
        .case_insensitive(true)
        .build()
        .expect("regex de mídia inválida")
    })
}

fn tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(r"<\s*(video|source|audio|iframe|embed)\b([^>]*)>")
            .case_insensitive(true)
            .build()
            .expect("regex de tag inválida")
    })
}

fn attr_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        RegexBuilder::new(
            r#"([a-zA-Z_:][-a-zA-Z0-9_:.]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))"#,
        )
        .build()
        .expect("regex de atributo inválida")
    })
}

/// Capítulo declarado pelo autor do vídeo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chapter {
    pub title: String,
    pub start: f64,
    pub end: f64,
}

/// Metadados de um vídeo antes do download.
///
/// Campos como `channel` e `view_count` só existem em sites que os
/// publicam; em páginas comuns vêm nulos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoInfo {
    pub id: String,
    pub title: String,
    pub extractor: String,
    pub channel: Option<String>,
    pub duration: f64,
    pub view_count: Option<i64>,
    pub upload_date: Option<String>,
    pub description: String,
    pub thumbnail: Option<String>,
    pub chapters: Vec<Chapter>,
}

/// Uma chamada ao yt-dlp, como o backend a recebe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YtdlRequest {
    pub url: String,
    /// `--force-generic-extractor`.
    pub generic: bool,
    /// `false` só lê metadados (`--skip-download`).
    pub download: bool,
    /// Argumentos extras (formato, saída, pós-processamento).
    pub extra_args: Vec<OsString>,
}

/// Executa uma chamada ao yt-dlp e devolve o JSON do vídeo, ou a mensagem de
/// erro limpa. Injetável para os testes não dependerem do executável.
pub type YtdlBackend = Arc<dyn Fn(&YtdlRequest) -> Result<Value, String> + Send + Sync>;

/// Baixa o HTML de uma página (`None` quando não dá para ler). Injetável.
pub type PageFetcher = Arc<dyn Fn(&str, f64) -> Option<String> + Send + Sync>;

/// Consulta e baixa vídeos de qualquer site, com erros amigáveis.
#[derive(Clone)]
pub struct Downloader {
    pub ffmpeg_bin: String,
    pub ytdlp_bin: String,
    pub timeout_seconds: f64,
    pub binaries: Binaries,
    backend: Option<YtdlBackend>,
    fetch_page: PageFetcher,
}

impl std::fmt::Debug for Downloader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Downloader")
            .field("ffmpeg_bin", &self.ffmpeg_bin)
            .field("ytdlp_bin", &self.ytdlp_bin)
            .field("timeout_seconds", &self.timeout_seconds)
            .finish_non_exhaustive()
    }
}

impl Default for Downloader {
    fn default() -> Self {
        Self::new("ffmpeg", "yt-dlp", 900.0, Binaries::default())
    }
}

impl Downloader {
    /// Cria o wrapper real, que chama o executável do yt-dlp.
    pub fn new(
        ffmpeg_bin: impl Into<String>,
        ytdlp_bin: impl Into<String>,
        timeout_seconds: f64,
        binaries: Binaries,
    ) -> Self {
        Self {
            ffmpeg_bin: ffmpeg_bin.into(),
            ytdlp_bin: ytdlp_bin.into(),
            timeout_seconds,
            binaries,
            backend: None,
            fetch_page: Arc::new(fetch),
        }
    }

    /// Substitui o yt-dlp por uma função, para testes.
    pub fn with_backend(mut self, backend: YtdlBackend) -> Self {
        self.backend = Some(backend);
        self
    }

    /// Substitui a leitura de páginas HTML, para testes sem rede.
    pub fn with_page_fetcher(mut self, fetcher: PageFetcher) -> Self {
        self.fetch_page = fetcher;
        self
    }

    /// Lê metadados do vídeo sem baixar.
    ///
    /// # Errors
    ///
    /// Se o vídeo não puder ser acessado.
    pub fn info(&self, url: &str) -> ToolResult<VideoInfo> {
        let data = self.extract(&validate_url(url)?, false, Vec::new())?;
        Ok(parse_info(&data))
    }

    /// Baixa o vídeo para `target_dir` e devolve o caminho do arquivo final.
    ///
    /// # Errors
    ///
    /// Se o download falhar.
    pub fn download(
        &self,
        url: &str,
        target_dir: &Path,
        quality: VideoQuality,
    ) -> ToolResult<PathBuf> {
        std::fs::create_dir_all(target_dir).map_err(|error| {
            ToolError::new(
                format!("Não foi possível criar {}: {error}.", target_dir.display()),
                ErrorCode::DownloadFailed,
            )
        })?;
        let outtmpl = target_dir.join(format!("%(title).{MAX_TITLE}B_%(id).{MAX_ID}B.%(ext)s"));
        let mut extra: Vec<OsString> = vec![
            "-f".into(),
            quality.format().into(),
            "-o".into(),
            outtmpl.into_os_string(),
            "--restrict-filenames".into(),
        ];
        if quality == VideoQuality::Audio {
            extra.extend(["-x".into(), "--audio-format".into(), "m4a".into()]);
        } else {
            extra.extend(["--merge-output-format".into(), "mp4".into()]);
        }
        let started = SystemTime::now();
        let data = self.extract(&validate_url(url)?, true, extra)?;
        final_path(&data, target_dir, started)
    }

    fn extract(&self, url: &str, download: bool, extra_args: Vec<OsString>) -> ToolResult<Value> {
        let request = |url: &str, generic: bool| YtdlRequest {
            url: url.to_string(),
            generic,
            download,
            extra_args: extra_args.clone(),
        };
        let failure = match self.run(&request(url, false)) {
            Ok(data) => return Ok(data),
            // DRM, login e vídeo removido não mudam de resultado com outra
            // tentativa. Falha do extractor, sim: vale seguir para as próximas.
            Err(error) if !worth_generic_retry(&error.message) => return Err(error),
            Err(error) => error,
        };
        // O genérico ignora quem "conhece" o site, lê o HTML e procura a fonte
        // do vídeo por conta própria.
        if let Ok(data) = self.run(&request(url, true)) {
            return Ok(data);
        }
        // Nem o genérico achou. Varre a página aqui e tenta cada candidato: é o
        // que resolve portais que escondem o arquivo dentro de um iframe.
        let mut last = failure;
        let timeout = self.timeout_seconds.min(PAGE_TIMEOUT);
        for candidate in self.discover_sources(url, timeout) {
            match self.run(&request(&candidate, false)) {
                Ok(data) => return Ok(data),
                // O vídeo foi encontrado e mesmo assim não veio. Esse motivo diz
                // mais ao agente do que o "Unsupported URL" da página.
                Err(error) => last = error,
            }
        }
        Err(last)
    }

    fn run(&self, request: &YtdlRequest) -> ToolResult<Value> {
        let outcome = match &self.backend {
            Some(backend) => backend(request),
            None => self.run_process(request)?,
        };
        outcome.map_err(|message| {
            let message = clean_error(&message);
            ToolError::with_hint(
                format!("yt-dlp falhou: {message}"),
                ErrorCode::DownloadFailed,
                hint_for(&message),
            )
        })
    }

    fn run_process(&self, request: &YtdlRequest) -> ToolResult<Result<Value, String>> {
        let ytdlp = self
            .binaries
            .ensure(ExternalTool::YtDlp, &self.ytdlp_bin)
            .map_err(|error| {
                if error.code == ErrorCode::Unavailable {
                    ToolError::with_hint(
                        "Download indisponível: yt-dlp não encontrado.",
                        ErrorCode::Unavailable,
                        "Coloque o executável yt-dlp ao lado do mcp-tools ou no PATH, aponte \
                     YTDLP_BIN para ele, ou deixe MCP_AUTO_DOWNLOAD=true para baixar sozinho.",
                    )
                } else {
                    error
                }
            })?;
        let mut args: Vec<OsString> = vec![
            "--quiet".into(),
            "--no-warnings".into(),
            "--no-progress".into(),
            "--socket-timeout".into(),
            format!("{}", self.timeout_seconds.min(60.0) as u64).into(),
            "--no-playlist".into(),
            "--dump-single-json".into(),
        ];
        // O yt-dlp exige um caminho real em --ffmpeg-location. Um nome nu como
        // "ffmpeg" é tratado como inexistente e o merge de vídeo+áudio falha.
        if let Some(ffmpeg) = self.binaries.locate(ExternalTool::Ffmpeg, &self.ffmpeg_bin) {
            args.push("--ffmpeg-location".into());
            args.push(ffmpeg.into_os_string());
        }
        if request.download {
            args.push("--no-simulate".into());
        } else {
            args.push("--skip-download".into());
        }
        if request.generic {
            args.push("--force-generic-extractor".into());
        }
        args.extend(request.extra_args.iter().cloned());
        args.push("--".into());
        args.push(request.url.clone().into());
        let timeout = Duration::from_secs_f64(self.timeout_seconds.max(1.0));
        let output = match process::run(&ytdlp, &args, timeout) {
            Ok(output) => output,
            Err(ProcessError::Timeout(_)) => {
                return Err(ToolError::with_hint(
                    format!("yt-dlp excedeu o timeout de {:.0}s.", self.timeout_seconds),
                    ErrorCode::Timeout,
                    "Use background=true ou aumente DOWNLOAD_TIMEOUT.",
                ))
            }
            Err(ProcessError::Spawn(error)) => {
                return Err(ToolError::with_hint(
                    format!("yt-dlp não pôde ser executado: {error}"),
                    ErrorCode::Unavailable,
                    "Confira YTDLP_BIN ou apague a pasta de cache para baixar de novo.",
                ))
            }
        };
        if !output.success() {
            let stderr = output.stderr.trim();
            let message = if stderr.is_empty() {
                "sem detalhes".to_string()
            } else {
                stderr.to_string()
            };
            return Ok(Err(message));
        }
        // Com --quiet só o JSON vai para stdout; mesmo assim pega a última linha
        // não vazia, que é onde o --dump-single-json escreve.
        let json_line = output
            .stdout
            .lines()
            .rev()
            .find(|line| line.trim_start().starts_with('{'));
        match json_line.and_then(|line| serde_json::from_str::<Value>(line).ok()) {
            Some(data) => Ok(Ok(data)),
            None => Err(ToolError::with_hint(
                "yt-dlp não devolveu informações do vídeo.",
                ErrorCode::DownloadFailed,
                DEFAULT_HINT,
            )),
        }
    }

    /// Procura na própria página as URLs reais do vídeo.
    ///
    /// Último recurso, quando nenhum extractor do yt-dlp reconhece o site. Junta as
    /// mídias diretas da página, os iframes e as mídias que estiverem dentro deles.
    /// Devolve candidatos sem repetição, do mais provável para o menos provável.
    pub fn discover_sources(&self, url: &str, timeout: f64) -> Vec<String> {
        let (media, frames) = self.scan(url, timeout);
        let mut inner = Vec::new();
        for frame in frames.iter().take(MAX_FRAMES) {
            let (found, _) = self.scan(frame, timeout);
            inner.extend(found);
        }
        let mut all = media;
        all.extend(frames);
        all.extend(inner);
        unique(all).into_iter().take(MAX_CANDIDATES).collect()
    }

    /// Devolve as mídias diretas e os iframes de uma página, como URLs absolutas.
    pub fn scan(&self, url: &str, timeout: f64) -> (Vec<String>, Vec<String>) {
        let Some(page) = (self.fetch_page)(url, timeout) else {
            return (Vec::new(), Vec::new());
        };
        let (media_raw, frames_raw) = find_sources(&page);
        let mut media: Vec<String> = media_raw.iter().map(|item| urljoin(url, item)).collect();
        // Player montado por JavaScript deixa a URL solta no meio do script, fora de
        // qualquer tag: a tag sozinha não basta.
        let unescaped = unescape_html(&page);
        media.extend(
            media_url_regex()
                .find_iter(&unescaped)
                .map(|m| m.as_str().to_string()),
        );
        let frames: Vec<String> = frames_raw.iter().map(|item| urljoin(url, item)).collect();
        (http_only(media), http_only(frames))
    }
}

/// Garante que a URL é um endereço http(s) que dá para buscar.
///
/// Qualquer site é aceito. A checagem existe só para barrar esquemas que não
/// são download da web: `file://` faria o yt-dlp ler o disco fora do
/// workspace, o que quebraria a fronteira do projeto.
///
/// # Errors
///
/// [`ErrorCode::InvalidArgument`] se a URL não for http(s).
pub fn validate_url(url: &str) -> ToolResult<String> {
    let cleaned = url.trim();
    if !is_http(cleaned) {
        return Err(ToolError::with_hint(
            format!("'{url}' não é uma URL http(s) válida."),
            ErrorCode::InvalidArgument,
            "Informe o endereço completo da página do vídeo, começando com https://.",
        ));
    }
    Ok(cleaned.to_string())
}

fn is_http(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let rest = if let Some(rest) = lower.strip_prefix("https://") {
        rest
    } else if let Some(rest) = lower.strip_prefix("http://") {
        rest
    } else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.is_empty()
}

/// Converte o título do vídeo em um nome de arquivo seguro.
pub fn safe_title(title: &str) -> String {
    let mut cleaned = String::with_capacity(title.len());
    let mut pending = false;
    for ch in title.chars() {
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
    let cleaned: String = cleaned.trim_matches('_').chars().take(MAX_TITLE).collect();
    let cleaned = cleaned.trim_matches('_');
    if cleaned.is_empty() {
        "video".to_string()
    } else {
        cleaned.to_string()
    }
}

/// `true` quando vale tentar o extractor genérico depois desta falha.
pub fn worth_generic_retry(message: &str) -> bool {
    !failure_patterns()
        .iter()
        .take(NO_RETRY)
        .any(|(pattern, _)| pattern.is_match(message))
}

/// Hint para o agente a partir da mensagem de erro do yt-dlp.
pub fn hint_for(message: &str) -> String {
    for (pattern, hint) in failure_patterns() {
        if pattern.is_match(message) {
            return (*hint).to_string();
        }
    }
    DEFAULT_HINT.to_string()
}

/// Limpa o prefixo `ERROR:` e limita a mensagem aos últimos 500 caracteres.
pub fn clean_error(raw: &str) -> String {
    let cleaned = raw.replace("ERROR: ", "");
    let cleaned = cleaned.trim();
    let count = cleaned.chars().count();
    cleaned.chars().skip(count.saturating_sub(500)).collect()
}

/// Coleta, de uma página HTML, o `src` das mídias diretas e dos iframes.
fn find_sources(page: &str) -> (Vec<String>, Vec<String>) {
    let mut media = Vec::new();
    let mut frames = Vec::new();
    for captures in tag_regex().captures_iter(page) {
        let tag = captures
            .get(1)
            .map_or("", |m| m.as_str())
            .to_ascii_lowercase();
        let attrs = captures.get(2).map_or("", |m| m.as_str());
        let mut src = None;
        let mut data_src = None;
        for attr in attr_regex().captures_iter(attrs) {
            let name = attr.get(1).map_or("", |m| m.as_str()).to_ascii_lowercase();
            let value = attr
                .get(2)
                .or_else(|| attr.get(3))
                .or_else(|| attr.get(4))
                .map(|m| unescape_html(m.as_str()))
                .filter(|v| !v.is_empty());
            match name.as_str() {
                "src" => src = src.or(value),
                "data-src" => data_src = data_src.or(value),
                _ => {}
            }
        }
        let Some(source) = src.or(data_src) else {
            continue;
        };
        if matches!(tag.as_str(), "video" | "source" | "audio") {
            media.push(source);
        } else {
            frames.push(source);
        }
    }
    (media, frames)
}

/// Baixa o HTML de uma página. Devolve `None` quando não dá para ler.
fn fetch(url: &str, timeout: f64) -> Option<String> {
    if !is_http(url) {
        return None;
    }
    let agent = crate::core::http::agent(Duration::from_secs_f64(timeout.max(1.0)), USER_AGENT);
    let response = agent.get(url).call().ok()?;
    let mut raw = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(MAX_PAGE_BYTES)
        .read_to_end(&mut raw)
        .ok()?;
    Some(String::from_utf8_lossy(&raw).into_owned())
}

/// Descarta candidatos que não sejam http(s), como `javascript:` e `data:`.
pub fn http_only(urls: Vec<String>) -> Vec<String> {
    urls.into_iter().filter(|item| is_http(item)).collect()
}

/// Remove repetidos preservando a ordem.
fn unique(urls: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    urls.into_iter()
        .filter(|item| seen.insert(item.clone()))
        .collect()
}

/// Resolve `reference` contra `base`, como `urllib.parse.urljoin`.
pub fn urljoin(base: &str, reference: &str) -> String {
    let reference = reference.trim();
    if reference.is_empty() {
        return base.to_string();
    }
    if reference.contains("://")
        || reference.starts_with("data:")
        || reference.starts_with("javascript:")
    {
        return reference.to_string();
    }
    let (scheme, rest) = match base.split_once("://") {
        Some(parts) => parts,
        None => return reference.to_string(),
    };
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    if let Some(stripped) = reference.strip_prefix("//") {
        return format!("{scheme}://{stripped}");
    }
    if reference.starts_with('/') {
        return format!("{scheme}://{authority}{reference}");
    }
    if reference.starts_with('?') || reference.starts_with('#') {
        let clean = path.split(['?', '#']).next().unwrap_or("/");
        return format!("{scheme}://{authority}{clean}{reference}");
    }
    let clean = path.split(['?', '#']).next().unwrap_or("/");
    let dir = match clean.rfind('/') {
        Some(index) => &clean[..=index],
        None => "/",
    };
    let mut segments: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for part in reference.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    let trailing =
        reference.ends_with('/') || reference.ends_with("/.") || reference.ends_with("/..");
    let mut joined = format!("{scheme}://{authority}/{}", segments.join("/"));
    if trailing && !joined.ends_with('/') {
        joined.push('/');
    }
    joined
}

/// Desfaz as entidades HTML mais comuns (`&amp;`, `&#39;`, `&#x2F;`...).
pub fn unescape_html(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        rest = &rest[index..];
        let Some(end) = rest.find(';').filter(|end| *end <= 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => entity
                .strip_prefix('#')
                .and_then(|num| {
                    if let Some(hex) = num.strip_prefix(['x', 'X']) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        num.parse().ok()
                    }
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(ch) => {
                out.push(ch);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Descobre o arquivo que o yt-dlp acabou de gravar.
///
/// * `data`: JSON devolvido pelo yt-dlp.
/// * `target_dir`: pasta de destino do download.
/// * `started`: instante em que o download começou, para reconhecer o que é novo.
///
/// # Errors
///
/// [`ErrorCode::DownloadFailed`] se nenhum arquivo novo for encontrado.
pub fn final_path(data: &Value, target_dir: &Path, started: SystemTime) -> ToolResult<PathBuf> {
    if let Some(reported) = reported_path(data) {
        return Ok(reported);
    }
    // O extractor genérico nem sempre preenche requested_downloads, e o nome do
    // arquivo já passou pelo saneamento do yt-dlp. Sobra olhar o que apareceu na
    // pasta durante este download.
    let threshold = started
        .checked_sub(Duration::from_secs_f64(MTIME_SLACK))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut fresh: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(target_dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_file())
                .filter(|path| path.extension().and_then(|e| e.to_str()) != Some("part"))
                .filter_map(|path| {
                    let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
                    (modified >= threshold).then_some((modified, path))
                })
                .collect()
        })
        .unwrap_or_default();
    fresh.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some((_, path)) = fresh.pop() {
        return Ok(path);
    }
    Err(ToolError::with_hint(
        "Download terminou mas o arquivo final não foi encontrado.",
        ErrorCode::DownloadFailed,
        "Use list_files na pasta de destino para procurar o arquivo baixado.",
    ))
}

fn reported_path(data: &Value) -> Option<PathBuf> {
    if let Some(downloads) = data.get("requested_downloads").and_then(Value::as_array) {
        for item in downloads {
            if let Some(filepath) = item.get("filepath").and_then(Value::as_str) {
                if !filepath.is_empty() {
                    return Some(PathBuf::from(filepath));
                }
            }
        }
    }
    data.get("filepath")
        .and_then(Value::as_str)
        .filter(|f| !f.is_empty())
        .map(PathBuf::from)
}

/// Converte o JSON bruto do yt-dlp em [`VideoInfo`].
pub fn parse_info(data: &Value) -> VideoInfo {
    let chapters = data
        .get("chapters")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.is_object())
                .map(|item| Chapter {
                    title: non_empty(item.get("title")).unwrap_or_default(),
                    start: num(item.get("start_time")),
                    end: num(item.get("end_time")),
                })
                .collect()
        })
        .unwrap_or_default();
    VideoInfo {
        id: non_empty(data.get("id")).unwrap_or_default(),
        title: non_empty(data.get("title")).unwrap_or_else(|| "video".to_string()),
        extractor: non_empty(data.get("extractor_key"))
            .or_else(|| non_empty(data.get("extractor")))
            .unwrap_or_else(|| "Generic".to_string()),
        channel: non_empty(data.get("channel")).or_else(|| non_empty(data.get("uploader"))),
        duration: num(data.get("duration")),
        view_count: data.get("view_count").and_then(Value::as_i64),
        upload_date: non_empty(data.get("upload_date")),
        description: non_empty(data.get("description")).unwrap_or_default(),
        thumbnail: non_empty(data.get("thumbnail")),
        chapters,
    }
}

fn num(value: Option<&Value>) -> f64 {
    value.and_then(Value::as_f64).unwrap_or(0.0)
}

fn non_empty(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
