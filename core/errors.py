"""Erros com mensagem clara para o agente.

Um agente de IA não sabe ler stack trace. Toda tool devolve, em caso de
falha, um dicionário ``{"error": ..., "code": ..., "hint": ...}`` que o
modelo consegue interpretar e usar para se corrigir sozinho.
"""

from __future__ import annotations

import functools
from typing import TYPE_CHECKING, Final, Literal, TypedDict

if TYPE_CHECKING:
    from collections.abc import Callable

type ErrorCode = Literal[
    "not_found",
    "invalid_argument",
    "outside_workspace",
    "ffmpeg_failed",
    "timeout",
    "job_not_found",
    "job_not_finished",
    "unavailable",
]


class ErrorPayload(TypedDict):
    """Formato único de erro devolvido por qualquer tool."""

    error: str
    code: ErrorCode
    hint: str | None


class ToolError(Exception):
    """Falha esperada de uma tool, com orientação para o agente."""

    __slots__ = ("code", "hint", "message")

    def __init__(self, message: str, *, code: ErrorCode, hint: str | None = None) -> None:
        """Cria o erro.

        Args:
            message: O que deu errado, em uma frase.
            code: Categoria do erro, estável para o agente ramificar.
            hint: O que o agente pode fazer para resolver.
        """
        super().__init__(message)
        self.message: Final = message
        self.code: Final = code
        self.hint: Final = hint

    def to_payload(self) -> ErrorPayload:
        """Serializa para o dicionário devolvido ao agente."""
        return ErrorPayload(error=self.message, code=self.code, hint=self.hint)


def guarded[**P, R](fn: Callable[P, R]) -> Callable[P, R | ErrorPayload]:
    """Converte ``ToolError`` em ``ErrorPayload`` preservando a assinatura da função.

    Aplicado em toda função exposta como tool. Qualquer outra exceção
    continua subindo, pois indica bug e não erro de uso.
    """

    @functools.wraps(fn)
    def wrapper(*args: P.args, **kwargs: P.kwargs) -> R | ErrorPayload:
        try:
            return fn(*args, **kwargs)
        except ToolError as exc:
            return exc.to_payload()

    return wrapper
