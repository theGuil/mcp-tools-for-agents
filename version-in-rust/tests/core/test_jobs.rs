use std::time::Duration;

use mcp_tools::core::errors::{ErrorCode, ToolError, ToolResult};
use mcp_tools::core::jobs::JobStatus;
use serde_json::json;

use crate::conftest::jobs;

#[test]
fn test_submit_and_result() {
    let jobs = jobs();
    let submitted = jobs.submit("demo", || -> ToolResult<_> { Ok(json!({"answer": 42})) });
    assert_eq!(submitted.status, JobStatus::Pending);
    let job = jobs
        .wait(&submitted.job_id, Some(Duration::from_secs(5)))
        .unwrap();
    assert_eq!(job.status, JobStatus::Done);
    let result = jobs.result(&job.id).unwrap();
    assert_eq!(result.get("answer"), Some(&json!(42)));
    assert!(job.to_payload().elapsed_seconds >= 0.0);
}

#[test]
fn test_failed_job() {
    let jobs = jobs();
    let submitted = jobs.submit("demo", || -> ToolResult<serde_json::Value> {
        Err(ToolError::new("quebrou", ErrorCode::FfmpegFailed))
    });
    let job = jobs
        .wait(&submitted.job_id, Some(Duration::from_secs(5)))
        .unwrap();
    assert_eq!(job.status, JobStatus::Failed);
    assert_eq!(job.error.as_deref(), Some("quebrou"));
    let error = jobs.result(&job.id).unwrap_err();
    assert_eq!(error.code, ErrorCode::FfmpegFailed);
}

#[test]
fn test_unknown_job() {
    let jobs = jobs();
    let error = jobs.get("nope").unwrap_err();
    assert_eq!(error.code, ErrorCode::JobNotFound);
}
