//! Testes de `core/downloader.rs`: validação de URL, hints, varredura de página
//! e a cadeia de tentativas (nativo, genérico, varredura) com um yt-dlp de mentira.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use mcp_tools::core::downloader::{
    final_path, hint_for, http_only, parse_info, safe_title, validate_url, worth_generic_retry,
    Downloader, YtdlRequest,
};
use mcp_tools::core::errors::ErrorCode;
use serde_json::{json, Value};

/// Resposta do yt-dlp de mentira: `Err(mensagem)` vira `DownloadError`, `Ok(json)` é sucesso.
type Reply = Result<Value, String>;

/// yt-dlp de mentira. Responde diferente conforme o extractor genérico for forçado.
struct FakeYtdl {
    native: Reply,
    generic: Reply,
    por_url: HashMap<String, Reply>,
    /// Flag `generic` de cada chamada, na ordem.
    calls: Arc<Mutex<Vec<bool>>>,
    /// URL de cada chamada, na ordem.
    urls: Arc<Mutex<Vec<String>>>,
}

impl FakeYtdl {
    fn new(native: Reply, generic: Reply) -> Self {
        Self {
            native,
            generic,
            por_url: HashMap::new(),
            calls: Arc::new(Mutex::new(Vec::new())),
            urls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn por_url(mut self, url: &str, reply: Reply) -> Self {
        self.por_url.insert(url.to_string(), reply);
        self
    }

    fn calls(&self) -> Vec<bool> {
        self.calls.lock().unwrap().clone()
    }

    fn urls(&self) -> Vec<String> {
        self.urls.lock().unwrap().clone()
    }

    /// Downloader sem rede (páginas em `paginas`) com este yt-dlp instalado.
    fn install(&self, paginas: Paginas) -> Downloader {
        let native = self.native.clone();
        let generic = self.generic.clone();
        let por_url = self.por_url.clone();
        let calls = Arc::clone(&self.calls);
        let urls = Arc::clone(&self.urls);
        Downloader::default()
            .with_backend(Arc::new(move |request: &YtdlRequest| {
                calls.lock().unwrap().push(request.generic);
                urls.lock().unwrap().push(request.url.clone());
                por_url.get(&request.url).cloned().unwrap_or_else(|| {
                    if request.generic {
                        generic.clone()
                    } else {
                        native.clone()
                    }
                })
            }))
            .with_page_fetcher(paginas.fetcher())
    }
}

fn ok(data: Value) -> Reply {
    Ok(data)
}

fn erro(message: &str) -> Reply {
    Err(message.to_string())
}

/// Serve HTML fixo no lugar da rede e registra as URLs buscadas.
#[derive(Clone, Default)]
struct Paginas {
    mapa: HashMap<String, String>,
    buscadas: Arc<Mutex<Vec<String>>>,
}

impl Paginas {
    fn new(mapa: &[(&str, String)]) -> Self {
        Self {
            mapa: mapa
                .iter()
                .map(|(url, html)| ((*url).to_string(), html.clone()))
                .collect(),
            buscadas: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn fetcher(&self) -> Arc<dyn Fn(&str, f64) -> Option<String> + Send + Sync> {
        let mapa = self.mapa.clone();
        let buscadas = Arc::clone(&self.buscadas);
        Arc::new(move |url: &str, _timeout: f64| {
            buscadas.lock().unwrap().push(url.to_string());
            mapa.get(url).cloned()
        })
    }

    fn buscadas(&self) -> Vec<String> {
        self.buscadas.lock().unwrap().clone()
    }

    /// Downloader sem yt-dlp de mentira: só a leitura de páginas é substituída.
    fn downloader(&self) -> Downloader {
        Downloader::default().with_page_fetcher(self.fetcher())
    }
}

#[test]
fn test_validate_url_accepts_any_http_source() {
    for url in [
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        "https://youtu.be/dQw4w9WgXcQ",
        "  https://m.youtube.com/watch?v=abc  ",
        "https://eaulas.usp.br/portal/video?idItem=2345",
        "https://vimeo.com/123",
        "http://exemplo.com.br/aula/parte-1",
        "https://cdn.exemplo.com/videos/aula.mp4",
        "https://stream.exemplo.com/live/playlist.m3u8",
        "http://localhost:8080/video",
    ] {
        assert_eq!(validate_url(url).unwrap(), url.trim(), "{url}");
    }
}

#[test]
fn test_validate_url_rejects_non_http() {
    for url in [
        "ftp://exemplo.com/x",
        "file:///etc/passwd",
        "abc",
        "",
        "https://",
        "www.exemplo.com/x",
    ] {
        let err = validate_url(url).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgument, "{url}");
    }
}

#[test]
fn test_safe_title() {
    assert_eq!(
        safe_title("Olá, mundo! (2024) / teste"),
        "Ol_mundo_2024_teste"
    );
    assert_eq!(safe_title("///"), "video");
    assert_eq!(safe_title(&"a".repeat(200)).len(), 60);
}

#[test]
fn test_parse_info_full() {
    let info = parse_info(&json!({
        "id": "abc",
        "title": "Título",
        "extractor_key": "Youtube",
        "channel": "Canal",
        "duration": 120,
        "view_count": 10,
        "upload_date": "20240101",
        "description": "desc",
        "thumbnail": "http://x/y.jpg",
        "chapters": [{"title": "Intro", "start_time": 0, "end_time": 10}, "lixo"],
    }));
    assert_eq!(info.title, "Título");
    assert_eq!(info.extractor, "Youtube");
    assert!((info.duration - 120.0).abs() < f64::EPSILON);
    assert_eq!(info.chapters.len(), 1);
    assert_eq!(info.chapters[0].title, "Intro");
    assert!((info.chapters[0].start - 0.0).abs() < f64::EPSILON);
    assert!((info.chapters[0].end - 10.0).abs() < f64::EPSILON);
}

#[test]
fn test_parse_info_missing_fields() {
    let info = parse_info(&json!({}));
    assert_eq!(info.title, "video");
    assert_eq!(info.extractor, "Generic");
    assert_eq!(info.channel, None);
    assert!((info.duration - 0.0).abs() < f64::EPSILON);
    assert!(info.chapters.is_empty());
}

#[test]
fn test_hint_for_distingue_causas() {
    for (message, trecho) in [
        ("This video is DRM protected", "DRM"),
        (
            "Private video. Sign in if you've been granted access",
            "login",
        ),
        (
            "Video unavailable. This video has been removed",
            "não existe mais",
        ),
        (
            "Unsupported URL: https://exemplo.com/pagina",
            "Nenhum vídeo foi encontrado",
        ),
        ("Connection reset by peer", "abre no navegador"),
    ] {
        assert!(hint_for(message).contains(trecho), "{message}");
    }
}

#[test]
fn test_worth_generic_retry() {
    for (message, esperado) in [
        ("Unsupported URL: https://exemplo.com", true),
        ("Unable to extract player data", true),
        ("Connection reset by peer", true),
        ("This video is DRM protected", false),
        ("Please sign in to continue", false),
        ("Video unavailable, it was removed", false),
    ] {
        assert_eq!(worth_generic_retry(message), esperado, "{message}");
    }
}

#[test]
fn test_info_usa_extractor_generico_quando_o_nativo_falha() {
    let fake = FakeYtdl::new(
        erro("Unsupported URL: https://eaulas.usp.br/portal/video?idItem=2345"),
        ok(json!({"id": "2345", "title": "Aula de matemática", "extractor_key": "Generic"})),
    );
    let downloader = fake.install(Paginas::default());
    let info = downloader
        .info("https://eaulas.usp.br/portal/video?idItem=2345")
        .unwrap();
    assert_eq!(info.title, "Aula de matemática");
    assert_eq!(info.extractor, "Generic");
    assert_eq!(fake.calls(), vec![false, true]);
}

#[test]
fn test_info_nao_repete_quando_exige_login() {
    let fake = FakeYtdl::new(
        erro("Private video. Sign in to continue"),
        ok(json!({"id": "x"})),
    );
    let downloader = fake.install(Paginas::default());
    let err = downloader.info("https://exemplo.com/aula").unwrap_err();
    assert_eq!(err.code, ErrorCode::DownloadFailed);
    assert!(err.hint.as_deref().unwrap().contains("login"));
    assert_eq!(fake.calls(), vec![false]);
}

#[test]
fn test_info_devolve_o_erro_do_nativo_quando_os_dois_falham() {
    let fake = FakeYtdl::new(
        erro("Unable to extract player data"),
        erro("Unsupported URL"),
    );
    let downloader = fake.install(Paginas::default());
    let err = downloader.info("https://exemplo.com/aula").unwrap_err();
    assert!(err.message.contains("Unable to extract player data"));
    assert_eq!(fake.calls(), vec![false, true]);
}

#[test]
fn test_info_sem_segunda_tentativa_quando_o_nativo_funciona() {
    let fake = FakeYtdl::new(
        ok(json!({"id": "abc", "title": "Ok"})),
        erro("nunca chamado"),
    );
    let downloader = fake.install(Paginas::default());
    assert_eq!(downloader.info("https://youtu.be/abc").unwrap().title, "Ok");
    assert_eq!(fake.calls(), vec![false]);
}

#[test]
fn test_final_path_usa_requested_downloads() {
    let tmp = tempfile::tempdir().unwrap();
    let alvo = tmp.path().join("video.mp4");
    std::fs::write(&alvo, b"").unwrap();
    let data = json!({"requested_downloads": [{"filepath": alvo.to_string_lossy()}]});
    assert_eq!(
        final_path(&data, tmp.path(), SystemTime::now()).unwrap(),
        alvo
    );
}

#[test]
fn test_final_path_usa_filepath_de_topo() {
    let tmp = tempfile::tempdir().unwrap();
    let alvo = tmp.path().join("video.mkv");
    std::fs::write(&alvo, b"").unwrap();
    let data = json!({"filepath": alvo.to_string_lossy()});
    assert_eq!(
        final_path(&data, tmp.path(), SystemTime::now()).unwrap(),
        alvo
    );
}

#[test]
fn test_final_path_pega_o_arquivo_novo_quando_o_extractor_nao_informa() {
    let tmp = tempfile::tempdir().unwrap();
    let antigo = tmp.path().join("de_outro_download.mp4");
    std::fs::write(&antigo, b"").unwrap();
    let old = std::fs::File::options().write(true).open(&antigo).unwrap();
    old.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1000))
        .unwrap();
    drop(old);
    let started = SystemTime::now();
    let alvo = tmp.path().join("Aula_de_matematica.mkv");
    std::fs::write(&alvo, b"").unwrap();
    assert_eq!(final_path(&json!({}), tmp.path(), started).unwrap(), alvo);
}

#[test]
fn test_final_path_ignora_arquivo_parcial() {
    let tmp = tempfile::tempdir().unwrap();
    let started = SystemTime::now();
    std::fs::write(tmp.path().join("video.mp4.part"), b"").unwrap();
    let err = final_path(&json!({}), tmp.path(), started).unwrap_err();
    assert_eq!(err.code, ErrorCode::DownloadFailed);
}

#[test]
fn test_final_path_sem_arquivo() {
    let tmp = tempfile::tempdir().unwrap();
    let err = final_path(&json!({"id": "2345"}), tmp.path(), SystemTime::now()).unwrap_err();
    assert_eq!(err.code, ErrorCode::DownloadFailed);
}

const PAGINA: &str = "https://eaulas.usp.br/portal/video?idItem=2345";
const EMBED: &str = "https://eaulas.usp.br/portal/embed-video?idItem=2345";
const MP4: &str = "https://cdn.eaulas.usp.br/route/2345/1350914719965.mp4?s=abc&v=0";

#[test]
fn test_scan_acha_video_source_e_iframe() {
    let paginas = Paginas::new(&[(
        PAGINA,
        format!(
            "<video src=\"/media/aula.mp4\"></video>\
             <source src=\"https://cdn.x/y.webm\">\
             <iframe src=\"{EMBED}\"></iframe>"
        ),
    )]);
    let (media, frames) = paginas.downloader().scan(PAGINA, 5.0);
    assert!(media.contains(&"https://eaulas.usp.br/media/aula.mp4".to_string()));
    assert!(media.contains(&"https://cdn.x/y.webm".to_string()));
    assert_eq!(frames, vec![EMBED.to_string()]);
}

#[test]
fn test_scan_acha_url_solta_no_javascript() {
    let paginas = Paginas::new(&[(
        PAGINA,
        format!("<script>var p = {{ src: '{MP4}' }};</script>"),
    )]);
    let (media, frames) = paginas.downloader().scan(PAGINA, 5.0);
    assert_eq!(media, vec![MP4.to_string()]);
    assert!(frames.is_empty());
}

#[test]
fn test_scan_descarta_esquemas_que_nao_sao_http() {
    let paginas = Paginas::new(&[(
        PAGINA,
        "<iframe src=\"javascript:void(0)\"></iframe><video src=\"data:video/mp4;base64,AA\">"
            .to_string(),
    )]);
    let (media, frames) = paginas.downloader().scan(PAGINA, 5.0);
    assert!(media.is_empty());
    assert!(frames.is_empty());
}

#[test]
fn test_scan_pagina_inacessivel() {
    let paginas = Paginas::default();
    let (media, frames) = paginas.downloader().scan(PAGINA, 5.0);
    assert!(media.is_empty());
    assert!(frames.is_empty());
}

#[test]
fn test_discover_sources_desce_um_nivel_no_iframe() {
    let paginas = Paginas::new(&[
        (PAGINA, format!("<iframe src=\"{EMBED}\"></iframe>")),
        (EMBED, format!("<script>src: '{MP4}'</script>")),
    ]);
    // O iframe vem antes da mídia de dentro dele: pode ser um embed conhecido.
    assert_eq!(
        paginas.downloader().discover_sources(PAGINA, 5.0),
        vec![EMBED.to_string(), MP4.to_string()]
    );
    assert_eq!(
        paginas.buscadas(),
        vec![PAGINA.to_string(), EMBED.to_string()]
    );
}

#[test]
fn test_discover_sources_sem_repetidos() {
    let paginas = Paginas::new(&[(
        PAGINA,
        format!("<video src=\"{MP4}\"></video><script>\"{MP4}\"</script>"),
    )]);
    assert_eq!(
        paginas.downloader().discover_sources(PAGINA, 5.0),
        vec![MP4.to_string()]
    );
}

#[test]
fn test_http_only_filtra() {
    let entrada: Vec<String> = [
        "https://a/b",
        "http://c/d",
        "data:x",
        "javascript:void(0)",
        "//x/y",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    assert_eq!(
        http_only(entrada),
        vec!["https://a/b".to_string(), "http://c/d".to_string()]
    );
}

#[test]
fn test_info_varre_a_pagina_quando_nenhum_extractor_reconhece() {
    let fake = FakeYtdl::new(erro("Unsupported URL"), erro("Unsupported URL")).por_url(
        MP4,
        ok(json!({"id": "x", "title": "Aula", "extractor_key": "Generic"})),
    );
    let paginas = Paginas::new(&[(PAGINA, format!("<video src=\"{MP4}\"></video>"))]);
    let downloader = fake.install(paginas);
    assert_eq!(downloader.info(PAGINA).unwrap().title, "Aula");
    assert_eq!(
        fake.urls(),
        vec![PAGINA.to_string(), PAGINA.to_string(), MP4.to_string()]
    );
}

#[test]
fn test_info_erro_do_candidato_ganha_do_erro_da_pagina() {
    let fake = FakeYtdl::new(erro("Unsupported URL"), erro("Unsupported URL"))
        .por_url(MP4, erro("HTTP Error 403"));
    let paginas = Paginas::new(&[(PAGINA, format!("<video src=\"{MP4}\"></video>"))]);
    let downloader = fake.install(paginas);
    let err = downloader.info(PAGINA).unwrap_err();
    assert!(err.message.contains("403"));
}

#[test]
fn test_info_erro_da_pagina_quando_a_varredura_nao_acha_nada() {
    let fake = FakeYtdl::new(erro("Unsupported URL"), erro("Unsupported URL"));
    let paginas = Paginas::new(&[(PAGINA, "<html><body>sem vídeo</body></html>".to_string())]);
    let downloader = fake.install(paginas);
    let err = downloader.info(PAGINA).unwrap_err();
    assert!(err.message.contains("Unsupported URL"));
    assert!(err
        .hint
        .as_deref()
        .unwrap()
        .contains("Nenhum vídeo foi encontrado"));
}
