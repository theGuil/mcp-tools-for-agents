"""Resolução e validação de caminhos dentro do workspace.

Todo caminho que o agente informa é relativo ao workspace. Nada fora dele
é lido ou escrito, mesmo que o agente peça com ``..`` ou caminho absoluto.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path
from typing import Final

from core.errors import ToolError

_SAFE_STEM: Final = re.compile(r"[^A-Za-z0-9._-]+")


@dataclass(frozen=True, slots=True)
class Workspace:
    """Diretório raiz onde o agente pode ler e escrever."""

    root: Path

    @classmethod
    def at(cls, path: Path | str) -> Workspace:
        """Cria o workspace garantindo que o diretório exista."""
        root = Path(path).expanduser().resolve()
        root.mkdir(parents=True, exist_ok=True)
        return cls(root=root)

    def resolve(self, relative: str) -> Path:
        """Converte um caminho informado pelo agente em caminho absoluto seguro.

        Raises:
            ToolError: Se o caminho sair do workspace.
        """
        candidate = (self.root / relative).resolve()
        if candidate != self.root and self.root not in candidate.parents:
            raise ToolError(
                f"Caminho '{relative}' está fora do workspace.",
                code="outside_workspace",
                hint="Use caminhos relativos à raiz do workspace, sem '..' nem caminho absoluto.",
            )
        return candidate

    def existing(self, relative: str) -> Path:
        """Resolve o caminho e garante que o arquivo exista.

        Raises:
            ToolError: Se o arquivo não existir.
        """
        path = self.resolve(relative)
        if not path.is_file():
            raise ToolError(
                f"Arquivo '{relative}' não encontrado no workspace.",
                code="not_found",
                hint="Use list_files para ver os arquivos disponíveis.",
            )
        return path

    def relative(self, path: Path) -> str:
        """Caminho relativo ao workspace, em formato POSIX, para devolver ao agente."""
        return path.relative_to(self.root).as_posix()

    def output_for(self, source: Path, suffix_tag: str, extension: str | None = None) -> Path:
        """Gera um caminho de saída único ao lado do arquivo de origem.

        Args:
            source: Arquivo de origem.
            suffix_tag: Marcador adicionado ao nome, ex: ``cut_0-10``.
            extension: Extensão final. Mantém a do original quando omitida.
        """
        tag = _SAFE_STEM.sub("_", suffix_tag).strip("_")
        ext = extension if extension is not None else source.suffix
        ext = ext if ext.startswith(".") or not ext else f".{ext}"
        base = source.with_name(f"{source.stem}_{tag}{ext}")
        counter = 1
        candidate = base
        while candidate.exists():
            candidate = source.with_name(f"{source.stem}_{tag}_{counter}{ext}")
            counter += 1
        return candidate
