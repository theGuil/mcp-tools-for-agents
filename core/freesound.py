"""Cliente tipado para a API do Freesound (https://freesound.org).

Acervo gratuito de efeitos sonoros com licença Creative Commons. Com a chave
simples (``token``) dá para buscar sons e baixar o preview em MP3 de 128 kbps,
que é suficiente para efeito sonoro em vídeo curto. O arquivo original em alta
qualidade exige OAuth2 e não é usado aqui.

Segundo ponto do projeto que conversa com a internet, ao lado do
``core/downloader.py``. O restante recebe apenas metadados tipados e o caminho
do arquivo baixado.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass
from http.client import HTTPException
from typing import TYPE_CHECKING, Final, TypedDict
from urllib.error import HTTPError, URLError
from urllib.parse import urlencode, urlparse
from urllib.request import Request, urlopen

from core.errors import ToolError

if TYPE_CHECKING:
    from pathlib import Path

_API_BASE: Final = "https://freesound.org/apiv2"
_FIELDS: Final = "id,name,duration,previews,license,username,tags"
_PREVIEW_KEY: Final = "preview-hq-mp3"
_MAX_LIMIT: Final = 30
_MAX_PREVIEW_BYTES: Final = 20_000_000
_SAFE_NAME: Final = re.compile(r"[^A-Za-z0-9._-]+")
# Muitos nomes no Freesound terminam com a extensão do original ("Boom.wav").
_AUDIO_EXT: Final = re.compile(r"\.(wav|mp3|ogg|flac|aif|aiff|m4a|aac)$", re.IGNORECASE)
_MAX_NAME: Final = 40
_HTTP_UNAUTHORIZED: Final = 401
_HTTP_NOT_FOUND: Final = 404
_HTTP_TOO_MANY: Final = 429

# Nomes curtos de licença, mais legíveis para o agente que a URL completa.
_LICENSES: Final[tuple[tuple[str, str], ...]] = (
    ("publicdomain/zero", "CC0"),
    ("licenses/by-nc", "CC BY-NC"),
    ("licenses/by", "CC BY"),
    ("sampling+", "Sampling+"),
)

_KEY_HINT: Final = (
    "Confira a chave em FREESOUND_API_KEY. Crie uma grátis em https://freesound.org/apiv2/apply/."
)


class SoundCandidate(TypedDict):
    """Um som encontrado no Freesound."""

    sound_id: int
    name: str
    duration: float
    license: str
    author: str
    tags: list[str]
    preview_url: str


@dataclass(frozen=True, slots=True)
class Freesound:
    """Busca e baixa efeitos sonoros do Freesound, com erros amigáveis."""

    api_key: str
    timeout_seconds: float = 30.0

    def search(
        self, query: str, *, limit: int = 5, max_duration: float | None = 10.0
    ) -> list[SoundCandidate]:
        """Busca sons por texto, do mais relevante para o menos relevante.

        Args:
            query: Termos de busca, em inglês de preferência (é o idioma do acervo).
            limit: Quantidade máxima de resultados, até 30.
            max_duration: Descarta sons mais longos que isso, em segundos.

        Raises:
            ToolError: Se a consulta for inválida ou a API falhar.
        """
        cleaned = query.strip()
        if not cleaned:
            raise ToolError(
                "query não pode ser vazia.",
                code="invalid_argument",
                hint="Descreva o som em inglês, ex: 'vine boom', 'record scratch', 'ding'.",
            )
        if limit < 1:
            raise ToolError("limit deve ser >= 1.", code="invalid_argument")
        if max_duration is not None and max_duration <= 0:
            raise ToolError("max_duration deve ser maior que zero.", code="invalid_argument")
        params: dict[str, str] = {
            "query": cleaned,
            "fields": _FIELDS,
            "page_size": str(min(limit, _MAX_LIMIT)),
            "token": self._require_key(),
        }
        if max_duration is not None:
            params["filter"] = f"duration:[0 TO {max_duration:g}]"
        data = self._get_json(f"{_API_BASE}/search/text/?{urlencode(params)}")
        return parse_search(data)

    def sound(self, sound_id: int) -> SoundCandidate:
        """Lê os metadados de um som pelo id.

        Raises:
            ToolError: Se o som não existir ou a API falhar.
        """
        if sound_id <= 0:
            raise ToolError("sound_id deve ser um inteiro positivo.", code="invalid_argument")
        params = {"fields": _FIELDS, "token": self._require_key()}
        data = self._get_json(f"{_API_BASE}/sounds/{sound_id}/?{urlencode(params)}")
        candidate = parse_sound(data)
        if candidate is None:
            raise ToolError(
                f"Som {sound_id} veio sem preview em MP3.",
                code="download_failed",
                hint="Escolha outro resultado de search_sound_effects.",
            )
        return candidate

    def download_preview(self, candidate: SoundCandidate, target_dir: Path) -> Path:
        """Baixa o preview MP3 de um som para ``target_dir``.

        O nome do arquivo junta o nome do som e o id, então baixar o mesmo som
        duas vezes reaproveita o arquivo já existente.

        Args:
            candidate: Som devolvido por ``search`` ou ``sound``.
            target_dir: Pasta de destino, criada se não existir.

        Raises:
            ToolError: Se o download falhar.
        """
        sound_id = candidate["sound_id"]
        target_dir.mkdir(parents=True, exist_ok=True)
        target = target_dir / f"{safe_name(candidate['name'])}_{sound_id}.mp3"
        if target.is_file() and target.stat().st_size > 0:
            return target
        raw = self._get_bytes(candidate["preview_url"])
        if not raw:
            raise ToolError(
                f"Preview do som {sound_id} veio vazio.",
                code="download_failed",
                hint="Tente outro resultado de search_sound_effects.",
            )
        target.write_bytes(raw)
        return target

    def _require_key(self) -> str:
        key = self.api_key.strip()
        if not key:
            raise ToolError(
                "Freesound indisponível: chave de API não configurada.",
                code="unavailable",
                hint=_KEY_HINT,
            )
        return key

    def _get_json(self, url: str) -> dict[str, object]:
        raw = self._get_bytes(url)
        try:
            data: object = json.loads(raw)
        except json.JSONDecodeError as exc:
            raise ToolError(
                "Resposta do Freesound não é JSON válido.",
                code="download_failed",
                hint="Tente de novo em alguns segundos.",
            ) from exc
        if not isinstance(data, dict):
            raise ToolError("Resposta do Freesound em formato inesperado.", code="download_failed")
        return data

    def _get_bytes(self, url: str) -> bytes:
        if urlparse(url).scheme != "https":
            raise ToolError(
                f"URL inesperada do Freesound: '{url}'.",
                code="download_failed",
                hint="Só endereços https são aceitos.",
            )
        request = Request(url, headers={"User-Agent": "mcp-tools-for-agents"})  # noqa: S310
        try:
            # S310: o esquema é validado logo acima, só https chega aqui.
            with urlopen(request, timeout=self.timeout_seconds) as response:  # noqa: S310
                body: bytes = response.read(_MAX_PREVIEW_BYTES)
        except HTTPError as exc:
            raise _http_error(exc) from exc
        except (URLError, HTTPException, TimeoutError, OSError) as exc:
            raise ToolError(
                f"Não foi possível falar com o Freesound: {exc}.",
                code="download_failed",
                hint="Confira a conexão com a internet e tente de novo.",
            ) from exc
        return body


def _http_error(exc: HTTPError) -> ToolError:
    if exc.code == _HTTP_UNAUTHORIZED:
        return ToolError("Freesound recusou a chave de API.", code="unavailable", hint=_KEY_HINT)
    if exc.code == _HTTP_NOT_FOUND:
        return ToolError(
            "Som não encontrado no Freesound.",
            code="not_found",
            hint="Use search_sound_effects para achar um sound_id válido.",
        )
    if exc.code == _HTTP_TOO_MANY:
        return ToolError(
            "Limite de requisições do Freesound atingido.",
            code="download_failed",
            hint="Aguarde um minuto antes de buscar de novo.",
        )
    return ToolError(
        f"Freesound respondeu HTTP {exc.code}.",
        code="download_failed",
        hint="Tente de novo em alguns segundos.",
    )


def safe_name(name: str) -> str:
    """Converte o nome do som em um pedaço seguro de nome de arquivo, sem extensão."""
    stem = _AUDIO_EXT.sub("", name.strip())
    cleaned = _SAFE_NAME.sub("_", stem).strip("_")[:_MAX_NAME].strip("_")
    return cleaned or "sound"


def parse_search(data: dict[str, object]) -> list[SoundCandidate]:
    """Converte a resposta bruta de ``/search/text/`` em candidatos."""
    results = data.get("results")
    if not isinstance(results, list):
        return []
    found: list[SoundCandidate] = []
    for item in results:
        if isinstance(item, dict):
            candidate = parse_sound(item)
            if candidate is not None:
                found.append(candidate)
    return found


def parse_sound(item: dict[str, object]) -> SoundCandidate | None:
    """Converte um som bruto da API em ``SoundCandidate``.

    Devolve ``None`` quando o som não tem id ou preview em MP3, já que sem
    preview não há o que baixar.
    """
    sound_id = item.get("id")
    previews = item.get("previews")
    if not isinstance(sound_id, int) or not isinstance(previews, dict):
        return None
    preview = previews.get(_PREVIEW_KEY)
    if not isinstance(preview, str) or not preview:
        return None
    duration = item.get("duration")
    raw_tags = item.get("tags")
    tags = [tag for tag in raw_tags if isinstance(tag, str)] if isinstance(raw_tags, list) else []
    return SoundCandidate(
        sound_id=sound_id,
        name=str(item.get("name") or "sound"),
        duration=float(duration) if isinstance(duration, (int, float)) else 0.0,
        license=short_license(str(item.get("license") or "")),
        author=str(item.get("username") or ""),
        tags=tags,
        preview_url=preview,
    )


def short_license(url: str) -> str:
    """Resume a URL da licença em um nome curto (``CC0``, ``CC BY``...)."""
    for fragment, label in _LICENSES:
        if fragment in url:
            return label
    return url or "desconhecida"
