"""Tool ``set_video_metadata``: grava título, descrição e autor nos metadados do arquivo."""

from __future__ import annotations

from typing import TYPE_CHECKING, TypedDict

from core.errors import ErrorPayload, ToolError, guarded

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

_MAX_FIELD = 5000


class SetVideoMetadataResult(TypedDict):
    """Arquivo gerado com os metadados."""

    output: str
    title: str | None
    description: str | None
    author: str | None
    comment: str | None


@guarded
def set_video_metadata(
    runtime: Runtime,
    path: str,
    *,
    title: str | None = None,
    description: str | None = None,
    author: str | None = None,
    comment: str | None = None,
) -> SetVideoMetadataResult:
    """Implementação pura, testável sem MCP."""
    source = runtime.workspace.existing(path)
    fields = {"title": title, "description": description, "artist": author, "comment": comment}
    provided = {key: value.strip() for key, value in fields.items() if value is not None}
    if not provided:
        raise ToolError(
            "Nenhum metadado informado.",
            code="invalid_argument",
            hint="Passe ao menos title, description, author ou comment.",
        )
    for key, value in provided.items():
        if len(value) > _MAX_FIELD:
            raise ToolError(
                f"{key} tem mais de {_MAX_FIELD} caracteres.",
                code="invalid_argument",
                hint="Encurte o texto.",
            )
    output = runtime.workspace.output_for(source, "meta")
    metadata_args: list[str] = []
    for key, value in provided.items():
        metadata_args.extend(["-metadata", f"{key}={value}"])
    runtime.ffmpeg.run(
        [
            "-i",
            str(source),
            "-map",
            "0",
            "-c",
            "copy",
            "-map_metadata",
            "0",
            *metadata_args,
            str(output),
        ]
    )
    return SetVideoMetadataResult(
        output=runtime.workspace.relative(output),
        title=provided.get("title"),
        description=provided.get("description"),
        author=provided.get("artist"),
        comment=provided.get("comment"),
    )


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="set_video_metadata")
    def _tool(
        path: str,
        title: str | None = None,
        description: str | None = None,
        author: str | None = None,
        comment: str | None = None,
    ) -> SetVideoMetadataResult | ErrorPayload:
        """Grava título, descrição e autor nos metadados do arquivo de vídeo, sem re-encodar.

        A descrição fica embutida no arquivo e é lida por players e plataformas.
        Não altera a imagem: para texto visível use add_text_overlay. O original
        não é modificado; é gerado um novo arquivo com sufixo _meta.

        Args:
            path: Vídeo, relativo ao workspace.
            title: Título do vídeo.
            description: Descrição completa.
            author: Autor ou canal.
            comment: Comentário livre (ex: hashtags).
        """
        return set_video_metadata(
            runtime, path, title=title, description=description, author=author, comment=comment
        )
