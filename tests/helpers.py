"""Auxiliares de teste."""

from __future__ import annotations

from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    from collections.abc import Mapping

    from core.errors import ErrorPayload
    from core.jobs import JobSubmitted


def unwrap[T: Mapping[str, object]](result: T | ErrorPayload | JobSubmitted) -> T:
    """Garante que a tool devolveu sucesso síncrono e estreita o tipo."""
    assert "error" not in result, result
    assert "job_id" not in result, result
    return result


def unwrap_job[T: Mapping[str, object]](result: T | ErrorPayload | JobSubmitted) -> JobSubmitted:
    """Garante que a tool devolveu um job e estreita o tipo."""
    assert "job_id" in result, result
    return cast("JobSubmitted", result)
