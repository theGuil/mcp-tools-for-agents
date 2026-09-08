"""Tool ``search_sound_effects``: busca efeitos sonoros no Freesound."""

from __future__ import annotations

from typing import TYPE_CHECKING, TypedDict

from core.errors import ErrorPayload, guarded
from core.freesound import SoundCandidate

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


class SearchSoundEffectsResult(TypedDict):
    """Sons encontrados para a busca."""

    query: str
    count: int
    sounds: list[SoundCandidate]


@guarded
def search_sound_effects(
    runtime: Runtime,
    query: str,
    *,
    limit: int = 5,
    max_duration: float = 10.0,
) -> SearchSoundEffectsResult:
    """Implementação pura, testável sem MCP."""
    sounds = runtime.freesound.search(query, limit=limit, max_duration=max_duration)
    return SearchSoundEffectsResult(query=query.strip(), count=len(sounds), sounds=sounds)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="search_sound_effects")
    def _tool(
        query: str,
        limit: int = 5,
        max_duration: float = 10.0,
    ) -> SearchSoundEffectsResult | ErrorPayload:
        """Busca efeitos sonoros gratuitos (Freesound) por descrição em texto.

        Use quando quiser escolher o som antes de aplicá-lo com
        add_sound_effects, ou para ver opções quando o primeiro resultado não
        serviu. Descreva o som em inglês, que é o idioma do acervo: "vine
        boom", "record scratch", "notification ding", "whoosh", "sad trombone",
        "crowd laugh", "crickets". Nada é baixado: cada resultado traz
        sound_id, nome, duração, licença e autor. Para baixar, use
        download_sound_effect com o sound_id, ou passe o sound_id direto em
        add_sound_effects.

        Licenças CC0 e CC BY servem para qualquer uso; CC BY pede crédito ao
        autor na descrição do vídeo. CC BY-NC é só para uso não comercial.

        Args:
            query: Descrição do som, de preferência em inglês.
            limit: Quantos resultados devolver, até 30.
            max_duration: Ignora sons mais longos que isso, em segundos.
        """
        return search_sound_effects(runtime, query, limit=limit, max_duration=max_duration)
