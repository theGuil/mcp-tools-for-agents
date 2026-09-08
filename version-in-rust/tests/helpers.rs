//! Auxiliares de teste.

#![allow(dead_code)]

use mcp_tools::core::errors::ToolResult;
use mcp_tools::core::jobs::{JobSubmitted, MaybeJob};

/// Garante que a tool devolveu sucesso síncrono e estreita o tipo.
pub fn unwrap<T: std::fmt::Debug>(result: ToolResult<MaybeJob<T>>) -> T {
    match result.expect("a tool devolveu erro") {
        MaybeJob::Done(value) => value,
        MaybeJob::Job(job) => panic!("a tool devolveu um job em vez do resultado: {job:?}"),
    }
}

/// Garante que a tool devolveu um job e estreita o tipo.
pub fn unwrap_job<T: std::fmt::Debug>(result: ToolResult<MaybeJob<T>>) -> JobSubmitted {
    match result.expect("a tool devolveu erro") {
        MaybeJob::Job(job) => job,
        MaybeJob::Done(value) => panic!("a tool devolveu resultado em vez de job: {value:?}"),
    }
}
