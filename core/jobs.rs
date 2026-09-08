//! Tarefas longas executadas em background.
//!
//! Cortes e re-encodes podem levar minutos. Em vez de travar o agente, a tool
//! devolve um `job_id` e o agente consulta o status quando quiser.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::core::errors::{ErrorCode, ToolError, ToolResult};

/// Identificador curto de um job (12 caracteres hexadecimais).
pub type JobId = String;

/// Resultado de um job: o mesmo objeto que a tool síncrona devolveria.
pub type JobResult = Map<String, Value>;

/// Estado de um job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Done,
    Failed,
}

impl JobStatus {
    /// Nome do estado como aparece no JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    fn is_open(self) -> bool {
        matches!(self, Self::Pending | Self::Running)
    }
}

/// Resposta de uma tool que foi enfileirada em background.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSubmitted {
    pub job_id: JobId,
    pub status: JobStatus,
    pub tool: String,
}

/// Estado de um job para o agente acompanhar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobStatusPayload {
    pub job_id: JobId,
    pub tool: String,
    pub status: JobStatus,
    pub elapsed_seconds: f64,
    pub error: Option<String>,
}

/// Retorno de uma tool que aceita `background`: o resultado ou o job enfileirado.
///
/// Serializa sem envelope, como a união `Result | JobSubmitted` do Python.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum MaybeJob<T> {
    Done(T),
    Job(JobSubmitted),
}

/// Registro de uma tarefa em execução.
#[derive(Debug, Clone)]
pub struct Job {
    pub id: JobId,
    pub tool: String,
    pub created_at: Instant,
    pub finished_at: Option<Instant>,
    pub status: JobStatus,
    pub result: Option<JobResult>,
    pub error: Option<String>,
}

impl Job {
    fn new(id: JobId, tool: &str) -> Self {
        Self {
            id,
            tool: tool.to_string(),
            created_at: Instant::now(),
            finished_at: None,
            status: JobStatus::Pending,
            result: None,
            error: None,
        }
    }

    /// Segundos entre criação e término, ou até agora se ainda roda.
    pub fn elapsed(&self) -> f64 {
        let end = self.finished_at.unwrap_or_else(Instant::now);
        let secs = end.duration_since(self.created_at).as_secs_f64();
        (secs * 1000.0).round() / 1000.0
    }

    /// Serializa o estado para o agente.
    pub fn to_payload(&self) -> JobStatusPayload {
        JobStatusPayload {
            job_id: self.id.clone(),
            tool: self.tool.clone(),
            status: self.status,
            elapsed_seconds: self.elapsed(),
            error: self.error.clone(),
        }
    }
}

type Task = Box<dyn FnOnce() -> ToolResult<JobResult> + Send + 'static>;
type Registry = Arc<Mutex<HashMap<JobId, Job>>>;

/// Fila de jobs em threads, com registro em memória.
pub struct JobManager {
    jobs: Registry,
    sender: Mutex<Option<Sender<(JobId, Task)>>>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl std::fmt::Debug for JobManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobManager").finish_non_exhaustive()
    }
}

impl JobManager {
    /// Cria o gerenciador com `workers` threads concorrentes.
    pub fn new(workers: usize) -> Self {
        let (sender, receiver) = channel::<(JobId, Task)>();
        let receiver = Arc::new(Mutex::new(receiver));
        let jobs: Registry = Arc::new(Mutex::new(HashMap::new()));
        let handles = (0..workers.max(1))
            .map(|index| {
                let receiver = Arc::clone(&receiver);
                let jobs = Arc::clone(&jobs);
                thread::Builder::new()
                    .name(format!("job-{index}"))
                    .spawn(move || worker_loop(&receiver, &jobs))
                    .expect("não foi possível criar a thread de jobs")
            })
            .collect();
        Self {
            jobs,
            sender: Mutex::new(Some(sender)),
            workers: Mutex::new(handles),
        }
    }

    /// Agenda `task` e devolve imediatamente o `job_id`.
    ///
    /// O resultado da task é serializado para o mesmo objeto que a tool
    /// síncrona devolveria, então `job_result` entrega o formato tipado.
    pub fn submit<T, F>(&self, tool: &str, task: F) -> JobSubmitted
    where
        T: Serialize,
        F: FnOnce() -> ToolResult<T> + Send + 'static,
    {
        let id = new_job_id();
        let job = Job::new(id.clone(), tool);
        lock(&self.jobs).insert(id.clone(), job);
        let boxed: Task = Box::new(move || task().map(|value| to_map(&value)));
        let sender = lock(&self.sender);
        match sender.as_ref() {
            Some(sender) if sender.send((id.clone(), boxed)).is_ok() => {}
            _ => {
                let mut jobs = lock(&self.jobs);
                if let Some(job) = jobs.get_mut(&id) {
                    job.status = JobStatus::Failed;
                    job.error = Some("gerenciador de jobs encerrado".to_string());
                    job.finished_at = Some(Instant::now());
                }
            }
        }
        JobSubmitted {
            job_id: id,
            status: JobStatus::Pending,
            tool: tool.to_string(),
        }
    }

    /// Busca um job pelo id.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::JobNotFound`] se o id não existir.
    pub fn get(&self, job_id: &str) -> ToolResult<Job> {
        lock(&self.jobs).get(job_id).cloned().ok_or_else(|| {
            ToolError::with_hint(
                format!("Job '{job_id}' não encontrado."),
                ErrorCode::JobNotFound,
                "O id vem da resposta da tool que criou o job.",
            )
        })
    }

    /// Resultado de um job concluído.
    ///
    /// # Errors
    ///
    /// Se o job não existir, ainda rodar ou tiver falhado.
    pub fn result(&self, job_id: &str) -> ToolResult<JobResult> {
        let job = self.get(job_id)?;
        if job.status.is_open() {
            return Err(ToolError::with_hint(
                format!("Job '{job_id}' ainda está {}.", job.status.as_str()),
                ErrorCode::JobNotFinished,
                "Consulte job_status e tente novamente em alguns segundos.",
            ));
        }
        match job.result {
            Some(result) if job.status == JobStatus::Done => Ok(result),
            _ => Err(ToolError::new(
                format!(
                    "Job '{job_id}' falhou: {}",
                    job.error.as_deref().unwrap_or("sem detalhes")
                ),
                ErrorCode::FfmpegFailed,
            )),
        }
    }

    /// Bloqueia até o job terminar (ou até `timeout`). Útil em testes.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::JobNotFound`] se o id não existir.
    pub fn wait(&self, job_id: &str, timeout: Option<Duration>) -> ToolResult<Job> {
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            let job = self.get(job_id)?;
            if !job.status.is_open() {
                return Ok(job);
            }
            if deadline.is_some_and(|d| Instant::now() > d) {
                return Ok(job);
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Encerra as threads sem esperar jobs pendentes.
    pub fn shutdown(&self) {
        lock(&self.sender).take();
        lock(&self.workers).clear();
    }
}

impl Drop for JobManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn worker_loop(receiver: &Mutex<Receiver<(JobId, Task)>>, jobs: &Registry) {
    loop {
        let next = lock(receiver).recv();
        let Ok((id, task)) = next else { return };
        run_job(&id, task, jobs);
    }
}

fn run_job(id: &str, task: Task, jobs: &Registry) {
    if let Some(job) = lock(jobs).get_mut(id) {
        job.status = JobStatus::Running;
    }
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
    let mut registry = lock(jobs);
    let Some(job) = registry.get_mut(id) else {
        return;
    };
    job.finished_at = Some(Instant::now());
    match outcome {
        Ok(Ok(result)) => {
            job.result = Some(result);
            job.status = JobStatus::Done;
        }
        Ok(Err(error)) => {
            job.status = JobStatus::Failed;
            job.error = Some(error.message);
        }
        Err(panic) => {
            job.status = JobStatus::Failed;
            job.error = Some(format!("panic: {}", panic_message(panic.as_ref())));
        }
    }
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = panic.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = panic.downcast_ref::<String>() {
        text.clone()
    } else {
        "erro interno".to_string()
    }
}

fn new_job_id() -> JobId {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

/// Serializa o resultado tipado para o mapa guardado no job.
fn to_map<T: Serialize>(value: &T) -> JobResult {
    match serde_json::to_value(value) {
        Ok(Value::Object(map)) => map,
        Ok(other) => {
            let mut map = Map::new();
            map.insert("value".to_string(), other);
            map
        }
        Err(_) => Map::new(),
    }
}

/// Trava um mutex ignorando envenenamento: o estado guardado continua válido.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
