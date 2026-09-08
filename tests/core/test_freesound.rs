//! Testes de `core/freesound.rs`: parse da API, nomes seguros, montagem da URL
//! de busca e download do preview com o transporte substituído.

use std::sync::{Arc, Mutex};

use mcp_tools::core::errors::{ErrorCode, ToolError};
use mcp_tools::core::freesound::{
    http_error, parse_search, parse_sound, safe_name, short_license, Freesound, SoundCandidate,
};
use serde_json::{json, Value};

fn raw_sound() -> Value {
    json!({
        "id": 785925,
        "name": "Drama Boom",
        "tags": ["boom", "comedy", 42],
        "license": "http://creativecommons.org/publicdomain/zero/1.0/",
        "duration": 4.42308,
        "username": "modusmogulus",
        "previews": {
            "preview-hq-mp3": "https://cdn.freesound.org/previews/785/785925_15956618-hq.mp3",
            "preview-lq-mp3": "https://cdn.freesound.org/previews/785/785925_15956618-lq.mp3",
        },
    })
}

#[test]
fn test_parse_sound() {
    let candidate = parse_sound(&raw_sound()).unwrap();
    assert_eq!(candidate.sound_id, 785925);
    assert_eq!(candidate.name, "Drama Boom");
    assert_eq!(candidate.license, "CC0");
    assert_eq!(candidate.author, "modusmogulus");
    assert_eq!(
        candidate.tags,
        vec!["boom".to_string(), "comedy".to_string()]
    );
    assert!(candidate.preview_url.ends_with("-hq.mp3"));
    assert!((candidate.duration - 4.42308).abs() < 1e-6);
}

#[test]
fn test_parse_sound_sem_preview() {
    assert!(parse_sound(&json!({"id": 1, "previews": {}})).is_none());
    assert!(
        parse_sound(&json!({"id": "x", "previews": {"preview-hq-mp3": "https://a/b.mp3"}}))
            .is_none()
    );
}

#[test]
fn test_parse_search_ignora_lixo() {
    let data = json!({"results": [raw_sound(), "lixo", {"id": 2, "previews": {}}]});
    let ids: Vec<i64> = parse_search(&data).iter().map(|c| c.sound_id).collect();
    assert_eq!(ids, vec![785925]);
    assert!(parse_search(&json!({"results": null})).is_empty());
}

#[test]
fn test_short_license() {
    assert_eq!(
        short_license("http://creativecommons.org/licenses/by/4.0/"),
        "CC BY"
    );
    assert_eq!(
        short_license("http://creativecommons.org/licenses/by-nc/4.0/"),
        "CC BY-NC"
    );
    assert_eq!(short_license(""), "desconhecida");
}

#[test]
fn test_safe_name() {
    assert_eq!(safe_name("Vine Boom (loud!) v2.wav"), "Vine_Boom_loud_v2");
    assert_eq!(safe_name("Soft Bell - LowDing.mp3"), "Soft_Bell_-_LowDing");
    assert_eq!(safe_name("///"), "sound");
}

#[test]
fn test_search_monta_url() {
    let urls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&urls);
    let client = Freesound::with_fetcher(
        "abc",
        Arc::new(move |url: &str| {
            seen.lock().unwrap().push(url.to_string());
            Ok(serde_json::to_vec(&json!({"results": [raw_sound()]})).unwrap())
        }),
    );
    let found = client.search("vine boom", 3, Some(8.0)).unwrap();
    assert_eq!(found[0].sound_id, 785925);
    let urls = urls.lock().unwrap();
    assert_eq!(urls.len(), 1);
    assert!(urls[0].starts_with("https://freesound.org/apiv2/search/text/?"));
    assert!(urls[0].contains("query=vine+boom"));
    assert!(urls[0].contains("page_size=3"));
    assert!(urls[0].contains("token=abc"));
    assert!(urls[0].contains("filter=duration%3A%5B0+TO+8%5D"));
}

#[test]
fn test_search_argumentos_invalidos() {
    let client = Freesound::new("abc");
    let err = client.search("   ", 5, Some(10.0)).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    assert!(client.search("boom", 0, Some(10.0)).is_err());
    assert!(client.search("boom", 5, Some(0.0)).is_err());
}

#[test]
fn test_sem_chave() {
    let err = Freesound::new("  ")
        .search("boom", 5, Some(10.0))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable);
}

#[test]
fn test_sound_id_invalido() {
    let err = Freesound::new("abc").sound(0).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_download_preview_grava_e_reaproveita() {
    let tmp = tempfile::tempdir().unwrap();
    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&calls);
    let client = Freesound::with_fetcher(
        "abc",
        Arc::new(move |url: &str| {
            seen.lock().unwrap().push(url.to_string());
            Ok(b"mp3-fake".to_vec())
        }),
    );
    let candidate = parse_sound(&raw_sound()).unwrap();
    let first = client
        .download_preview(&candidate, &tmp.path().join("sfx"))
        .unwrap();
    assert_eq!(first, tmp.path().join("sfx").join("Drama_Boom_785925.mp3"));
    assert_eq!(std::fs::read(&first).unwrap(), b"mp3-fake");
    let second = client
        .download_preview(&candidate, &tmp.path().join("sfx"))
        .unwrap();
    assert_eq!(second, first);
    assert_eq!(*calls.lock().unwrap(), vec![candidate.preview_url.clone()]);
}

#[test]
fn test_download_preview_vazio() {
    let tmp = tempfile::tempdir().unwrap();
    let client = Freesound::with_fetcher("abc", Arc::new(|_url: &str| Ok(Vec::new())));
    let candidate = SoundCandidate {
        sound_id: 1,
        name: "x".to_string(),
        duration: 1.0,
        license: "CC0".to_string(),
        author: "a".to_string(),
        tags: Vec::new(),
        preview_url: "https://cdn.freesound.org/x.mp3".to_string(),
    };
    let err = client.download_preview(&candidate, tmp.path()).unwrap_err();
    assert_eq!(err.code, ErrorCode::DownloadFailed);
}

#[test]
fn test_rejeita_url_sem_https() {
    let err = Freesound::new("abc")
        .get_bytes("file:///etc/passwd")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::DownloadFailed);
}

#[test]
fn test_erros_http() {
    for (status, code) in [
        (401, ErrorCode::Unavailable),
        (404, ErrorCode::NotFound),
        (429, ErrorCode::DownloadFailed),
        (500, ErrorCode::DownloadFailed),
    ] {
        assert_eq!(http_error(status).code, code, "status {status}");
        let client = Freesound::with_fetcher(
            "abc",
            Arc::new(move |_url: &str| -> Result<Vec<u8>, ToolError> { Err(http_error(status)) }),
        );
        let err = client.sound(123).unwrap_err();
        assert_eq!(err.code, code, "status {status}");
    }
}
