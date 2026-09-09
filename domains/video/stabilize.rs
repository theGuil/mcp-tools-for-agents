//! Tool `stabilize_video`: tira o tremor de câmera na mão, com o vid.stab em dois passes.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::fonts::escape_filter_path;
use crate::core::jobs::MaybeJob;
use crate::core::numbers::format_g;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

/// O que fazer com a borda que sobra quando o quadro é deslocado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CropMode {
    Black,
    Keep,
}

impl CropMode {
    /// Nome do modo como o agente informa e como o vid.stab espera.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Black => "black",
            Self::Keep => "keep",
        }
    }
}

const MIN_SMOOTHING: i64 = 1;
const MAX_SMOOTHING: i64 = 100;
const MIN_SHAKINESS: i64 = 1;
const MAX_SHAKINESS: i64 = 10;
const MAX_ZOOM: f64 = 50.0;
/// Precisão do passe de análise. Alta de propósito: vídeo baixado de plataforma
/// vem recomprimido, e artefato de macrobloco engana o detector de movimento.
const DETECT_ACCURACY: i64 = 15;

/// Vídeo gerado com o tremor corrigido.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StabilizeVideoResult {
    pub output: String,
    pub smoothing: i64,
    pub shakiness: i64,
    pub crop: CropMode,
    /// Zoom fixo aplicado, em porcentagem. `None` quando o zoom foi adaptativo.
    pub zoom: Option<f64>,
}

fn validate(smoothing: i64, shakiness: i64, zoom: Option<f64>) -> ToolResult<()> {
    if !(MIN_SMOOTHING..=MAX_SMOOTHING).contains(&smoothing) {
        return Err(ToolError::with_hint(
            format!("smoothing deve estar entre {MIN_SMOOTHING} e {MAX_SMOOTHING}."),
            ErrorCode::InvalidArgument,
            "10 a 15 corrige tremor de mão sem fazer a imagem flutuar.",
        ));
    }
    if !(MIN_SHAKINESS..=MAX_SHAKINESS).contains(&shakiness) {
        return Err(ToolError::with_hint(
            format!("shakiness deve estar entre {MIN_SHAKINESS} e {MAX_SHAKINESS}."),
            ErrorCode::InvalidArgument,
            "6 serve para a maioria; suba para 8 ou 10 se o tremor for forte.",
        ));
    }
    if let Some(value) = zoom {
        if !(-MAX_ZOOM..=MAX_ZOOM).contains(&value) {
            return Err(ToolError::with_hint(
                format!(
                    "zoom deve estar entre -{} e {}.",
                    format_g(MAX_ZOOM),
                    format_g(MAX_ZOOM)
                ),
                ErrorCode::InvalidArgument,
                "Omita zoom para o vid.stab escolher sozinho o mínimo necessário.",
            ));
        }
    }
    Ok(())
}

/// Confere que este ffmpeg tem o vid.stab compilado.
fn require_vidstab(runtime: &Runtime) -> ToolResult<()> {
    let filters = runtime.ffmpeg.filters()?;
    let faltando: Vec<&str> = ["vidstabdetect", "vidstabtransform"]
        .into_iter()
        .filter(|name| !filters.contains(*name))
        .collect();
    if faltando.is_empty() {
        return Ok(());
    }
    Err(ToolError::with_hint(
        format!(
            "Este ffmpeg não tem o vid.stab compilado (falta {}).",
            faltando.join(", ")
        ),
        ErrorCode::Unavailable,
        "Apague FFMPEG_BIN para o servidor baixar um ffmpeg completo, ou instale \
         um build feito com --enable-libvidstab.",
    ))
}

fn do_stabilize(
    runtime: &Runtime,
    path: &str,
    smoothing: i64,
    shakiness: i64,
    zoom: Option<f64>,
    crop: CropMode,
) -> ToolResult<StabilizeVideoResult> {
    let source = runtime.workspace.existing(path)?;
    validate(smoothing, shakiness, zoom)?;
    require_vidstab(runtime)?;
    let info = runtime.ffmpeg.probe(&source)?;
    if !info.has_video {
        return Err(ToolError::with_hint(
            format!("'{path}' não tem trilha de vídeo."),
            ErrorCode::InvalidArgument,
            "stabilize_video só se aplica a vídeos.",
        ));
    }
    // Passe 1: mede o movimento quadro a quadro e grava as transformações num
    // arquivo temporário. Não produz vídeo, por isso a saída vai para null.
    let transforms = tempfile::Builder::new()
        .prefix("vidstab")
        .suffix(".trf")
        .tempfile()
        .map_err(|error| {
            ToolError::with_hint(
                format!("Não foi possível criar o arquivo de análise: {error}"),
                ErrorCode::FfmpegFailed,
                "Confira o espaço em disco e a permissão da pasta temporária.",
            )
        })?;
    let trf = escape_filter_path(transforms.path());
    let detect =
        format!("vidstabdetect=shakiness={shakiness}:accuracy={DETECT_ACCURACY}:result={trf}");
    runtime.ffmpeg.run(ffargs![
        "-i", source, "-vf", detect, "-an", "-f", "null", "-"
    ])?;
    // Passe 2: aplica a trajetória suavizada. optzoom=2 escolhe o menor zoom
    // que esconde as bordas em cada trecho, em vez de um zoom fixo no vídeo
    // inteiro: preserva mais imagem, o que importa em fonte de baixa resolução.
    let zoom_args = match zoom {
        Some(value) => format!("optzoom=0:zoom={}", format_g(value)),
        None => "optzoom=2".to_string(),
    };
    // A reamostragem bicúbica amacia a imagem; o unsharp em seguida devolve a
    // definição, como recomenda a própria documentação do vid.stab.
    let transform = format!(
        "vidstabtransform=input={trf}:smoothing={smoothing}:{zoom_args}\
         :interpol=bicubic:crop={},unsharp=5:5:0.8:3:3:0.4",
        crop.as_str()
    );
    let output = runtime.workspace.output_for(&source, "stabilized", None);
    let mut args = ffargs!["-i", source, "-vf", transform, "-c:v", "libx264", "-preset", "fast"];
    if info.has_audio {
        args.extend(ffargs!["-c:a", "copy"]);
    }
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(StabilizeVideoResult {
        output: runtime.workspace.relative(&output),
        smoothing,
        shakiness,
        crop,
        zoom,
    })
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Argumento inválido, arquivo inexistente, ffmpeg sem vid.stab ou falha do ffmpeg.
pub fn stabilize_video(
    runtime: &Arc<Runtime>,
    path: &str,
    smoothing: i64,
    shakiness: i64,
    zoom: Option<f64>,
    crop: CropMode,
    background: bool,
) -> ToolResult<MaybeJob<StabilizeVideoResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        return Ok(MaybeJob::Job(
            runtime.jobs.submit("stabilize_video", move || {
                do_stabilize(&runtime_job, &path, smoothing, shakiness, zoom, crop)
            }),
        ));
    }
    Ok(MaybeJob::Done(do_stabilize(
        runtime, path, smoothing, shakiness, zoom, crop,
    )?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// Quadros de suavização da trajetória, de 1 a 100. 12 é um bom padrão.
    #[serde(default = "default_smoothing")]
    pub smoothing: i64,
    /// Quanto o vídeo treme, de 1 a 10. 6 serve para a maioria.
    #[serde(default = "default_shakiness")]
    pub shakiness: i64,
    /// Zoom fixo em porcentagem para esconder as bordas, ex: 5.
    /// Omitido = zoom adaptativo, o mínimo necessário em cada trecho.
    #[serde(default)]
    pub zoom: Option<f64>,
    /// "black" deixa a borda preta, "keep" preenche com o quadro anterior.
    #[serde(default = "default_crop")]
    pub crop: CropMode,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_smoothing() -> i64 {
    12
}

fn default_shakiness() -> i64 {
    6
}

fn default_crop() -> CropMode {
    CropMode::Black
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "stabilize_video",
        "Remove o tremor de câmera na mão, deixando a imagem parada como se \
         estivesse num tripé.\n\n\
         Use quando o vídeo balança ao ponto de incomodar: gravação de celular, \
         câmera andando, mão trêmula. Faz dois passes (analisa o movimento, \
         depois corrige), por isso é lento: em vídeo de mais de um minuto passe \
         background=true e acompanhe com job_status.\n\
         Parâmetros que importam:\n\
         - smoothing: quantos quadros de suavização. 12 é o padrão; acima de 30 \
         a imagem começa a \"flutuar\" em vez de ficar firme.\n\
         - shakiness: o quanto treme, de 1 a 10. Suba para 8 ou 10 se o tremor \
         for forte e o resultado ainda balançar.\n\
         - zoom: corrigir tremor desloca o quadro e abre bordas. Por padrão o \
         zoom é adaptativo e usa o mínimo necessário em cada trecho, o que \
         preserva mais imagem. Passe um número (ex: 5) para fixar.\n\
         - crop: \"black\" deixa a borda preta, \"keep\" preenche com o quadro \
         anterior.\n\
         Atenção em vídeo de baixa resolução: o zoom come definição real, então \
         prefira o adaptativo e estabilize ANTES de qualquer upscale. O original \
         não é modificado; o vídeo é re-encodado com o sufixo _stabilized.",
        move |params: Params| {
            guarded(stabilize_video(
                &runtime,
                &params.path,
                params.smoothing,
                params.shakiness,
                params.zoom,
                params.crop,
                params.background,
            ))
        },
    );
}
