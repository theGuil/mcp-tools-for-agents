"""Tool ``add_sound_effects``: coloca efeitos sonoros em instantes específicos do vídeo."""

from __future__ import annotations

from typing import TYPE_CHECKING, Final, NotRequired, TypedDict

from core.errors import ErrorPayload, ToolError, guarded
from core.jobs import JobSubmitted

if TYPE_CHECKING:
    from pathlib import Path

    from mcp.server.mcpserver import MCPServer

    from domains import Runtime

_DEFAULT_SFX_FOLDER: Final = "sfx"
_DEFAULT_EFFECT_VOLUME: Final = 1.0
_MAX_EFFECT_VOLUME: Final = 3.0
_MAX_EFFECTS: Final = 20


class SoundEffect(TypedDict):
    """Um efeito a inserir: de onde vem o som e quando ele toca."""

    start: float
    audio: NotRequired[str]
    query: NotRequired[str]
    sound_id: NotRequired[int]
    volume: NotRequired[float]


class AppliedSoundEffect(TypedDict):
    """Efeito que entrou no vídeo, com o arquivo que foi usado."""

    audio: str
    start: float
    volume: float
    duration: float
    sound_id: int | None
    name: str | None
    license: str | None
    author: str | None


class AddSoundEffectsResult(TypedDict):
    """Vídeo gerado com os efeitos."""

    output: str
    effects: list[AppliedSoundEffect]
    original_volume: float


class _Resolved(TypedDict):
    """Efeito com o arquivo já no disco, pronto para o ffmpeg."""

    path: Path
    applied: AppliedSoundEffect


def _resolve_effect(runtime: Runtime, effect: SoundEffect, index: int, folder: str) -> _Resolved:
    """Descobre o arquivo do efeito: do workspace, por id ou por busca no Freesound."""
    start = effect["start"]
    if start < 0:
        raise ToolError(f"effects[{index}].start deve ser >= 0.", code="invalid_argument")
    volume = effect.get("volume", _DEFAULT_EFFECT_VOLUME)
    if not 0.0 < volume <= _MAX_EFFECT_VOLUME:
        raise ToolError(
            f"effects[{index}].volume deve estar entre 0 e {_MAX_EFFECT_VOLUME:g}.",
            code="invalid_argument",
            hint="1 mantém o volume do efeito, 0.5 deixa mais baixo, 2 aumenta.",
        )
    audio = effect.get("audio")
    sound_id = effect.get("sound_id")
    query = effect.get("query")
    sources = sum(value is not None for value in (audio, sound_id, query))
    if sources != 1:
        raise ToolError(
            f"effects[{index}] precisa de exatamente um entre audio, sound_id e query.",
            code="invalid_argument",
            hint=(
                "Use audio para um arquivo do workspace, sound_id para um resultado de "
                "search_sound_effects ou query para buscar o som pelo nome."
            ),
        )
    if audio is not None:
        path = runtime.workspace.existing(audio)
        return _Resolved(
            path=path,
            applied=AppliedSoundEffect(
                audio=runtime.workspace.relative(path),
                start=start,
                volume=volume,
                duration=0.0,
                sound_id=None,
                name=None,
                license=None,
                author=None,
            ),
        )
    if sound_id is not None:
        candidate = runtime.freesound.sound(sound_id)
    else:
        found = runtime.freesound.search(query or "", limit=1)
        if not found:
            raise ToolError(
                f"Nenhum som encontrado para '{query}'.",
                code="not_found",
                hint=(
                    "Tente outra descrição em inglês, mais curta ou mais genérica, "
                    "ou busque opções com search_sound_effects."
                ),
            )
        candidate = found[0]
    target_dir = runtime.workspace.resolve(folder)
    path = runtime.freesound.download_preview(candidate, target_dir)
    return _Resolved(
        path=path,
        applied=AppliedSoundEffect(
            audio=runtime.workspace.relative(path),
            start=start,
            volume=volume,
            duration=candidate["duration"],
            sound_id=candidate["sound_id"],
            name=candidate["name"],
            license=candidate["license"],
            author=candidate["author"],
        ),
    )


def _build_filter(resolved: list[_Resolved], *, has_original: bool, original_volume: float) -> str:
    """Monta o filter_complex: cada efeito ganha volume e atraso, e tudo é somado."""
    parts: list[str] = []
    labels: list[str] = []
    if has_original:
        parts.append(f"[0:a]volume={original_volume}[bg]")
        labels.append("[bg]")
    for index, item in enumerate(resolved, start=1):
        applied = item["applied"]
        delay_ms = round(applied["start"] * 1000)
        parts.append(f"[{index}:a]volume={applied['volume']},adelay={delay_ms}:all=1[fx{index}]")
        labels.append(f"[fx{index}]")
    if len(labels) == 1:
        # Só um fluxo: não há o que misturar, mas o apad garante som até o fim do vídeo.
        parts.append(f"{labels[0]}apad[aout]")
    elif has_original:
        parts.append(
            f"{''.join(labels)}amix=inputs={len(labels)}:duration=first:"
            "dropout_transition=0:normalize=0[aout]"
        )
    else:
        parts.append(
            f"{''.join(labels)}amix=inputs={len(labels)}:duration=longest:"
            "dropout_transition=0:normalize=0,apad[aout]"
        )
    return ";".join(parts)


def _do_sound_effects(
    runtime: Runtime,
    path: str,
    effects: list[SoundEffect],
    *,
    original_volume: float,
    folder: str,
) -> AddSoundEffectsResult:
    source = runtime.workspace.existing(path)
    if not effects:
        raise ToolError(
            "effects não pode ser vazio.",
            code="invalid_argument",
            hint='Informe ao menos um efeito, ex: [{"query": "vine boom", "start": 3.2}].',
        )
    if len(effects) > _MAX_EFFECTS:
        raise ToolError(
            f"No máximo {_MAX_EFFECTS} efeitos por chamada.",
            code="invalid_argument",
            hint="Divida em mais de uma chamada, aplicando sobre o vídeo gerado.",
        )
    if not 0.0 <= original_volume <= 1.0:
        raise ToolError(
            "original_volume deve estar entre 0 e 1.",
            code="invalid_argument",
            hint="1 mantém o áudio original, 0.5 abaixa pela metade, 0 silencia.",
        )
    info = runtime.ffmpeg.probe(source)
    # Confere os instantes antes de baixar qualquer som: erro barato primeiro.
    for index, effect in enumerate(effects):
        if effect["start"] >= info["duration"]:
            raise ToolError(
                f"effects[{index}].start={effect['start']}s ultrapassa a duração do vídeo "
                f"({info['duration']:.2f}s).",
                code="invalid_argument",
                hint="Use probe_video para conferir a duração.",
            )
    resolved = [
        _resolve_effect(runtime, effect, index, folder) for index, effect in enumerate(effects)
    ]
    for item in resolved:
        effect_info = runtime.ffmpeg.probe(item["path"])
        if not effect_info["has_audio"]:
            raise ToolError(
                f"'{item['applied']['audio']}' não tem trilha de áudio.",
                code="invalid_argument",
                hint="Informe um arquivo de áudio (mp3, m4a, wav).",
            )
        if item["applied"]["duration"] == 0.0:
            item["applied"]["duration"] = effect_info["duration"]
    has_original = info["has_audio"] and original_volume > 0.0
    filter_expr = _build_filter(
        resolved, has_original=has_original, original_volume=original_volume
    )
    output = runtime.workspace.output_for(source, "sfx")
    args: list[str] = ["-i", str(source)]
    for item in resolved:
        args.extend(["-i", str(item["path"])])
    args.extend(
        [
            "-filter_complex",
            filter_expr,
            "-map",
            "0:v:0",
            "-map",
            "[aout]",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-shortest",
            str(output),
        ]
    )
    runtime.ffmpeg.run(args)
    return AddSoundEffectsResult(
        output=runtime.workspace.relative(output),
        effects=[item["applied"] for item in resolved],
        original_volume=original_volume,
    )


@guarded
def add_sound_effects(
    runtime: Runtime,
    path: str,
    effects: list[SoundEffect],
    *,
    original_volume: float = 1.0,
    folder: str = _DEFAULT_SFX_FOLDER,
    background: bool = False,
) -> AddSoundEffectsResult | JobSubmitted:
    """Implementação pura, testável sem MCP."""
    if background:
        return runtime.jobs.submit(
            "add_sound_effects",
            lambda: _do_sound_effects(
                runtime, path, effects, original_volume=original_volume, folder=folder
            ),
        )
    return _do_sound_effects(runtime, path, effects, original_volume=original_volume, folder=folder)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="add_sound_effects")
    def _tool(
        path: str,
        effects: list[SoundEffect],
        original_volume: float = 1.0,
        folder: str = _DEFAULT_SFX_FOLDER,
        background: bool = False,
    ) -> AddSoundEffectsResult | JobSubmitted | ErrorPayload:
        """Insere efeitos sonoros (vine boom, ding, whoosh...) em instantes do vídeo.

        Use para dar ritmo de TikTok/Reels a um vídeo: um "boom" na revelação,
        um "record scratch" na pausa, um "ding" quando aparece o texto. Passe
        quantos efeitos quiser de uma vez; tudo é aplicado em um único passo e
        o vídeo não é re-encodado, só o áudio.

        Cada item de effects tem start (segundo em que o som começa) e uma
        única origem para o som:
        - query: descreve o som em inglês ("vine boom", "notification ding",
          "crowd laugh") e a tool busca no Freesound, baixa o primeiro
          resultado para a pasta sfx/ e usa;
        - sound_id: id devolvido por search_sound_effects, quando você quer
          escolher o som;
        - audio: arquivo de áudio que já está no workspace.
        volume é opcional (1 = como veio, 0.5 mais baixo, até 3 para reforçar).

        O áudio original continua no vídeo em original_volume; use 0 para
        deixar só os efeitos. Descubra os instantes certos com
        transcribe_audio ou detect_scenes. O resultado lista, para cada efeito,
        o arquivo usado e a licença; sons CC BY pedem crédito ao autor.

        Args:
            path: Vídeo, relativo ao workspace.
            effects: Lista de efeitos, cada um com start e query, sound_id ou audio.
            original_volume: Volume do áudio original, de 0 (mudo) a 1 (igual).
            folder: Pasta do workspace onde os sons baixados ficam guardados.
            background: Executa como job e devolve job_id.
        """
        return add_sound_effects(
            runtime,
            path,
            effects,
            original_volume=original_volume,
            folder=folder,
            background=background,
        )
