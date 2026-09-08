"""Configuração do servidor, lida de variáveis de ambiente.

Nenhum outro módulo lê ``os.environ``. Tudo passa por ``Settings``.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Final, Literal, get_args

from core.errors import ToolError

if TYPE_CHECKING:
    from collections.abc import Mapping

type DomainName = Literal["video", "audio", "files", "jobs", "media"]

ALL_DOMAINS: Final[tuple[DomainName, ...]] = get_args(DomainName.__value__)

_DEFAULT_WORKSPACE: Final = "./workspace"
_DEFAULT_FFMPEG_TIMEOUT: Final = 600.0
_DEFAULT_JOB_WORKERS: Final = 2
_DEFAULT_DOWNLOAD_TIMEOUT: Final = 900.0


@dataclass(frozen=True, slots=True)
class Settings:
    """Parâmetros de execução do servidor."""

    workspace_dir: Path
    ffmpeg_bin: str
    ffprobe_bin: str
    ffmpeg_timeout: float
    job_workers: int
    download_timeout: float
    domains: frozenset[DomainName]
    server_name: str = "mcp-tools-for-agents"

    @classmethod
    def from_env(cls, env: Mapping[str, str] | None = None) -> Settings:
        """Monta as configurações a partir do ambiente.

        Args:
            env: Mapa de variáveis. Usa ``os.environ`` quando omitido.

        Raises:
            ToolError: Se algum valor for inválido.
        """
        source = os.environ if env is None else env
        return cls(
            workspace_dir=Path(source.get("WORKSPACE_DIR", _DEFAULT_WORKSPACE)).expanduser(),
            ffmpeg_bin=source.get("FFMPEG_BIN", "ffmpeg"),
            ffprobe_bin=source.get("FFPROBE_BIN", "ffprobe"),
            ffmpeg_timeout=_parse_float(source, "FFMPEG_TIMEOUT", _DEFAULT_FFMPEG_TIMEOUT),
            job_workers=_parse_int(source, "JOB_WORKERS", _DEFAULT_JOB_WORKERS),
            download_timeout=_parse_float(source, "DOWNLOAD_TIMEOUT", _DEFAULT_DOWNLOAD_TIMEOUT),
            domains=_parse_domains(source.get("MCP_DOMAINS")),
        )


def _parse_float(source: Mapping[str, str], key: str, default: float) -> float:
    raw = source.get(key)
    if raw is None or raw.strip() == "":
        return default
    try:
        value = float(raw)
    except ValueError as exc:
        raise ToolError(
            f"{key} deve ser numérico, recebido '{raw}'.", code="invalid_argument"
        ) from exc
    if value <= 0:
        raise ToolError(f"{key} deve ser maior que zero.", code="invalid_argument")
    return value


def _parse_int(source: Mapping[str, str], key: str, default: int) -> int:
    raw = source.get(key)
    if raw is None or raw.strip() == "":
        return default
    try:
        value = int(raw)
    except ValueError as exc:
        raise ToolError(
            f"{key} deve ser inteiro, recebido '{raw}'.", code="invalid_argument"
        ) from exc
    if value <= 0:
        raise ToolError(f"{key} deve ser maior que zero.", code="invalid_argument")
    return value


def _parse_domains(raw: str | None) -> frozenset[DomainName]:
    if raw is None or raw.strip() == "":
        return frozenset(ALL_DOMAINS)
    chosen: set[DomainName] = set()
    for item in raw.split(","):
        name = item.strip().lower()
        if not name:
            continue
        if name not in ALL_DOMAINS:
            raise ToolError(
                f"Domínio desconhecido em MCP_DOMAINS: '{name}'.",
                code="invalid_argument",
                hint=f"Valores aceitos: {', '.join(ALL_DOMAINS)}.",
            )
        chosen.add(name)
    return frozenset(chosen)
