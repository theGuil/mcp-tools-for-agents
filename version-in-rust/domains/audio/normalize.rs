//! Tool `normalize_audio`: normaliza o volume para o padrão de loudness das plataformas.

use std::ffi::OsString;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::core::numbers::{format_g, round_to};
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

/// Plataforma alvo, cada uma com sua loudness integrada recomendada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LoudnessPreset {
    Tiktok,
    Youtube,
    Instagram,
    Podcast,
    Broadcast,
}

impl LoudnessPreset {
    /// Nome do preset como o agente informa.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tiktok => "tiktok",
            Self::Youtube => "youtube",
            Self::Instagram => "instagram",
            Self::Podcast => "podcast",
            Self::Broadcast => "broadcast",
        }
    }

    /// Loudness integrada (LUFS) recomendada pela plataforma.
    pub fn target_lufs(self) -> f64 {
        match self {
            Self::Tiktok | Self::Youtube | Self::Instagram => -14.0,
            Self::Podcast => -16.0,
            Self::Broadcast => -23.0,
        }
    }
}

const TRUE_PEAK: f64 = -1.5;
const LRA: f64 = 11.0;
const MIN_LUFS: f64 = -70.0;
const MAX_LUFS: f64 = -5.0;

/// Localiza o bloco JSON do loudnorm no stderr: o objeto `{...}` sem chaves
/// aninhadas que contém `"input_i"` (o `\{[^{}]*"input_i"[^{}]*\}` do Python).
fn json_block(stderr: &str) -> Option<&str> {
    let key = stderr.find("\"input_i\"")?;
    let start = stderr[..key].rfind('{')?;
    if stderr[start..key].contains('}') {
        return None;
    }
    let end = key + stderr[key..].find('}')? + 1;
    if stderr[key..end].contains('{') {
        return None;
    }
    Some(&stderr[start..end])
}

/// Arquivo gerado com o áudio normalizado.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NormalizeAudioResult {
    pub output: String,
    pub target_lufs: f64,
    pub measured_lufs: Option<f64>,
    pub measured_true_peak: Option<f64>,
    pub gain_db: Option<f64>,
    pub is_video: bool,
}

/// Medidas da primeira passada do loudnorm, como texto (o ffmpeg devolve strings).
#[derive(Debug, Clone, PartialEq)]
struct Measurement {
    input_i: String,
    input_tp: String,
    input_lra: String,
    input_thresh: String,
    target_offset: String,
}

fn resolve_target(preset: Option<LoudnessPreset>, target_lufs: Option<f64>) -> ToolResult<f64> {
    if let Some(target) = target_lufs {
        if !(MIN_LUFS..=MAX_LUFS).contains(&target) {
            return Err(ToolError::with_hint(
                format!(
                    "target_lufs deve estar entre {} e {}.",
                    format_g(MIN_LUFS),
                    format_g(MAX_LUFS)
                ),
                ErrorCode::InvalidArgument,
                "-14 é o padrão de TikTok, YouTube e Instagram.",
            ));
        }
        return Ok(target);
    }
    Ok(preset.unwrap_or(LoudnessPreset::Youtube).target_lufs())
}

/// Valor do JSON do loudnorm como texto, como o `str(...)` do Python.
fn as_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Primeira passada do loudnorm: só mede, sem gravar nada.
fn measure(runtime: &Runtime, source_args: &[OsString], target: f64) -> ToolResult<Measurement> {
    let mut args = source_args.to_vec();
    args.extend(ffargs![
        "-af",
        format!("loudnorm=I={target:?}:TP={TRUE_PEAK:?}:LRA={LRA:?}:print_format=json"),
        "-vn",
        "-f",
        "null",
        "-"
    ]);
    let stderr = runtime.ffmpeg.run(&args)?;
    let no_measure = || {
        ToolError::with_hint(
            "Não foi possível medir o loudness do áudio.",
            ErrorCode::FfmpegFailed,
            "Confira com probe_video se o arquivo tem trilha de áudio válida.",
        )
    };
    let block = json_block(&stderr).ok_or_else(no_measure)?;
    let data: serde_json::Value = serde_json::from_str(block).map_err(|_| no_measure())?;
    let field =
        |name: &str| -> ToolResult<String> { data.get(name).map(as_text).ok_or_else(no_measure) };
    Ok(Measurement {
        input_i: field("input_i")?,
        input_tp: field("input_tp")?,
        input_lra: field("input_lra")?,
        input_thresh: field("input_thresh")?,
        target_offset: field("target_offset")?,
    })
}

/// Converte o texto do loudnorm em número; `None` para inf, nan ou lixo.
fn as_float(value: &str) -> Option<f64> {
    let number: f64 = value.trim().parse().ok()?;
    number.is_finite().then_some(number)
}

fn do_normalize(
    runtime: &Runtime,
    path: &str,
    preset: Option<LoudnessPreset>,
    target_lufs: Option<f64>,
) -> ToolResult<NormalizeAudioResult> {
    let source = runtime.workspace.existing(path)?;
    let target = resolve_target(preset, target_lufs)?;
    let info = runtime.ffmpeg.probe(&source)?;
    if !info.has_audio {
        return Err(ToolError::with_hint(
            format!("'{path}' não tem trilha de áudio."),
            ErrorCode::InvalidArgument,
            "Só arquivos com som podem ser normalizados.",
        ));
    }
    let source_args = ffargs!["-i", source];
    let measured = measure(runtime, &source_args, target)?;
    let Some(measured_i) = as_float(&measured.input_i) else {
        return Err(ToolError::with_hint(
            "O áudio é silêncio total, não há o que normalizar.",
            ErrorCode::InvalidArgument,
            "Confira o volume original com probe_video ou extraia o áudio com extract_audio.",
        ));
    };
    let second_pass = format!(
        "loudnorm=I={target:?}:TP={TRUE_PEAK:?}:LRA={LRA:?}\
         :measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}\
         :offset={}:linear=true:print_format=summary",
        measured.input_i,
        measured.input_tp,
        measured.input_lra,
        measured.input_thresh,
        measured.target_offset
    );
    let is_video = info.has_video;
    let output = runtime.workspace.output_for(&source, "normalized", None);
    let codec_args: &[&str] = if is_video {
        &[
            "-map", "0:v:0", "-map", "0:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k",
        ]
    } else {
        &["-map", "0:a:0"]
    };
    let mut args = ffargs!["-i", source, "-af", second_pass];
    args.extend(codec_args.iter().map(OsString::from));
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(NormalizeAudioResult {
        output: runtime.workspace.relative(&output),
        target_lufs: target,
        measured_lufs: Some(measured_i),
        measured_true_peak: as_float(&measured.input_tp),
        gain_db: Some(round_to(target - measured_i, 2)),
        is_video,
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Arquivo inexistente, `target_lufs` fora da faixa, sem trilha de áudio,
/// silêncio total ou falha do ffmpeg.
pub fn normalize_audio(
    runtime: &Arc<Runtime>,
    path: &str,
    preset: Option<LoudnessPreset>,
    target_lufs: Option<f64>,
    background: bool,
) -> ToolResult<MaybeJob<NormalizeAudioResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("normalize_audio", move || {
                do_normalize(&runtime_job, &path, preset, target_lufs)
            }),
        ));
    }
    Ok(MaybeJob::Done(do_normalize(
        runtime,
        path,
        preset,
        target_lufs,
    )?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo ou áudio, relativo ao workspace.
    pub path: String,
    /// Plataforma alvo: tiktok, youtube, instagram, podcast ou broadcast.
    #[serde(default)]
    pub preset: Option<LoudnessPreset>,
    /// Loudness alvo em LUFS, ex: -14. Ignora o preset se informado.
    #[serde(default)]
    pub target_lufs: Option<f64>,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "normalize_audio",
        "Normaliza o volume do áudio para o padrão de loudness da plataforma (EBU R128).\n\n\
         Use antes de exportar: garante que o vídeo não toque baixo demais nem \
         estoure em relação aos outros vídeos do feed. Faz duas passadas do \
         loudnorm (mede e depois corrige), o mesmo processo de mastering usado \
         em estúdio. Funciona em vídeo (só o áudio é re-encodado, a imagem é \
         copiada) e em áudio puro (mp3, wav, m4a).\n\n\
         Presets: tiktok, youtube e instagram (-14 LUFS), podcast (-16 LUFS), \
         broadcast (-23 LUFS). Sem preset usa -14 LUFS. target_lufs sobrescreve. \
         Devolve o loudness medido e o ganho aplicado em dB.",
        move |params: Params| {
            guarded(normalize_audio(
                &runtime,
                &params.path,
                params.preset,
                params.target_lufs,
                params.background,
            ))
        },
    );
}
