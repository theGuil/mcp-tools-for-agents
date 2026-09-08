"""Domínios de tools. Cada subpacote expõe ``register(mcp, runtime)``.

Regras:
- Um arquivo por tool.
- Domínio não importa outro domínio. O que é comum vai para ``core``.
- Tool valida entrada, chama o ``core`` e devolve um dicionário tipado.
"""

from __future__ import annotations

import importlib
from dataclasses import dataclass
from typing import TYPE_CHECKING, Protocol, cast

if TYPE_CHECKING:
    from collections.abc import Iterable

    from mcp.server.mcpserver import MCPServer

    from config import DomainName, Settings
    from core.downloader import Downloader
    from core.ffmpeg import FFmpeg
    from core.freesound import Freesound
    from core.jobs import JobManager
    from core.paths import Workspace


@dataclass(frozen=True, slots=True)
class Runtime:
    """Dependências compartilhadas que toda tool recebe."""

    settings: Settings
    workspace: Workspace
    ffmpeg: FFmpeg
    jobs: JobManager
    downloader: Downloader
    freesound: Freesound


class DomainModule(Protocol):
    """Contrato mínimo de um pacote de domínio."""

    def register(self, mcp: MCPServer[None], runtime: Runtime) -> None:
        """Registra as tools do domínio no servidor."""
        ...


def register_domains(
    mcp: MCPServer[None], runtime: Runtime, names: Iterable[DomainName]
) -> tuple[str, ...]:
    """Importa e registra os domínios pedidos. Devolve os nomes registrados, em ordem."""
    registered: list[str] = []
    for name in sorted(names):
        module = cast("DomainModule", importlib.import_module(f"domains.{name}"))
        module.register(mcp, runtime)
        registered.append(name)
    return tuple(registered)
