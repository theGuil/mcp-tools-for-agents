"""Tool ``create_subtitles``: gera um arquivo .srt a partir de trechos com tempos."""

from __future__ import annotations

from typing import TYPE_CHECKING, TypedDict

from core.errors import ErrorPayload, ToolError, guarded

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


class SubtitleSegment(TypedDict):
    """Um trecho de legenda."""

    start: float
    end: float
    text: str


class CreateSubtitlesResult(TypedDict):
    """Arquivo .srt gerado."""

    output: str
    segments: int
    duration: float


def format_timestamp(seconds: float) -> str:
    """Converte segundos para o formato ``HH:MM:SS,mmm`` do SRT."""
    total_ms = round(seconds * 1000)
    hours, rest = divmod(total_ms, 3_600_000)
    minutes, rest = divmod(rest, 60_000)
    secs, millis = divmod(rest, 1000)
    return f"{hours:02d}:{minutes:02d}:{secs:02d},{millis:03d}"


def build_srt(segments: list[SubtitleSegment]) -> str:
    """Monta o conteúdo SRT validando os trechos.

    Raises:
        ToolError: Se algum trecho tiver tempos ou texto inválidos.
    """
    if not segments:
        raise ToolError(
            "segments está vazio.",
            code="invalid_argument",
            hint="Envie ao menos um trecho {start, end, text}.",
        )
    blocks: list[str] = []
    for index, seg in enumerate(segments, start=1):
        start, end, text = seg["start"], seg["end"], seg["text"].strip()
        if start < 0 or end <= start:
            raise ToolError(
                f"Trecho {index} com intervalo inválido: start={start}, end={end}.",
                code="invalid_argument",
                hint="start deve ser >= 0 e end maior que start, em segundos.",
            )
        if not text:
            raise ToolError(f"Trecho {index} sem texto.", code="invalid_argument")
        blocks.append(f"{index}\n{format_timestamp(start)} --> {format_timestamp(end)}\n{text}\n")
    return "\n".join(blocks)


@guarded
def create_subtitles(
    runtime: Runtime, segments: list[SubtitleSegment], output: str
) -> CreateSubtitlesResult:
    """Implementação pura, testável sem MCP."""
    target = runtime.workspace.resolve(output)
    if target.suffix.lower() != ".srt":
        raise ToolError(
            f"output '{output}' deve terminar em .srt.",
            code="invalid_argument",
            hint="Ex: legendas/video.srt",
        )
    content = build_srt(segments)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8")
    return CreateSubtitlesResult(
        output=runtime.workspace.relative(target),
        segments=len(segments),
        duration=round(max(s["end"] for s in segments), 3),
    )


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="create_subtitles")
    def _tool(segments: list[SubtitleSegment], output: str) -> CreateSubtitlesResult | ErrorPayload:
        """Cria um arquivo de legendas .srt a partir de trechos com início, fim e texto.

        Use com os segments de transcribe_audio (corrigindo ou traduzindo o texto),
        ou escreva as legendas você mesmo. Depois aplique no vídeo com burn_subtitles.

        Args:
            segments: Lista de {start, end, text}, tempos em segundos.
            output: Caminho do .srt a criar, relativo ao workspace.
        """
        return create_subtitles(runtime, segments, output)
