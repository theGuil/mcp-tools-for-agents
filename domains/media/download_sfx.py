"""Tool ``download_sound_effect``: baixa um efeito sonoro do Freesound para o workspace."""

from __future__ import annotations

from typing import TYPE_CHECKING, TypedDict

from core.errors import ErrorPayload, ToolError, guarded

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

DEFAULT_SFX_FOLDER = "sfx"


class DownloadSoundEffectResult(TypedDict):
    """Efeito sonoro salvo no workspace."""

    output: str
    sound_id: int
    name: str
    duration: float
    license: str
    author: str


@guarded
def download_sound_effect(
    runtime: Runtime, sound_id: int, *, folder: str = DEFAULT_SFX_FOLDER
) -> DownloadSoundEffectResult:
    """Implementação pura, testável sem MCP."""
    target_dir = runtime.workspace.resolve(folder)
    if target_dir.exists() and not target_dir.is_dir():
        raise ToolError(
            f"'{folder}' existe e não é uma pasta.",
            code="invalid_argument",
            hint="Informe outra pasta em folder.",
        )
    candidate = runtime.freesound.sound(sound_id)
    path = runtime.freesound.download_preview(candidate, target_dir)
    return DownloadSoundEffectResult(
        output=runtime.workspace.relative(path),
        sound_id=sound_id,
        name=candidate["name"],
        duration=candidate["duration"],
        license=candidate["license"],
        author=candidate["author"],
    )


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="download_sound_effect")
    def _tool(
        sound_id: int, folder: str = DEFAULT_SFX_FOLDER
    ) -> DownloadSoundEffectResult | ErrorPayload:
        """Baixa para o workspace um efeito sonoro do Freesound, em MP3.

        Use depois de search_sound_effects, quando quiser guardar o som para
        reaproveitar em vários vídeos ou ouvir antes de aplicar. Se o objetivo
        é só colocar o som no vídeo, add_sound_effects já faz o download
        sozinho a partir do sound_id ou de uma query. Baixar o mesmo som de
        novo reaproveita o arquivo que já está na pasta.

        Args:
            sound_id: Id do som, devolvido por search_sound_effects.
            folder: Pasta do workspace onde salvar. Criada se não existir.
        """
        return download_sound_effect(runtime, sound_id, folder=folder)
