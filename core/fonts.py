"""Localiza uma fonte TrueType para os filtros ``drawtext`` e ``subtitles``.

Sem fonte o FFmpeg depende do fontconfig, que nem sempre existe. Aqui
procuramos uma fonte comum do sistema e devolvemos o caminho.
"""

from __future__ import annotations

import re
from pathlib import Path
from typing import Final

from core.errors import ToolError

_CANDIDATES: Final = (
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
    "/usr/share/fonts/truetype/freefont/FreeSansBold.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/Library/Fonts/Arial Bold.ttf",
    "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
    "/System/Library/Fonts/Helvetica.ttc",
    "C:/Windows/Fonts/arialbd.ttf",
    "C:/Windows/Fonts/arial.ttf",
)

_SPECIAL: Final = re.compile(r"([\\'%:;\[\],])")


def find_font() -> Path | None:
    """Primeira fonte conhecida encontrada no sistema, ou ``None``."""
    for candidate in _CANDIDATES:
        path = Path(candidate)
        if path.is_file():
            return path
    return None


def require_font() -> Path:
    """Fonte obrigatória para desenhar texto.

    Raises:
        ToolError: Se nenhuma fonte for encontrada.
    """
    font = find_font()
    if font is None:
        raise ToolError(
            "Nenhuma fonte TrueType encontrada no sistema.",
            code="unavailable",
            hint="Instale a fonte DejaVu (ex: apt install fonts-dejavu-core).",
        )
    return font


def escape_drawtext(text: str) -> str:
    """Escapa um texto para uso dentro do filtro ``drawtext``."""
    return _SPECIAL.sub(r"\\\1", text)


def escape_filter_path(path: Path) -> str:
    """Escapa um caminho para uso como valor dentro de um filtergraph."""
    return str(path).replace("\\", "/").replace(":", "\\:").replace("'", "\\'")
