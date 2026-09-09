//! Garante que o servidor sobe com todos os domínios e expõe as mesmas tools da
//! versão Python, com os mesmos nomes.

use std::collections::HashMap;

use mcp_tools::config::Settings;
use mcp_tools::server::build_server;

const EXPECTED_TOOLS: &[&str] = &[
    "add_background_music",
    "add_banner",
    "add_fade",
    "add_narration",
    "add_sound_effects",
    "add_text_overlay",
    "apply_template",
    "burn_subtitles",
    "change_speed",
    "concat_videos",
    "create_dynamic_subtitles",
    "create_subtitles",
    "create_thumbnail",
    "cut_video",
    "delete_file",
    "detect_scenes",
    "download_sound_effect",
    "download_video",
    "export_for_platform",
    "extract_audio",
    "extract_frame",
    "get_video_info",
    "job_result",
    "job_status",
    "list_files",
    "list_templates",
    "normalize_audio",
    "probe_video",
    "remove_silence",
    "search_sound_effects",
    "set_video_metadata",
    "smart_crop",
    "stabilize_video",
    "transcribe_audio",
    "zoom_video",
];

fn settings(domains: &str) -> Settings {
    let dir = tempfile::tempdir().expect("tmp");
    let mut env = HashMap::new();
    env.insert(
        "WORKSPACE_DIR".to_string(),
        dir.path().join("ws").to_string_lossy().into_owned(),
    );
    env.insert("MCP_DOMAINS".to_string(), domains.to_string());
    env.insert("MCP_AUTO_DOWNLOAD".to_string(), "false".to_string());
    std::mem::forget(dir);
    Settings::from_map(&env).expect("settings")
}

#[test]
fn test_all_tools_registered() {
    let server = build_server(settings(""), None).expect("servidor");
    assert_eq!(server.tool_names(), EXPECTED_TOOLS);
}

#[test]
fn test_domains_filter() {
    let server = build_server(settings("files,jobs"), None).expect("servidor");
    assert_eq!(
        server.tool_names(),
        ["delete_file", "job_result", "job_status", "list_files"]
    );
}

#[test]
fn test_every_tool_has_description_and_schema() {
    let server = build_server(settings(""), None).expect("servidor");
    for tool in server.tools() {
        let description = tool.description.as_deref().unwrap_or("");
        assert!(
            !description.trim().is_empty(),
            "{} sem description",
            tool.name
        );
        assert_eq!(
            tool.input_schema.get("type").and_then(|v| v.as_str()),
            Some("object"),
            "{} sem schema de objeto",
            tool.name
        );
    }
}
