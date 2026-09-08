//! Tool `smart_crop`: reenquadra para vertical/quadrado seguindo o rosto de quem fala.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::core::numbers::format_g;
use crate::core::vision::load_face_detector;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

/// Proporção final do recorte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CropAspect {
    #[serde(rename = "9:16")]
    Vertical,
    #[serde(rename = "1:1")]
    Square,
    #[serde(rename = "4:5")]
    Portrait,
    #[serde(rename = "16:9")]
    Landscape,
}

impl CropAspect {
    /// Nome da proporção como o agente escreve (`9:16`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vertical => "9:16",
            Self::Square => "1:1",
            Self::Portrait => "4:5",
            Self::Landscape => "16:9",
        }
    }

    fn ratio(self) -> (i64, i64) {
        match self {
            Self::Vertical => (9, 16),
            Self::Square => (1, 1),
            Self::Portrait => (4, 5),
            Self::Landscape => (16, 9),
        }
    }
}

/// Como escolher a janela do recorte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CropMode {
    Face,
    Center,
}

const ANALYSIS_WIDTH: i64 = 480;
const COMMAND_STEP: f64 = 0.1;
const MIN_INTERVAL: f64 = 0.1;
const MAX_INTERVAL: f64 = 5.0;
const DEADZONE_RATIO: f64 = 0.04;
const SMOOTHING: f64 = 0.35;

/// Vídeo gerado com o novo enquadramento.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SmartCropResult {
    pub output: String,
    pub aspect: CropAspect,
    pub mode_used: CropMode,
    pub width: i64,
    pub height: i64,
    pub frames_analyzed: usize,
    pub frames_with_face: usize,
}

/// Rastreador de rosto: devolve a posição x do rosto em cada frame amostrado
/// (`None` quando não há rosto) e quantos frames tinham rosto. Ponto de injeção
/// para testes, como o `monkeypatch` de `_face_track` no Python.
pub type FaceTracker =
    Arc<dyn Fn(&Runtime, &Path, i64, f64) -> ToolResult<(Vec<Option<f64>>, usize)> + Send + Sync>;

/// Maior janela com a proporção pedida que cabe dentro do vídeo (dimensões pares).
pub fn crop_size(width: i64, height: i64, aspect: CropAspect) -> (i64, i64) {
    let (num, den) = aspect.ratio();
    let (mut crop_w, mut crop_h) = (
        width,
        round_half_even(width as f64 * den as f64 / num as f64),
    );
    if crop_h > height {
        crop_h = height;
        crop_w = round_half_even(height as f64 * num as f64 / den as f64);
    }
    (crop_w - crop_w % 2, crop_h - crop_h % 2)
}

/// `round()` do Python: arredonda para o inteiro par mais próximo em empate.
fn round_half_even(value: f64) -> i64 {
    value.round_ties_even() as i64
}

/// Suaviza a trajetória do rosto: ignora tremores pequenos e segura a última posição.
pub fn smooth_positions(samples: &[Option<f64>], default: f64, deadzone: f64) -> Vec<f64> {
    let mut positions = Vec::with_capacity(samples.len());
    let mut current = samples.iter().flatten().next().copied().unwrap_or(default);
    for sample in samples {
        if let Some(sample) = sample {
            let delta = sample - current;
            if delta.abs() > deadzone {
                current += delta * SMOOTHING;
            }
        }
        positions.push(current);
    }
    positions
}

fn extract_frames(
    runtime: &Runtime,
    source: &Path,
    folder: &Path,
    interval: f64,
) -> ToolResult<Vec<PathBuf>> {
    let mut args = ffargs![
        "-i",
        source,
        "-vf",
        format!("fps=1/{},scale={ANALYSIS_WIDTH}:-2", format_g(interval)),
        "-q:v",
        "4"
    ];
    args.push(folder.join("f%06d.jpg").into_os_string());
    runtime.ffmpeg.run(&args)?;
    let mut frames: Vec<PathBuf> = std::fs::read_dir(folder)
        .map_err(|error| {
            ToolError::new(
                format!("Não foi possível ler os frames de análise: {error}"),
                ErrorCode::FfmpegFailed,
            )
        })?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
            name.is_some_and(|n| n.starts_with('f') && n.ends_with(".jpg"))
        })
        .collect();
    frames.sort();
    Ok(frames)
}

fn face_track(
    runtime: &Runtime,
    source: &Path,
    width: i64,
    interval: f64,
) -> ToolResult<(Vec<Option<f64>>, usize)> {
    let detector = load_face_detector()?;
    let scale = width as f64 / ANALYSIS_WIDTH as f64;
    let tmp = tempfile::Builder::new()
        .prefix("smart_crop_")
        .tempdir()
        .map_err(|error| {
            ToolError::new(
                format!("Não foi possível criar a pasta temporária: {error}"),
                ErrorCode::FfmpegFailed,
            )
        })?;
    let frames = extract_frames(runtime, source, tmp.path(), interval)?;
    let mut samples: Vec<Option<f64>> = Vec::with_capacity(frames.len());
    let mut found = 0;
    for frame in &frames {
        match detector.largest_face(frame)? {
            None => samples.push(None),
            Some(face) => {
                samples.push(Some(face.center_x() * scale));
                found += 1;
            }
        }
    }
    Ok((samples, found))
}

/// Arquivo do `sendcmd`: posição x do crop a cada décimo de segundo, interpolada.
fn write_commands(positions: &[f64], interval: f64, crop_w: i64, width: i64) -> String {
    let mut lines: Vec<String> = Vec::new();
    let max_x = width - crop_w;
    for (index, pos) in positions.iter().enumerate() {
        let nxt = positions.get(index + 1).copied().unwrap_or(*pos);
        let steps = round_half_even(interval / COMMAND_STEP).max(1);
        for step in 0..steps {
            let t = index as f64 * interval + step as f64 * COMMAND_STEP;
            let value = pos + (nxt - pos) * (step as f64 / steps as f64);
            let x = round_half_even(value - crop_w as f64 / 2.0).clamp(0, max_x);
            lines.push(format!("{t:.2} crop x {x};"));
        }
    }
    format!("{}\n", lines.join("\n"))
}

fn validate(sample_interval: f64) -> ToolResult<()> {
    if !(MIN_INTERVAL..=MAX_INTERVAL).contains(&sample_interval) {
        return Err(ToolError::new(
            format!(
                "sample_interval deve estar entre {} e {} segundos.",
                format_g(MIN_INTERVAL),
                format_g(MAX_INTERVAL)
            ),
            ErrorCode::InvalidArgument,
        ));
    }
    Ok(())
}

/// Arquivo temporário do sendcmd, apagado ao sair de escopo.
struct CommandFile(tempfile::NamedTempFile);

/// Rastreia o rosto e grava o arquivo do sendcmd. Devolve (arquivo, analisados, com rosto).
fn command_file(
    runtime: &Runtime,
    source: &Path,
    tracker: &FaceTracker,
    width: i64,
    crop_w: i64,
    interval: f64,
) -> ToolResult<(Option<CommandFile>, usize, usize)> {
    let (samples, found) = tracker(runtime, source, width, interval)?;
    if found == 0 {
        return Ok((None, samples.len(), 0));
    }
    let positions = smooth_positions(&samples, width as f64 / 2.0, width as f64 * DEADZONE_RATIO);
    let io_error = |error: std::io::Error| {
        ToolError::new(
            format!("Não foi possível gravar o arquivo de comandos: {error}"),
            ErrorCode::FfmpegFailed,
        )
    };
    let mut handle = tempfile::Builder::new()
        .suffix(".cmd")
        .tempfile()
        .map_err(io_error)?;
    handle
        .write_all(write_commands(&positions, interval, crop_w, width).as_bytes())
        .map_err(io_error)?;
    handle.flush().map_err(io_error)?;
    Ok((Some(CommandFile(handle)), samples.len(), found))
}

fn do_smart_crop(
    runtime: &Runtime,
    path: &str,
    aspect: CropAspect,
    mode: CropMode,
    sample_interval: f64,
    tracker: &FaceTracker,
) -> ToolResult<SmartCropResult> {
    let source = runtime.workspace.existing(path)?;
    validate(sample_interval)?;
    let info = runtime.ffmpeg.probe(&source)?;
    let (Some(width), Some(height)) = (info.width, info.height) else {
        return Err(no_video(path));
    };
    if !info.has_video {
        return Err(no_video(path));
    }
    let (crop_w, crop_h) = crop_size(width, height, aspect);
    if crop_w == width && crop_h == height {
        return Err(ToolError::with_hint(
            format!(
                "O vídeo já está em {} ({width}x{height}); não há o que recortar.",
                aspect.as_str()
            ),
            ErrorCode::InvalidArgument,
            "Use apply_template para adicionar bordas ou fundo desfocado.",
        ));
    }
    let center_x = (width - crop_w) / 2;
    let y = (height - crop_h) / 2;
    let mut commands: Option<CommandFile> = None;
    let (mut frames_analyzed, mut frames_with_face) = (0, 0);
    if mode == CropMode::Face && crop_w < width {
        let (file, analyzed, with_face) =
            command_file(runtime, &source, tracker, width, crop_w, sample_interval)?;
        commands = file;
        frames_analyzed = analyzed;
        frames_with_face = with_face;
    }
    let mode_used = if commands.is_some() {
        CropMode::Face
    } else {
        CropMode::Center
    };
    let mut filter_expr = format!("crop={crop_w}:{crop_h}:{center_x}:{y}");
    if let Some(file) = &commands {
        let escaped = file
            .0
            .path()
            .to_string_lossy()
            .replace('\\', "/")
            .replace(':', "\\:");
        filter_expr = format!("sendcmd=f='{escaped}',{filter_expr}");
    }
    let output = runtime.workspace.output_for(
        &source,
        &format!("crop_{}", aspect.as_str().replace(':', "x")),
        None,
    );
    let mut args = ffargs![
        "-i",
        source,
        "-vf",
        filter_expr,
        "-c:v",
        "libx264",
        "-preset",
        "fast",
        "-pix_fmt",
        "yuv420p"
    ];
    if info.has_audio {
        args.extend(ffargs!["-c:a", "copy"]);
    }
    args.push(output.clone().into_os_string());
    let run = runtime.ffmpeg.run(&args);
    // O arquivo de comandos é apagado aqui, com ou sem sucesso do ffmpeg.
    drop(commands);
    run?;
    Ok(SmartCropResult {
        output: runtime.workspace.relative(&output),
        aspect,
        mode_used,
        width: crop_w,
        height: crop_h,
        frames_analyzed,
        frames_with_face,
    })
}

fn no_video(path: &str) -> ToolError {
    ToolError::with_hint(
        format!("'{path}' não tem trilha de vídeo."),
        ErrorCode::InvalidArgument,
        "smart_crop só se aplica a vídeos.",
    )
}

/// Rastreador padrão: extrai frames com o ffmpeg e roda o YuNet.
pub fn default_tracker() -> FaceTracker {
    Arc::new(face_track)
}

/// Implementação pura com rastreador injetável (para testes sem detector).
///
/// # Errors
///
/// Argumento inválido, arquivo inexistente, detector indisponível ou falha do ffmpeg.
pub fn smart_crop_with_tracker(
    runtime: &Arc<Runtime>,
    path: &str,
    aspect: CropAspect,
    mode: CropMode,
    sample_interval: f64,
    background: bool,
    tracker: FaceTracker,
) -> ToolResult<MaybeJob<SmartCropResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("smart_crop", move || {
                do_smart_crop(&runtime_job, &path, aspect, mode, sample_interval, &tracker)
            }),
        ));
    }
    Ok(MaybeJob::Done(do_smart_crop(
        runtime,
        path,
        aspect,
        mode,
        sample_interval,
        &tracker,
    )?))
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Argumento inválido, arquivo inexistente, detector indisponível ou falha do ffmpeg.
pub fn smart_crop(
    runtime: &Arc<Runtime>,
    path: &str,
    aspect: CropAspect,
    mode: CropMode,
    sample_interval: f64,
    background: bool,
) -> ToolResult<MaybeJob<SmartCropResult>> {
    smart_crop_with_tracker(
        runtime,
        path,
        aspect,
        mode,
        sample_interval,
        background,
        default_tracker(),
    )
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// Proporção final: 9:16 (padrão), 1:1, 4:5 ou 16:9.
    #[serde(default = "default_aspect")]
    pub aspect: CropAspect,
    /// "face" segue o rosto; "center" recorta o centro fixo.
    #[serde(default = "default_mode")]
    pub mode: CropMode,
    /// Segundos entre frames analisados. Menor = mais preciso e lento.
    #[serde(default = "default_interval")]
    pub sample_interval: f64,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_aspect() -> CropAspect {
    CropAspect::Vertical
}

fn default_mode() -> CropMode {
    CropMode::Face
}

fn default_interval() -> f64 {
    0.5
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "smart_crop",
        "Reenquadra um vídeo horizontal para vertical (9:16) seguindo o rosto de quem fala.\n\n\
         É o passo que transforma um podcast ou aula gravada em 16:9 em um corte \
         para TikTok, Reels ou Shorts sem cortar a pessoa fora do quadro. A tool \
         analisa um frame a cada sample_interval segundos, encontra o maior rosto, \
         suaviza o movimento da câmera virtual e recorta o vídeo acompanhando. \
         Sem rosto detectado (ou com mode=\"center\") recorta o centro. Não adiciona \
         bordas: o resultado tem exatamente a proporção pedida com a altura do \
         original. Depois use export_for_platform para 1080x1920.\n\n\
         Diferença para apply_template \"shorts\": o template mantém o vídeo inteiro \
         pequeno sobre um fundo desfocado; smart_crop preenche a tela com a pessoa.\n\n\
         Requer o extra \"vision\" (binário compilado com a feature vision) para \
         mode=\"face\". O original não é modificado; o vídeo é re-encodado. Use \
         background=true em vídeos longos.",
        move |params: Params| {
            guarded(smart_crop(
                &runtime,
                &params.path,
                params.aspect,
                params.mode,
                params.sample_interval,
                params.background,
            ))
        },
    );
}
