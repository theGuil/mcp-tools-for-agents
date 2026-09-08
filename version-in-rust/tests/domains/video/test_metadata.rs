use std::process::Command;

use mcp_tools::core::errors::ErrorCode;
use mcp_tools::domains::video::metadata::{set_video_metadata, MetadataFields};

use crate::conftest::{runtime, sample_video};
use crate::skip_without_ffmpeg;

#[test]
fn test_set_metadata() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let fields = MetadataFields {
        title: Some("Meu título".to_string()),
        description: Some("Descrição longa".to_string()),
        author: Some("Eu".to_string()),
        comment: None,
    };
    let result = set_video_metadata(&rt.runtime, &sample, &fields).unwrap();
    assert_eq!(result.output, "sample_meta.mp4");
    let target = rt.runtime.workspace.existing(&result.output).unwrap();
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-print_format", "json", "-show_format"])
        .arg(&target)
        .output()
        .expect("ffprobe");
    assert!(output.status.success());
    let raw: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let tags = &raw["format"]["tags"];
    assert_eq!(tags["title"], "Meu título");
    assert_eq!(tags["description"], "Descrição longa");
    assert_eq!(tags["artist"], "Eu");
}

#[test]
fn test_set_metadata_nothing() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let error = set_video_metadata(&rt.runtime, &sample, &MetadataFields::default()).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_set_metadata_too_long() {
    skip_without_ffmpeg!();
    let rt = runtime();
    let sample = sample_video(&rt.runtime.workspace);
    let fields = MetadataFields {
        description: Some("x".repeat(6000)),
        ..MetadataFields::default()
    };
    let error = set_video_metadata(&rt.runtime, &sample, &fields).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
}

#[test]
fn test_set_metadata_missing() {
    let rt = runtime();
    let fields = MetadataFields {
        title: Some("x".to_string()),
        ..MetadataFields::default()
    };
    let error = set_video_metadata(&rt.runtime, "ghost.mp4", &fields).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}
