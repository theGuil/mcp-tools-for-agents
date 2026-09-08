use std::process::Command;
use std::time::Duration;

use mcp_tools::core::errors::ErrorCode;
use mcp_tools::core::jobs::JobStatus;
use mcp_tools::core::paths::Workspace;
use mcp_tools::domains::video::probe::probe_video;
use mcp_tools::domains::video::template::{
    apply_template, list_templates, TemplateName, TEMPLATES,
};

use crate::conftest::{runtime, sample_video};
use crate::helpers::{unwrap, unwrap_job};
use crate::skip_without_ffmpeg;

/// Gera uma imagem vermelha 200x80 (`logo.png`) no workspace.
fn logo(workspace: &Workspace) -> String {
    let target = workspace.root.join("logo.png");
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:size=200x80:duration=0.1",
            "-frames:v",
            "1",
        ])
        .arg(&target)
        .status()
        .expect("ffmpeg");
    assert!(status.success());
    "logo.png".to_string()
}

#[test]
fn test_list_templates() {
    let rt = runtime();
    let result = list_templates(&rt.runtime).unwrap();
    let names: Vec<TemplateName> = result.templates.iter().map(|t| t.name).collect();
    let expected: Vec<TemplateName> = TEMPLATES.iter().map(|t| t.name).collect();
    assert_eq!(names, expected);
}

#[test]
fn test_shorts_with_title() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(apply_template(
        &rt.runtime,
        &sample,
        TemplateName::Shorts,
        Some("Meu Short"),
        None,
        false,
    ));
    assert_eq!(result.output, "sample_shorts.mp4");
    let probed = probe_video(&rt.runtime, &result.output).unwrap();
    assert_eq!(
        (probed.info.width, probed.info.height),
        (Some(1080), Some(1920))
    );
    assert!(probed.info.has_audio);
}

#[test]
fn test_square_and_landscape() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let square = unwrap(apply_template(
        &rt.runtime,
        &sample,
        TemplateName::Square,
        None,
        None,
        false,
    ));
    let info = probe_video(&rt.runtime, &square.output).unwrap().info;
    assert_eq!((info.width, info.height), (Some(1080), Some(1080)));
    let land = unwrap(apply_template(
        &rt.runtime,
        &sample,
        TemplateName::Landscape,
        Some("T"),
        None,
        false,
    ));
    let info = probe_video(&rt.runtime, &land.output).unwrap().info;
    assert_eq!((info.width, info.height), (Some(1920), Some(1080)));
}

#[test]
fn test_intro_title() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let result = unwrap(apply_template(
        &rt.runtime,
        &sample,
        TemplateName::IntroTitle,
        Some("Abertura"),
        None,
        false,
    ));
    assert_eq!((result.width, result.height), (320, 240));
}

#[test]
fn test_intro_title_requires_title() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let error = apply_template(
        &rt.runtime,
        &sample,
        TemplateName::IntroTitle,
        None,
        None,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_watermark() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let image = logo(&rt.runtime.workspace);
    let result = unwrap(apply_template(
        &rt.runtime,
        &sample,
        TemplateName::Watermark,
        None,
        Some(&image),
        false,
    ));
    assert_eq!(result.output, "sample_watermark.mp4");
}

#[test]
fn test_watermark_requires_logo() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let error = apply_template(
        &rt.runtime,
        &sample,
        TemplateName::Watermark,
        None,
        None,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    let error = apply_template(
        &rt.runtime,
        &sample,
        TemplateName::Watermark,
        None,
        Some("ghost.png"),
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn test_unknown_template() {
    // No Rust o nome é um enum: um valor inválido é rejeitado na desserialização.
    let parsed: Result<TemplateName, _> = serde_json::from_str("\"nope\"");
    assert!(parsed.is_err());
}

#[test]
fn test_template_background() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let submitted = unwrap_job(apply_template(
        &rt.runtime,
        &sample,
        TemplateName::Square,
        None,
        None,
        true,
    ));
    let job = rt
        .runtime
        .jobs
        .wait(&submitted.job_id, Some(Duration::from_secs(120)))
        .unwrap();
    assert_eq!(job.status, JobStatus::Done);
}
