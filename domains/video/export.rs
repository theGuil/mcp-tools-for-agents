//! Tool `export_for_platform`: codifica o vídeo final com o preset da plataforma.

use std::collections::HashSet;
use std::ffi::OsString;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::core::numbers::{format_g, round_to};
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

/// Codec de vídeo do arquivo final.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum VideoCodec {
    /// H.264/AVC: aceito em toda plataforma. Padrão.
    #[default]
    H264,
    /// H.265/HEVC: mesma qualidade com ~40% menos bytes; aceito por TikTok,
    /// Instagram e YouTube, não pelo X (Twitter).
    #[serde(alias = "hevc")]
    H265,
    /// AV1: arquivos ainda menores; aceito pelo YouTube, evite em TikTok, Reels e X.
    Av1,
}

impl VideoCodec {
    /// Nome como aparece no JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::H265 => "h265",
            Self::Av1 => "av1",
        }
    }

    /// Encoders por software, do melhor para o alternativo.
    fn software_encoders(self) -> &'static [&'static str] {
        match self {
            Self::H264 => &["libx264"],
            Self::H265 => &["libx265"],
            Self::Av1 => &["libsvtav1", "libaom-av1"],
        }
    }

    /// Encoders por hardware, na ordem em que valem a pena neste sistema.
    fn hardware_encoders(self) -> Vec<String> {
        let base = match self {
            Self::H264 => "h264",
            Self::H265 => "hevc",
            Self::Av1 => "av1",
        };
        let vendors: &[&str] = if cfg!(target_os = "macos") {
            &["videotoolbox"]
        } else {
            &["nvenc", "qsv", "amf"]
        };
        vendors.iter().map(|v| format!("{base}_{v}")).collect()
    }

    /// Ajuste do CRF em relação ao x264: x265 e AV1 usam escalas diferentes.
    fn crf_offset(self) -> i64 {
        match self {
            Self::H264 => 0,
            Self::H265 => 5,
            Self::Av1 => 10,
        }
    }
}

/// Como escolher o encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum EncoderMode {
    /// Placa de vídeo se houver (3 a 10x mais rápido); senão software.
    #[default]
    Auto,
    /// Só CPU (libx264, libx265, SVT-AV1): qualidade máxima por byte.
    Software,
    /// Só placa de vídeo; erro se não houver.
    Hardware,
}

/// Encoder escolhido para a exportação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderChoice {
    pub name: String,
    pub hardware: bool,
}

/// Escolhe o encoder entre os que o ffmpeg tem, respeitando o modo pedido.
///
/// Devolve a lista de tentativas em ordem: com `auto`, hardware primeiro e o
/// software como reserva caso a placa falhe na hora.
///
/// # Errors
///
/// [`ErrorCode::Unavailable`] se nenhum encoder do codec existir no ffmpeg
/// ou se `hardware` foi exigido e não há nenhum.
pub fn choose_encoders(
    available: &HashSet<String>,
    codec: VideoCodec,
    mode: EncoderMode,
) -> ToolResult<Vec<EncoderChoice>> {
    let hardware: Vec<EncoderChoice> = codec
        .hardware_encoders()
        .into_iter()
        .filter(|name| available.contains(name))
        .map(|name| EncoderChoice {
            name,
            hardware: true,
        })
        .collect();
    let software: Vec<EncoderChoice> = codec
        .software_encoders()
        .iter()
        .filter(|name| available.contains(**name))
        .map(|name| EncoderChoice {
            name: (*name).to_string(),
            hardware: false,
        })
        .collect();
    let choices: Vec<EncoderChoice> = match mode {
        EncoderMode::Software => software,
        EncoderMode::Hardware => hardware,
        EncoderMode::Auto => hardware.into_iter().chain(software).collect(),
    };
    if choices.is_empty() {
        let (message, hint) = if mode == EncoderMode::Hardware {
            (
                format!(
                    "Nenhum encoder de hardware para {} neste ffmpeg/sistema.",
                    codec.as_str()
                ),
                "Use encoder='auto' ou 'software'.",
            )
        } else {
            (
                format!("Este ffmpeg não tem encoder para {}.", codec.as_str()),
                "Use codec='h264' ou deixe MCP_AUTO_DOWNLOAD=true para baixar um ffmpeg completo.",
            )
        };
        return Err(ToolError::with_hint(message, ErrorCode::Unavailable, hint));
    }
    Ok(choices)
}

/// Encoders de hardware que já falharam nesta sessão: listados pelo ffmpeg, mas
/// sem placa ou driver que os sustente. Não vale a pena tentá-los de novo.
fn failed_encoders() -> &'static Mutex<HashSet<String>> {
    static FAILED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    FAILED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn remember_failure(name: &str) {
    if let Ok(mut set) = failed_encoders().lock() {
        set.insert(name.to_string());
    }
}

fn known_failure(name: &str) -> bool {
    failed_encoders()
        .lock()
        .map(|set| set.contains(name))
        .unwrap_or(false)
}

/// Argumentos de qualidade e perfil próprios de cada encoder.
///
/// Todos partem do mesmo `crf` do x264 (menor = melhor) e o traduzem para a
/// escala do encoder: CRF do x265 e do AV1, CQ da NVIDIA, `global_quality` da
/// Intel, QP da AMD e `q:v` (1-100, maior = melhor) do VideoToolbox.
pub fn encoder_args(encoder: &str, codec: VideoCodec, crf: i64) -> Vec<OsString> {
    let crf = (crf + codec.crf_offset()).clamp(0, 51);
    let quality = crf.to_string();
    match encoder {
        "libx264" => ffargs![
            "-preset",
            "medium",
            "-profile:v",
            "high",
            "-level",
            "4.2",
            "-crf",
            quality
        ],
        "libx265" => ffargs![
            "-preset",
            "medium",
            "-crf",
            quality,
            "-tag:v",
            "hvc1",
            "-x265-params",
            "log-level=error"
        ],
        "libsvtav1" => ffargs!["-preset", "6", "-crf", quality, "-svtav1-params", "tune=0"],
        "libaom-av1" => ffargs![
            "-cpu-used",
            "6",
            "-crf",
            quality,
            "-b:v",
            "0",
            "-row-mt",
            "1"
        ],
        name if name.ends_with("_nvenc") => {
            let mut args = ffargs![
                "-preset",
                "p5",
                "-tune",
                "hq",
                "-rc",
                "vbr",
                "-cq",
                quality,
                "-b:v",
                "0",
                "-spatial-aq",
                "1"
            ];
            match codec {
                VideoCodec::H264 => args.extend(ffargs!["-profile:v", "high"]),
                VideoCodec::H265 => args.extend(ffargs!["-tag:v", "hvc1"]),
                VideoCodec::Av1 => {}
            }
            args
        }
        name if name.ends_with("_qsv") => {
            let mut args = ffargs!["-preset", "medium", "-global_quality", quality];
            if codec == VideoCodec::H265 {
                args.extend(ffargs!["-tag:v", "hvc1"]);
            }
            args
        }
        name if name.ends_with("_amf") => {
            let mut args =
                ffargs!["-quality", "quality", "-rc", "cqp", "-qp_i", quality, "-qp_p", quality];
            if codec == VideoCodec::H265 {
                args.extend(ffargs!["-tag:v", "hvc1"]);
            }
            args
        }
        name if name.ends_with("_videotoolbox") => {
            let q = (100 - crf * 2).clamp(1, 100).to_string();
            let mut args = ffargs!["-q:v", q];
            match codec {
                VideoCodec::H264 => args.extend(ffargs!["-profile:v", "high"]),
                VideoCodec::H265 => args.extend(ffargs!["-tag:v", "hvc1"]),
                VideoCodec::Av1 => {}
            }
            args
        }
        _ => ffargs!["-crf", quality],
    }
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    #[default]
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
    /// Codec de vídeo gravado: h264, h265 ou av1.
    pub video_codec: VideoCodec,
    /// Encoder do ffmpeg que fez a codificação (libx264, h264_nvenc...).
    pub encoder: String,
    /// true quando a codificação rodou na placa de vídeo.
    pub hardware: bool,
    /// Tempo gasto na codificação, em segundos.
    pub encode_seconds: f64,
    pub warnings: Vec<String>,
}

/// Opções de codificação além da plataforma.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportOptions {
    pub quality: Quality,
    pub codec: VideoCodec,
    pub encoder: EncoderMode,
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

/// Aviso quando a plataforma não aceita (ou aceita mal) o codec escolhido.
pub fn codec_warning(platform: Platform, codec: VideoCodec) -> Option<String> {
    match (platform, codec) {
        (Platform::Twitter, VideoCodec::H265 | VideoCodec::Av1) => {
            Some("O X (Twitter) só aceita H.264. Exporte de novo com codec='h264'.".to_string())
        }
        (Platform::Tiktok | Platform::InstagramReels, VideoCodec::Av1) => Some(
            "TikTok e Instagram não garantem AV1 no upload. Prefira codec='h264' ou 'h265'."
                .to_string(),
        ),
        _ => None,
    }
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
    options: ExportOptions,
    output: Option<&str>,
) -> ToolResult<ExportForPlatformResult> {
    let ExportOptions {
        quality,
        codec,
        encoder,
    } = options;
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
            "Todas as plataformas aceitam MP4.",
        ));
    }
    let choices = choose_encoders(&runtime.ffmpeg.video_encoders()?, codec, encoder)?;
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
    let mut notes = warnings(&preset, width, height, info.duration);
    if let Some(note) = codec_warning(platform, codec) {
        notes.push(note);
    }

    // Tenta os encoders na ordem: um de hardware listado pode falhar na hora
    // (sem placa, driver antigo); nesse caso cai para o próximo e avisa.
    let mut last_error: Option<ToolError> = None;
    let mut chosen: Option<(EncoderChoice, f64)> = None;
    let mut failed: Vec<String> = Vec::new();
    for choice in choices {
        if choice.hardware && encoder == EncoderMode::Auto && known_failure(&choice.name) {
            failed.push(choice.name);
            continue;
        }
        let mut args = ffargs!["-i", source, "-vf", video_filter, "-c:v", choice.name];
        args.extend(encoder_args(&choice.name, codec, crf));
        args.extend(ffargs![
            "-g",
            ((fps * 2.0).round_ties_even() as i64).to_string(),
            "-pix_fmt",
            "yuv420p"
        ]);
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
        let started = Instant::now();
        match runtime.ffmpeg.run(&args) {
            Ok(_) => {
                chosen = Some((choice, started.elapsed().as_secs_f64()));
                break;
            }
            Err(error) => {
                if error.code != ErrorCode::FfmpegFailed {
                    return Err(error);
                }
                if choice.hardware {
                    remember_failure(&choice.name);
                }
                failed.push(choice.name);
                last_error = Some(error);
            }
        }
    }
    if !failed.is_empty() && chosen.is_some() {
        notes.push(format!(
            "Sem placa de vídeo utilizável ({} sem resposta); a codificação foi feita por \
             software.",
            failed.join(", ")
        ));
    }
    let Some((choice, encode_seconds)) = chosen else {
        let error = last_error.unwrap_or_else(|| {
            ToolError::new("Nenhum encoder disponível.", ErrorCode::Unavailable)
        });
        return Err(ToolError::with_hint(
            error.message,
            error.code,
            "Tente encoder='software' ou codec='h264'.",
        ));
    };
    let size_bytes = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
    Ok(ExportForPlatformResult {
        output: runtime.workspace.relative(&target),
        platform,
        width,
        height,
        fps: round_to(fps, 3),
        duration: round_to(info.duration, 3),
        size_bytes,
        video_codec: codec,
        encoder: choice.name,
        hardware: choice.hardware,
        encode_seconds: round_to(encode_seconds, 2),
        warnings: notes,
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
    options: ExportOptions,
    output: Option<&str>,
    background: bool,
) -> ToolResult<MaybeJob<ExportForPlatformResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        let output = output.map(str::to_string);
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("export_for_platform", move || {
                do_export(&runtime_job, &path, platform, options, output.as_deref())
            }),
        ));
    }
    Ok(MaybeJob::Done(do_export(
        runtime, path, platform, options, output,
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
    /// "h264" (padrão, aceito em toda plataforma), "h265" (mesma qualidade com
    /// ~40% menos bytes; TikTok, Instagram e YouTube) ou "av1" (só YouTube).
    #[serde(default)]
    pub codec: VideoCodec,
    /// "auto" (padrão: usa a placa de vídeo se houver, 3 a 10x mais rápido, e cai
    /// para software se ela falhar), "software" (só CPU, melhor qualidade por
    /// byte) ou "hardware" (exige placa; erro se não houver).
    #[serde(default)]
    pub encoder: EncoderMode,
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
         Último passo da edição. Aplica o preset de resolução máxima, fps, qualidade \
         (CRF) e bitrate de áudio da plataforma e gera MP4 com faststart (começa a \
         tocar antes de baixar). codec escolhe H.264 (padrão), H.265 ou AV1; encoder \
         'auto' usa a placa de vídeo quando existe (NVIDIA, Intel, AMD, Apple) e cai \
         para software sozinho se ela falhar, então o export é rápido sem risco. Nunca \
         amplia a imagem nem muda a proporção: para virar vertical use smart_crop ou \
         apply_template antes. Devolve o encoder usado, o tempo de codificação e \
         warnings quando algo vai contra as regras da plataforma (orientação errada, \
         duração acima do limite, resolução baixa, codec não aceito), com a tool que \
         resolve.\n\n\
         Presets: tiktok e instagram_reels (1080x1920, 30 fps), youtube_shorts \
         (1080x1920, 60 fps), youtube (1920x1080), youtube_4k (3840x2160), twitter \
         (1920x1200, 140 s). Para volume padronizado rode normalize_audio antes.",
        move |params: Params| {
            guarded(export_for_platform(
                &runtime,
                &params.path,
                params.platform,
                ExportOptions {
                    quality: params.quality,
                    codec: params.codec,
                    encoder: params.encoder,
                },
                params.output.as_deref(),
                params.background,
            ))
        },
    );
}
