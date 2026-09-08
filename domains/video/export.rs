//! Tool `export_for_platform`: codifica o vídeo final com o preset da plataforma.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::core::numbers::{format_g, round_to};
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

/// Plataforma de destino.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Tiktok,
    InstagramReels,
    YoutubeShorts,
    Youtube,
    #[serde(rename = "youtube_4k")]
    Youtube4k,
    Twitter,
}

impl Platform {
    /// Nome da plataforma como aparece no JSON e no nome do arquivo.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tiktok => "tiktok",
            Self::InstagramReels => "instagram_reels",
            Self::YoutubeShorts => "youtube_shorts",
            Self::Youtube => "youtube",
            Self::Youtube4k => "youtube_4k",
            Self::Twitter => "twitter",
        }
    }

    /// Regras de codificação da plataforma.
    pub fn preset(self) -> PlatformPreset {
        match self {
            Self::Tiktok => PlatformPreset {
                name: self,
                description: "Vertical 1080x1920, H.264 High, 30 fps, AAC 192k, até 10 min.",
                max_width: 1080,
                max_height: 1920,
                max_fps: 30.0,
                crf: 20,
                audio_bitrate: "192k",
                orientation: Orientation::Vertical,
                max_seconds: Some(600.0),
            },
            Self::InstagramReels => PlatformPreset {
                name: self,
                description: "Vertical 1080x1920, H.264 High, 30 fps, AAC 192k, até 3 min.",
                max_width: 1080,
                max_height: 1920,
                max_fps: 30.0,
                crf: 20,
                audio_bitrate: "192k",
                orientation: Orientation::Vertical,
                max_seconds: Some(180.0),
            },
            Self::YoutubeShorts => PlatformPreset {
                name: self,
                description: "Vertical 1080x1920, H.264 High, até 60 fps, AAC 192k, até 3 min.",
                max_width: 1080,
                max_height: 1920,
                max_fps: 60.0,
                crf: 19,
                audio_bitrate: "192k",
                orientation: Orientation::Vertical,
                max_seconds: Some(180.0),
            },
            Self::Youtube => PlatformPreset {
                name: self,
                description: "Horizontal até 1920x1080, H.264 High, até 60 fps, AAC 256k.",
                max_width: 1920,
                max_height: 1080,
                max_fps: 60.0,
                crf: 18,
                audio_bitrate: "256k",
                orientation: Orientation::Horizontal,
                max_seconds: None,
            },
            Self::Youtube4k => PlatformPreset {
                name: self,
                description: "Horizontal até 3840x2160, H.264 High, até 60 fps, AAC 256k.",
                max_width: 3840,
                max_height: 2160,
                max_fps: 60.0,
                crf: 18,
                audio_bitrate: "256k",
                orientation: Orientation::Horizontal,
                max_seconds: None,
            },
            Self::Twitter => PlatformPreset {
                name: self,
                description:
                    "Qualquer orientação até 1920x1200, H.264, 30 fps, AAC 128k, até 140 s.",
                max_width: 1920,
                max_height: 1200,
                max_fps: 30.0,
                crf: 21,
                audio_bitrate: "128k",
                orientation: Orientation::Any,
                max_seconds: Some(140.0),
            },
        }
    }
}

/// Nível de qualidade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Standard,
    High,
}

impl Quality {
    fn crf_delta(self) -> i64 {
        match self {
            Self::Standard => 0,
            Self::High => -3,
        }
    }
}

/// Orientação esperada pela plataforma.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    Vertical,
    Horizontal,
    Any,
}

/// Regras de codificação de uma plataforma.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlatformPreset {
    pub name: Platform,
    pub description: &'static str,
    pub max_width: i64,
    pub max_height: i64,
    pub max_fps: f64,
    pub crf: i64,
    pub audio_bitrate: &'static str,
    pub orientation: Orientation,
    pub max_seconds: Option<f64>,
}

/// Todas as plataformas, na ordem do Python.
pub const PLATFORMS: &[Platform] = &[
    Platform::Tiktok,
    Platform::InstagramReels,
    Platform::YoutubeShorts,
    Platform::Youtube,
    Platform::Youtube4k,
    Platform::Twitter,
];

const MIN_SHORT_SIDE: i64 = 720;

/// Vídeo final exportado.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportForPlatformResult {
    pub output: String,
    pub platform: Platform,
    pub width: i64,
    pub height: i64,
    pub fps: f64,
    pub duration: f64,
    pub size_bytes: u64,
    pub warnings: Vec<String>,
}

/// Reduz (nunca amplia) para caber na caixa, mantendo a proporção e dimensões pares.
pub fn fit_size(width: i64, height: i64, max_width: i64, max_height: i64) -> (i64, i64) {
    let factor = 1f64
        .min(max_width as f64 / width as f64)
        .min(max_height as f64 / height as f64);
    let new_w = (width as f64 * factor).round_ties_even() as i64;
    let new_h = (height as f64 * factor).round_ties_even() as i64;
    (new_w - new_w % 2, new_h - new_h % 2)
}

fn warnings(preset: &PlatformPreset, width: i64, height: i64, duration: f64) -> Vec<String> {
    let mut notes: Vec<String> = Vec::new();
    if preset.orientation == Orientation::Vertical && width > height {
        notes.push(
            "O vídeo é horizontal, mas a plataforma é vertical. Use smart_crop \
             (preenche a tela seguindo o rosto) ou apply_template 'shorts' antes de exportar."
                .to_string(),
        );
    }
    if preset.orientation == Orientation::Horizontal && height > width {
        notes.push(
            "O vídeo é vertical, mas o preset é horizontal. Para Shorts use youtube_shorts; \
             para manter, aplique apply_template 'landscape' antes."
                .to_string(),
        );
    }
    if let Some(max_seconds) = preset.max_seconds {
        if duration > max_seconds {
            notes.push(format!(
                "Duração de {duration:.0}s passa do limite de {max_seconds:.0}s da \
                 plataforma. Use cut_video para encurtar."
            ));
        }
    }
    if width.min(height) < MIN_SHORT_SIDE {
        notes.push(format!(
            "Resolução baixa ({width}x{height}); a plataforma pode mostrar em qualidade \
             reduzida. A exportação nunca amplia a imagem."
        ));
    }
    notes
}

fn do_export(
    runtime: &Runtime,
    path: &str,
    platform: Platform,
    quality: Quality,
    output: Option<&str>,
) -> ToolResult<ExportForPlatformResult> {
    let source = runtime.workspace.existing(path)?;
    let preset = platform.preset();
    let info = runtime.ffmpeg.probe(&source)?;
    let (Some(src_width), Some(src_height)) = (info.width, info.height) else {
        return Err(no_video(path));
    };
    if !info.has_video {
        return Err(no_video(path));
    }
    let (width, height) = fit_size(src_width, src_height, preset.max_width, preset.max_height);
    let fps = info
        .fps
        .filter(|fps| *fps != 0.0)
        .unwrap_or(preset.max_fps)
        .min(preset.max_fps);
    let target = match output {
        Some(output) => runtime.workspace.resolve(output)?,
        None => runtime
            .workspace
            .output_for(&source, platform.as_str(), Some("mp4")),
    };
    let is_mp4 = target
        .extension()
        .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case("mp4"));
    if !is_mp4 {
        return Err(ToolError::with_hint(
            format!(
                "output '{}' deve terminar em .mp4.",
                output.unwrap_or_default()
            ),
            ErrorCode::InvalidArgument,
            "Todas as plataformas aceitam MP4 H.264.",
        ));
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            ToolError::new(
                format!("Não foi possível criar a pasta de saída: {error}"),
                ErrorCode::InvalidArgument,
            )
        })?;
    }
    let crf = preset.crf + quality.crf_delta();
    let video_filter = format!(
        "scale={width}:{height}:flags=lanczos,fps={},format=yuv420p",
        format_g(fps)
    );
    let mut args = ffargs![
        "-i",
        source,
        "-vf",
        video_filter,
        "-c:v",
        "libx264",
        "-preset",
        "medium",
        "-profile:v",
        "high",
        "-level",
        "4.2",
        "-crf",
        crf.to_string(),
        "-g",
        ((fps * 2.0).round_ties_even() as i64).to_string()
    ];
    if info.has_audio {
        args.extend(ffargs![
            "-c:a",
            "aac",
            "-b:a",
            preset.audio_bitrate,
            "-ar",
            "48000"
        ]);
    } else {
        args.extend(ffargs!["-an"]);
    }
    args.extend(ffargs!["-movflags", "+faststart"]);
    args.push(target.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    let size_bytes = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
    Ok(ExportForPlatformResult {
        output: runtime.workspace.relative(&target),
        platform,
        width,
        height,
        fps: round_to(fps, 3),
        duration: round_to(info.duration, 3),
        size_bytes,
        warnings: warnings(&preset, width, height, info.duration),
    })
}

fn no_video(path: &str) -> ToolError {
    ToolError::with_hint(
        format!("'{path}' não tem trilha de vídeo."),
        ErrorCode::InvalidArgument,
        "export_for_platform só se aplica a vídeos.",
    )
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Argumento inválido, arquivo inexistente ou falha do ffmpeg.
pub fn export_for_platform(
    runtime: &Arc<Runtime>,
    path: &str,
    platform: Platform,
    quality: Quality,
    output: Option<&str>,
    background: bool,
) -> ToolResult<MaybeJob<ExportForPlatformResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        let output = output.map(str::to_string);
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("export_for_platform", move || {
                do_export(&runtime_job, &path, platform, quality, output.as_deref())
            }),
        ));
    }
    Ok(MaybeJob::Done(do_export(
        runtime, path, platform, quality, output,
    )?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// tiktok, instagram_reels, youtube_shorts, youtube, youtube_4k ou twitter.
    pub platform: Platform,
    /// "standard" (equilíbrio tamanho/qualidade) ou "high" (arquivo maior).
    #[serde(default = "default_quality")]
    pub quality: Quality,
    /// Caminho do .mp4 final. Gerado ao lado do original se omitido.
    #[serde(default)]
    pub output: Option<String>,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_quality() -> Quality {
    Quality::Standard
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "export_for_platform",
        "Exporta o vídeo final no formato que a plataforma recomenda, pronto para subir.\n\n\
         Último passo da edição. Aplica o preset de codec, resolução máxima, fps, \
         qualidade (CRF) e bitrate de áudio da plataforma, gera MP4 H.264 High com \
         faststart (começa a tocar antes de baixar). Nunca amplia a imagem nem muda a \
         proporção: para virar vertical use smart_crop ou apply_template antes. \
         Devolve warnings quando algo vai contra as regras da plataforma (orientação \
         errada, duração acima do limite, resolução baixa), com a tool que resolve.\n\n\
         Presets: tiktok e instagram_reels (1080x1920, 30 fps), youtube_shorts \
         (1080x1920, 60 fps), youtube (1920x1080), youtube_4k (3840x2160), twitter \
         (1920x1200, 140 s). Para volume padronizado rode normalize_audio antes.",
        move |params: Params| {
            guarded(export_for_platform(
                &runtime,
                &params.path,
                params.platform,
                params.quality,
                params.output.as_deref(),
                params.background,
            ))
        },
    );
}
