//! Tools `list_templates` e `apply_template`: formatos prontos para redes sociais.

use std::path::Path;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::core::fonts::{escape_drawtext, escape_filter_path, require_font};
use crate::core::jobs::MaybeJob;
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

const MAX_TITLE: usize = 120;
const INTRO_SECONDS: f64 = 3.0;

/// Nome de um template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TemplateName {
    Shorts,
    Square,
    Landscape,
    IntroTitle,
    Watermark,
}

impl TemplateName {
    /// Nome do template como o agente informa.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shorts => "shorts",
            Self::Square => "square",
            Self::Landscape => "landscape",
            Self::IntroTitle => "intro_title",
            Self::Watermark => "watermark",
        }
    }

    /// Tamanho fixo do canvas, para os formatos de rede social.
    fn canvas(self) -> Option<(i64, i64)> {
        match self {
            Self::Shorts => Some((1080, 1920)),
            Self::Square => Some((1080, 1080)),
            Self::Landscape => Some((1920, 1080)),
            Self::IntroTitle | Self::Watermark => None,
        }
    }
}

/// Descrição de um template para o agente escolher.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TemplateInfo {
    pub name: TemplateName,
    pub description: &'static str,
    pub output_size: &'static str,
    pub requires_title: bool,
    pub requires_logo: bool,
}

/// Templates disponíveis, na ordem em que são listados.
pub const TEMPLATES: &[TemplateInfo] = &[
    TemplateInfo {
        name: TemplateName::Shorts,
        description: "Vertical 9:16 para Shorts, Reels e TikTok. O vídeo fica centralizado \
                      sobre uma versão desfocada dele mesmo. title opcional no topo.",
        output_size: "1080x1920",
        requires_title: false,
        requires_logo: false,
    },
    TemplateInfo {
        name: TemplateName::Square,
        description: "Quadrado 1:1 para feed. Fundo desfocado, title opcional no topo.",
        output_size: "1080x1080",
        requires_title: false,
        requires_logo: false,
    },
    TemplateInfo {
        name: TemplateName::Landscape,
        description: "Horizontal 16:9 Full HD. Redimensiona e preenche bordas com preto.",
        output_size: "1920x1080",
        requires_title: false,
        requires_logo: false,
    },
    TemplateInfo {
        name: TemplateName::IntroTitle,
        description: "Mantém o tamanho e mostra o title em destaque, sobre faixa escura, \
                      nos 3 primeiros segundos.",
        output_size: "original",
        requires_title: true,
        requires_logo: false,
    },
    TemplateInfo {
        name: TemplateName::Watermark,
        description: "Mantém o tamanho e coloca a imagem logo_path no canto inferior direito.",
        output_size: "original",
        requires_title: false,
        requires_logo: true,
    },
];

/// Templates disponíveis.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ListTemplatesResult {
    pub templates: Vec<TemplateInfo>,
}

/// Vídeo gerado pelo template.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ApplyTemplateResult {
    pub output: String,
    pub template: TemplateName,
    pub width: i64,
    pub height: i64,
    pub title: Option<String>,
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Nunca falha; o `Result` mantém a assinatura padrão das tools.
pub fn list_templates(_runtime: &Runtime) -> ToolResult<ListTemplatesResult> {
    Ok(ListTemplatesResult {
        templates: TEMPLATES.to_vec(),
    })
}

fn title_filter(title: &str, font: &Path, font_size: i64, y: &str, enable: Option<&str>) -> String {
    let mut expr = format!(
        "drawtext=fontfile='{}':text={}:fontsize={font_size}:fontcolor=white:borderw=3\
         :bordercolor=black@0.8:x=(w-text_w)/2:y={y}:line_spacing=10",
        escape_filter_path(font),
        escape_drawtext(title)
    );
    if let Some(enable) = enable {
        expr.push_str(&format!(":enable='{enable}'"));
    }
    expr
}

fn blurred_canvas_filter(width: i64, height: i64, title: Option<&str>) -> ToolResult<String> {
    let mut chain = format!(
        "[0:v]split[bg][fg];\
         [bg]scale={width}:{height}:force_original_aspect_ratio=increase,\
         crop={width}:{height},boxblur=20:5[bgb];\
         [fg]scale={width}:{height}:force_original_aspect_ratio=decrease[fgs];\
         [bgb][fgs]overlay=(W-w)/2:(H-h)/2"
    );
    if let Some(title) = title.filter(|t| !t.is_empty()) {
        chain.push(',');
        chain.push_str(&title_filter(
            title,
            &require_font()?,
            (height as f64 * 0.035) as i64,
            "h*0.06",
            None,
        ));
    }
    chain.push_str(",format=yuv420p[vout]");
    Ok(chain)
}

fn landscape_filter(width: i64, height: i64, title: Option<&str>) -> ToolResult<String> {
    let mut chain = format!(
        "[0:v]scale={width}:{height}:force_original_aspect_ratio=decrease,\
         pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:black"
    );
    if let Some(title) = title.filter(|t| !t.is_empty()) {
        chain.push(',');
        chain.push_str(&title_filter(
            title,
            &require_font()?,
            (height as f64 * 0.05) as i64,
            "h*0.06",
            None,
        ));
    }
    chain.push_str(",format=yuv420p[vout]");
    Ok(chain)
}

fn intro_filter(title: &str, height: i64) -> ToolResult<String> {
    let enable = format!("lte(t\\,{INTRO_SECONDS})");
    let boxf =
        format!("drawbox=x=0:y=ih*0.35:w=iw:h=ih*0.3:color=black@0.6:t=fill:enable='{enable}'");
    let text = title_filter(
        title,
        &require_font()?,
        ((height as f64 * 0.07) as i64).max(24),
        "(h-text_h)/2",
        Some(&enable),
    );
    Ok(format!("[0:v]{boxf},{text},format=yuv420p[vout]"))
}

fn watermark_filter(height: i64) -> String {
    let logo_h = ((height as f64 * 0.08) as i64).max(24);
    let margin = (height as f64 * 0.03) as i64;
    format!(
        "[1:v]scale=-1:{logo_h}[logo];\
         [0:v][logo]overlay=W-w-{margin}:H-h-{margin},format=yuv420p[vout]"
    )
}

fn validate(
    template: TemplateName,
    title: Option<&str>,
    logo_path: Option<&str>,
) -> ToolResult<&'static TemplateInfo> {
    let Some(spec) = TEMPLATES.iter().find(|t| t.name == template) else {
        return Err(ToolError::with_hint(
            format!("Template '{}' não existe.", template.as_str()),
            ErrorCode::InvalidArgument,
            "Use list_templates para ver os nomes válidos.",
        ));
    };
    if spec.requires_title && title.is_none_or(|t| t.trim().is_empty()) {
        return Err(ToolError::with_hint(
            format!("Template '{}' exige title.", template.as_str()),
            ErrorCode::InvalidArgument,
            "Passe o texto do título em title.",
        ));
    }
    if title.is_some_and(|t| t.chars().count() > MAX_TITLE) {
        return Err(ToolError::with_hint(
            format!("title deve ter até {MAX_TITLE} caracteres."),
            ErrorCode::InvalidArgument,
            "Encurte o título ou use add_text_overlay para textos longos.",
        ));
    }
    if spec.requires_logo && logo_path.is_none_or(str::is_empty) {
        return Err(ToolError::with_hint(
            format!("Template '{}' exige logo_path.", template.as_str()),
            ErrorCode::InvalidArgument,
            "Informe uma imagem PNG do workspace em logo_path.",
        ));
    }
    Ok(spec)
}

fn do_apply(
    runtime: &Runtime,
    path: &str,
    template: TemplateName,
    title: Option<&str>,
    logo_path: Option<&str>,
) -> ToolResult<ApplyTemplateResult> {
    let source = runtime.workspace.existing(path)?;
    validate(template, title, logo_path)?;
    let title: Option<String> = title
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    let info = runtime.ffmpeg.probe(&source)?;
    let (Some(src_w), Some(src_h)) = (info.width, info.height) else {
        return Err(no_video(path));
    };
    if !info.has_video {
        return Err(no_video(path));
    }
    let mut inputs = ffargs!["-i", source];
    let (width, height, filter_expr) = if let Some((width, height)) = template.canvas() {
        let expr = if template == TemplateName::Landscape {
            landscape_filter(width, height, title.as_deref())?
        } else {
            blurred_canvas_filter(width, height, title.as_deref())?
        };
        (width, height, expr)
    } else if template == TemplateName::IntroTitle {
        (
            src_w,
            src_h,
            intro_filter(title.as_deref().unwrap_or(""), src_h)?,
        )
    } else {
        let logo = runtime.workspace.existing(logo_path.unwrap_or(""))?;
        inputs.extend(ffargs!["-i", logo]);
        (src_w, src_h, watermark_filter(src_h))
    };
    let output = runtime
        .workspace
        .output_for(&source, template.as_str(), None);
    let audio_args = if info.has_audio {
        ffargs!["-map", "0:a?", "-c:a", "copy"]
    } else {
        ffargs!["-an"]
    };
    let mut args = inputs;
    args.extend(ffargs!["-filter_complex", filter_expr, "-map", "[vout]"]);
    args.extend(audio_args);
    args.extend(ffargs!["-c:v", "libx264", "-preset", "fast"]);
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    Ok(ApplyTemplateResult {
        output: runtime.workspace.relative(&output),
        template,
        width,
        height,
        title,
    })
}

fn no_video(path: &str) -> ToolError {
    ToolError::with_hint(
        format!("'{path}' não tem trilha de vídeo."),
        ErrorCode::InvalidArgument,
        "Templates só se aplicam a vídeos.",
    )
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Template inválido, exigência não atendida, arquivo inexistente ou falha do ffmpeg.
pub fn apply_template(
    runtime: &Arc<Runtime>,
    path: &str,
    template: TemplateName,
    title: Option<&str>,
    logo_path: Option<&str>,
    background: bool,
) -> ToolResult<MaybeJob<ApplyTemplateResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        let title = title.map(str::to_string);
        let logo_path = logo_path.map(str::to_string);
        return Ok(MaybeJob::Job(runtime.jobs.submit(
            "apply_template",
            move || {
                do_apply(
                    &runtime_job,
                    &path,
                    template,
                    title.as_deref(),
                    logo_path.as_deref(),
                )
            },
        )));
    }
    Ok(MaybeJob::Done(do_apply(
        runtime, path, template, title, logo_path,
    )?))
}

/// Parâmetros de `list_templates` (nenhum).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListParams {}

/// Parâmetros de `apply_template`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ApplyParams {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// shorts, square, landscape, intro_title ou watermark.
    pub template: TemplateName,
    /// Texto do título. Obrigatório em intro_title, opcional nos formatos.
    #[serde(default)]
    pub title: Option<String>,
    /// Imagem PNG do workspace. Obrigatório em watermark.
    #[serde(default)]
    pub logo_path: Option<String>,
    /// Executa como job e devolve job_id.
    #[serde(default)]
    pub background: bool,
}

/// Expõe as tools no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime_list = Arc::clone(runtime);
    mcp.tool(
        "list_templates",
        "Lista os templates visuais disponíveis para apply_template, com o que cada um exige.",
        move |_params: ListParams| guarded(list_templates(&runtime_list)),
    );
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "apply_template",
        "Aplica um template visual pronto ao vídeo: formato de rede social, título ou marca.\n\n\
         Templates: shorts (9:16), square (1:1), landscape (16:9), intro_title \
         (título em destaque nos 3 primeiros segundos) e watermark (logo no canto). \
         Veja detalhes com list_templates. Templates podem ser encadeados: aplique \
         shorts e depois watermark, por exemplo. O original não é modificado.",
        move |params: ApplyParams| {
            guarded(apply_template(
                &runtime,
                &params.path,
                params.template,
                params.title.as_deref(),
                params.logo_path.as_deref(),
                params.background,
            ))
        },
    );
}
