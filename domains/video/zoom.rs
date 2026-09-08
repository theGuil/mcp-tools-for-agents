//! Tool `zoom_video`: zoom de impacto (punch-in) ou zoom progressivo (Ken Burns).

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::jobs::MaybeJob;
use crate::core::numbers::{format_g, round_to};
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

/// Modo do zoom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ZoomMode {
    Punch,
    In,
    Out,
}

impl ZoomMode {
    /// Nome do modo como aparece no JSON e no nome do arquivo.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Punch => "punch",
            Self::In => "in",
            Self::Out => "out",
        }
    }
}

const MIN_ZOOM: f64 = 1.05;
const MAX_ZOOM: f64 = 4.0;

/// Vídeo gerado com o zoom.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ZoomVideoResult {
    pub output: String,
    pub mode: ZoomMode,
    pub zoom: f64,
    pub start: f64,
    pub end: f64,
    pub focus_x: f64,
    pub focus_y: f64,
}

fn validate(zoom: f64, start: f64, end: f64, focus_x: f64, focus_y: f64) -> ToolResult<()> {
    if !(MIN_ZOOM..=MAX_ZOOM).contains(&zoom) {
        return Err(ToolError::with_hint(
            format!(
                "zoom deve estar entre {} e {}.",
                format_g(MIN_ZOOM),
                format_g(MAX_ZOOM)
            ),
            ErrorCode::InvalidArgument,
            "1.2 é um punch-in sutil, 1.5 forte, 2 bem fechado.",
        ));
    }
    if start < 0.0 || end <= start {
        return Err(ToolError::with_hint(
            format!("Intervalo inválido: start={start}, end={end}."),
            ErrorCode::InvalidArgument,
            "start deve ser >= 0 e end maior que start, em segundos.",
        ));
    }
    if !((0.0..=1.0).contains(&focus_x) && (0.0..=1.0).contains(&focus_y)) {
        return Err(ToolError::with_hint(
            "focus_x e focus_y devem estar entre 0 e 1.",
            ErrorCode::InvalidArgument,
            "0.5,0.5 é o centro; 0.5,0.3 mira um pouco acima (rosto de quem fala).",
        ));
    }
    Ok(())
}

/// Expressão do fator de zoom em função do tempo `t` para o filtro `crop`.
pub fn zoom_expression(mode: ZoomMode, zoom: f64, start: f64, end: f64) -> String {
    let length = end - start;
    let progress = format!("((t-{start:.3})/{length:.3})");
    let active = match mode {
        ZoomMode::Punch => format!("{zoom:.4}"),
        ZoomMode::In => format!("(1+({zoom:.4}-1)*{progress})"),
        ZoomMode::Out => format!("({zoom:.4}-({zoom:.4}-1)*{progress})"),
    };
    format!("if(between(t\\,{start:.3}\\,{end:.3})\\,{active}\\,1)")
}

#[allow(clippy::too_many_arguments)] // os argumentos são a interface da tool
fn do_zoom(
    runtime: &Runtime,
    path: &str,
    mode: ZoomMode,
    zoom: f64,
    start: f64,
    end: f64,
    focus_x: f64,
    focus_y: f64,
) -> ToolResult<ZoomVideoResult> {
    let source = runtime.workspace.existing(path)?;
    validate(zoom, start, end, focus_x, focus_y)?;
    let info = runtime.ffmpeg.probe(&source)?;
    let (Some(width), Some(height)) = (info.width, info.height) else {
        return Err(no_video(path));
    };
    if !info.has_video {
        return Err(no_video(path));
    }
    if start >= info.duration {
        return Err(ToolError::with_hint(
            format!(
                "start={start}s ultrapassa a duração do vídeo ({:.2}s).",
                info.duration
            ),
            ErrorCode::InvalidArgument,
            "Use probe_video para conferir a duração.",
        ));
    }
    let end = end.min(info.duration);
    let z = zoom_expression(mode, zoom, start, end);
    // Recorta uma janela de tamanho iw/z centrada no ponto de foco (limitada às bordas)
    // e amplia de volta ao tamanho original. Resolução e proporção não mudam.
    let crop = format!(
        "crop=w='iw/({z})':h='ih/({z})'\
         :x='clip(iw*{focus_x:.4}-ow/2\\,0\\,iw-ow)'\
         :y='clip(ih*{focus_y:.4}-oh/2\\,0\\,ih-oh)'"
    );
    let filter_expr = format!("{crop},scale={width}:{height}:flags=lanczos,format=yuv420p");
    let output = runtime
        .workspace
        .output_for(&source, &format!("zoom_{}", mode.as_str()), None);
    let mut args = ffargs![
        "-i",
        source,
        "-vf",
        filter_expr,
        "-c:v",
        "libx264",
        "-preset",
        "fast"
    ];
    if info.has_audio {
        args.extend(ffargs!["-c:a", "copy"]);
    }
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(ZoomVideoResult {
        output: runtime.workspace.relative(&output),
        mode,
        zoom,
        start,
        end: round_to(end, 3),
        focus_x,
        focus_y,
    })
}

fn no_video(path: &str) -> ToolError {
    ToolError::with_hint(
        format!("'{path}' não tem trilha de vídeo."),
        ErrorCode::InvalidArgument,
        "zoom_video só se aplica a vídeos.",
    )
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Argumento inválido, arquivo inexistente ou falha do ffmpeg.
#[allow(clippy::too_many_arguments)] // os argumentos são a interface da tool
pub fn zoom_video(
    runtime: &Arc<Runtime>,
    path: &str,
    start: f64,
    end: f64,
    mode: ZoomMode,
    zoom: f64,
    focus_x: f64,
    focus_y: f64,
    background: bool,
) -> ToolResult<MaybeJob<ZoomVideoResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        return Ok(MaybeJob::Job(runtime.jobs.submit(
            "zoom_video",
            move || {
                do_zoom(
                    &runtime_job,
                    &path,
                    mode,
                    zoom,
                    start,
                    end,
                    focus_x,
                    focus_y,
                )
            },
        )));
    }
    Ok(MaybeJob::Done(do_zoom(
        runtime, path, mode, zoom, start, end, focus_x, focus_y,
    )?))
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// Segundo em que o zoom começa.
    pub start: f64,
    /// Segundo em que o zoom termina.
    pub end: f64,
    /// "punch", "in" ou "out".
    #[serde(default = "default_mode")]
    pub mode: ZoomMode,
    /// Fator de zoom entre 1.05 e 4. 1.3 é um bom padrão.
    #[serde(default = "default_zoom")]
    pub zoom: f64,
    /// Ponto horizontal de foco, de 0 (esquerda) a 1 (direita).
    #[serde(default = "default_focus")]
    pub focus_x: f64,
    /// Ponto vertical de foco, de 0 (topo) a 1 (base).
    #[serde(default = "default_focus")]
    pub focus_y: f64,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

fn default_mode() -> ZoomMode {
    ZoomMode::Punch
}

fn default_zoom() -> f64 {
    1.3
}

fn default_focus() -> f64 {
    0.5
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "zoom_video",
        "Aplica zoom em um trecho do vídeo: punch-in de impacto ou zoom progressivo.\n\n\
         É o efeito mais usado em cortes de podcast e TikTok para dar ênfase a uma \
         frase ou esconder um jump cut. Modos:\n\
         - \"punch\": a imagem fecha de uma vez no instante start e volta ao normal em \
         end (zoom seco de ênfase). Use zoom entre 1.2 e 1.5.\n\
         - \"in\": aproxima aos poucos de 1x até zoom entre start e end (Ken Burns).\n\
         - \"out\": começa em zoom e afasta até 1x entre start e end.\n\
         A resolução do vídeo não muda. focus_x/focus_y (0 a 1) dizem para onde o \
         zoom mira: 0.5,0.5 é o centro, 0.5,0.3 mira mais alto (rosto). Para \
         vários zooms encadeie chamadas no arquivo gerado. O original não é modificado.",
        move |params: Params| {
            guarded(zoom_video(
                &runtime,
                &params.path,
                params.start,
                params.end,
                params.mode,
                params.zoom,
                params.focus_x,
                params.focus_y,
                params.background,
            ))
        },
    );
}
