"""Tool ``list_files``: lista arquivos do workspace."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final, Literal, TypedDict

from core.errors import ErrorPayload, ToolError, guarded

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

type FileKind = Literal["video", "audio", "image", "other"]

_KINDS: Final[dict[str, FileKind]] = {
    ".mp4": "video", ".mov": "video", ".mkv": "video", ".webm": "video", ".avi": "video",
    ".mp3": "audio", ".wav": "audio", ".aac": "audio", ".flac": "audio", ".m4a": "audio",
    ".png": "image", ".jpg": "image", ".jpeg": "image", ".webp": "image",
}  # fmt: skip
_MAX_ENTRIES: Final = 500


class FileEntry(TypedDict):
    """Um arquivo do workspace."""

    path: str
    kind: FileKind
    size_bytes: int


class ListFilesResult(TypedDict):
    """Arquivos encontrados."""

    directory: str
    files: list[FileEntry]
    count: int
    truncated: bool


def kind_of(suffix: str) -> FileKind:
    """Classifica um arquivo pela extensão."""
    return _KINDS.get(suffix.lower(), "other")


@guarded
def list_files(
    runtime: Runtime, directory: str = ".", *, kind: FileKind | None = None, recursive: bool = True
) -> ListFilesResult:
    """Implementação pura, testável sem MCP."""
    base = runtime.workspace.resolve(directory)
    if not base.is_dir():
        raise ToolError(f"Diretório '{directory}' não existe.", code="not_found")
    iterator = base.rglob("*") if recursive else base.glob("*")
    entries: list[FileEntry] = []
    truncated = False
    for path in sorted(p for p in iterator if p.is_file()):
        entry_kind = kind_of(path.suffix)
        if kind is not None and entry_kind != kind:
            continue
        if len(entries) >= _MAX_ENTRIES:
            truncated = True
            break
        entries.append(
            FileEntry(
                path=runtime.workspace.relative(path),
                kind=entry_kind,
                size_bytes=path.stat().st_size,
            )
        )
    return ListFilesResult(
        directory=runtime.workspace.relative(base) or ".",
        files=entries,
        count=len(entries),
        truncated=truncated,
    )


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="list_files")
    def _tool(
        directory: str = ".", kind: FileKind | None = None, recursive: bool = True
    ) -> ListFilesResult | ErrorPayload:
        """Lista os arquivos disponíveis no workspace.

        Chame primeiro para descobrir com o que trabalhar. Não altera nada.

        Args:
            directory: Subdiretório do workspace. Raiz por padrão.
            kind: Filtra por tipo: video, audio, image ou other.
            recursive: Inclui subdiretórios.
        """
        return list_files(runtime, directory, kind=kind, recursive=recursive)
