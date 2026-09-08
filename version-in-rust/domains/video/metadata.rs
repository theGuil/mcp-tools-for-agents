//! Tool `set_video_metadata`: grava título, descrição e autor nos metadados do arquivo.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::domains::{McpServer, Runtime};
use crate::ffargs;

const MAX_FIELD: usize = 5000;

/// Arquivo gerado com os metadados.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SetVideoMetadataResult {
    pub output: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub comment: Option<String>,
}

/// Campos de metadados informados pelo agente. `None` deixa o campo como está.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataFields {
    pub title: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub comment: Option<String>,
}

/// Implementação pura, testável sem MCP.
///
/// # Errors
///
/// Nenhum campo informado, campo longo demais, arquivo inexistente ou falha do ffmpeg.
pub fn set_video_metadata(
    runtime: &Runtime,
    path: &str,
    fields: &MetadataFields,
) -> ToolResult<SetVideoMetadataResult> {
    let source = runtime.workspace.existing(path)?;
    let entries = [
        ("title", &fields.title),
        ("description", &fields.description),
        ("artist", &fields.author),
        ("comment", &fields.comment),
    ];
    let provided: Vec<(&str, String)> = entries
        .iter()
        .filter_map(|(key, value)| value.as_ref().map(|v| (*key, v.trim().to_string())))
        .collect();
    if provided.is_empty() {
        return Err(ToolError::with_hint(
            "Nenhum metadado informado.",
            ErrorCode::InvalidArgument,
            "Passe ao menos title, description, author ou comment.",
        ));
    }
    for (key, value) in &provided {
        if value.chars().count() > MAX_FIELD {
            return Err(ToolError::with_hint(
                format!("{key} tem mais de {MAX_FIELD} caracteres."),
                ErrorCode::InvalidArgument,
                "Encurte o texto.",
            ));
        }
    }
    let output = runtime.workspace.output_for(&source, "meta", None);
    let mut args = ffargs![
        "-i",
        source,
        "-map",
        "0",
        "-c",
        "copy",
        "-map_metadata",
        "0"
    ];
    for (key, value) in &provided {
        args.extend(ffargs!["-metadata", format!("{key}={value}")]);
    }
    args.push(output.clone().into_os_string());
    runtime.ffmpeg.run(&args)?;
    let get = |wanted: &str| {
        provided
            .iter()
            .find(|(key, _)| *key == wanted)
            .map(|(_, value)| value.clone())
    };
    Ok(SetVideoMetadataResult {
        output: runtime.workspace.relative(&output),
        title: get("title"),
        description: get("description"),
        author: get("artist"),
        comment: get("comment"),
    })
}

/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Vídeo, relativo ao workspace.
    pub path: String,
    /// Título do vídeo.
    #[serde(default)]
    pub title: Option<String>,
    /// Descrição completa.
    #[serde(default)]
    pub description: Option<String>,
    /// Autor ou canal.
    #[serde(default)]
    pub author: Option<String>,
    /// Comentário livre (ex: hashtags).
    #[serde(default)]
    pub comment: Option<String>,
}

/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "set_video_metadata",
        "Grava título, descrição e autor nos metadados do arquivo de vídeo, sem re-encodar.\n\n\
         A descrição fica embutida no arquivo e é lida por players e plataformas. \
         Não altera a imagem: para texto visível use add_text_overlay. O original \
         não é modificado; é gerado um novo arquivo com sufixo _meta.",
        move |params: Params| {
            guarded(set_video_metadata(
                &runtime,
                &params.path,
                &MetadataFields {
                    title: params.title,
                    description: params.description,
                    author: params.author,
                    comment: params.comment,
                },
            ))
        },
    );
}
