"""Wrapper tipado para o ``yt-dlp``.

Junto com ``core/freesound.py``, é onde o projeto conversa com a internet. O
restante recebe apenas metadados tipados e o caminho do arquivo baixado.

Aceita qualquer URL http(s). O yt-dlp tem extractor nativo para mais de mil
sites e, quando nenhum reconhece a página, o extractor genérico baixa o HTML e
procura sozinho a fonte do vídeo: tag ``<video>``, ``<source>``, manifesto HLS
``.m3u8``, DASH ``.mpd``, player embutido ou JSON-LD.

São três tentativas, nesta ordem: o extractor nativo do site, o genérico e, se
nenhum achar nada, uma varredura própria do HTML que desce um nível nos iframes
-- é lá que players caseiros, como os de portais de aula, escondem o arquivo.
"""

from __future__ import annotations

import importlib
import re
import shutil
import time
from dataclasses import dataclass
from html import unescape
from html.parser import HTMLParser
from http.client import HTTPException
from pathlib import Path
from typing import TYPE_CHECKING, Final, Literal, Protocol, Self, TypedDict, cast
from urllib.parse import urljoin, urlparse
from urllib.request import Request, urlopen

from core.errors import ToolError

if TYPE_CHECKING:
    from collections.abc import Iterable

type VideoQuality = Literal["best", "1080p", "720p", "480p", "360p", "audio"]

_ALLOWED_SCHEMES: Final = frozenset({"http", "https"})
_SAFE_TITLE: Final = re.compile(r"[^A-Za-z0-9._-]+")
_MAX_TITLE: Final = 60
# O id do YouTube tem 11 caracteres, mas o extractor genérico usa a URL
# inteira como id. Sem corte, o nome do arquivo estoura o limite do sistema.
_MAX_ID: Final = 40

# Varredura do HTML, usada só quando o yt-dlp não acha nada sozinho.
_MEDIA_TAGS: Final = frozenset({"video", "source", "audio"})
_FRAME_TAGS: Final = frozenset({"iframe", "embed"})
_MEDIA_URL: Final = re.compile(
    r"https?://[^\s\"'<>\\]+?\.(?:mp4|m3u8|mpd|webm|mov|m4v)(?:\?[^\s\"'<>\\]*)?",
    re.IGNORECASE,
)
_PAGE_TIMEOUT: Final = 30.0
_MAX_PAGE_BYTES: Final = 2_000_000
_MAX_FRAMES: Final = 3
_MAX_CANDIDATES: Final = 6
# Alguns sistemas de arquivos guardam mtime com menos precisão que
# time.time(), e o arquivo recém-gravado parece anterior ao início.
_MTIME_SLACK: Final = 2.0
# Muitos portais devolvem uma página vazia para cliente que não parece navegador.
_USER_AGENT: Final = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"
)


def _capped(height: int) -> str:
    """Formato preferindo MP4, aceitando qualquer container e caindo para o melhor disponível."""
    return (
        f"bestvideo[height<={height}][ext=mp4]+bestaudio[ext=m4a]/"
        f"bestvideo[height<={height}]+bestaudio/"
        f"best[height<={height}]/best"
    )


# O segundo nível de cada cadeia (sem filtro de extensão) é o que faz sites
# genéricos funcionarem: HLS e players caseiros raramente entregam mp4/m4a puros.
_FORMATS: Final[dict[VideoQuality, str]] = {
    "best": "bestvideo[ext=mp4]+bestaudio[ext=m4a]/bestvideo+bestaudio/best[ext=mp4]/best",
    "1080p": _capped(1080),
    "720p": _capped(720),
    "480p": _capped(480),
    "360p": _capped(360),
    "audio": "bestaudio[ext=m4a]/bestaudio/best",
}

# Casos em que insistir não adianta: o hint manda o agente parar em vez de
# gastar tentativas. Ordem importa, a primeira que casar vence.
_FAILURE_HINTS: Final[tuple[tuple[re.Pattern[str], str], ...]] = (
    (
        re.compile(r"\bdrm\b|widevine|fairplay|playready", re.IGNORECASE),
        (
            "O vídeo é protegido por DRM e nenhuma ferramenta consegue baixá-lo. "
            "Não tente de novo: peça o arquivo ao usuário."
        ),
    ),
    (
        re.compile(
            r"sign in|log ?in|cookies|private|members.only|premium|subscriber|purchase|paid",
            re.IGNORECASE,
        ),
        (
            "A página exige login. Não tente de novo com esta URL: peça ao usuário "
            "um link público ou o arquivo já baixado."
        ),
    ),
    (
        re.compile(
            r"unavailable|removed|deleted|not found|404|terminated|geo.?block|"
            r"geo.?restrict|not available in your country",
            re.IGNORECASE,
        ),
        (
            "O vídeo não existe mais ou está bloqueado nesta região. Confira o link "
            "no navegador antes de tentar de novo."
        ),
    ),
    (
        re.compile(
            r"unsupported url|unable to extract|no video|no media|found no|no formats",
            re.IGNORECASE,
        ),
        (
            "Nenhum vídeo foi encontrado nessa página. Ela pode montar o player por "
            "JavaScript: abra o link no navegador, copie a URL direta do arquivo "
            "(.mp4 ou .m3u8) ou a URL do player embutido, e tente com ela."
        ),
    ),
)

_DEFAULT_HINT: Final = (
    "Confira se o link abre no navegador sem login. Se abrir, copie a URL direta "
    "do vídeo (.mp4 ou .m3u8) e tente com ela."
)

# Falha de DRM, login ou vídeo removido não muda de resultado com outro
# extractor. Qualquer outra vale uma segunda tentativa com o genérico.
_NO_RETRY: Final = tuple(pattern for pattern, _ in _FAILURE_HINTS[:3])


class Chapter(TypedDict):
    """Capítulo declarado pelo autor do vídeo."""

    title: str
    start: float
    end: float


class VideoInfo(TypedDict):
    """Metadados de um vídeo antes do download.

    Campos como ``channel`` e ``view_count`` só existem em sites que os
    publicam; em páginas comuns vêm nulos.
    """

    id: str
    title: str
    extractor: str
    channel: str | None
    duration: float
    view_count: int | None
    upload_date: str | None
    description: str
    thumbnail: str | None
    chapters: list[Chapter]


class _YtdlInstance(Protocol):
    def __enter__(self) -> Self: ...
    def __exit__(self, *args: object) -> None: ...
    def extract_info(self, url: str, download: bool) -> dict[str, object] | None: ...
    def prepare_filename(self, info: dict[str, object]) -> str: ...


class _YtdlModule(Protocol):
    def YoutubeDL(self, params: dict[str, object]) -> _YtdlInstance: ...  # noqa: N802

    class utils:  # noqa: N801
        class DownloadError(Exception): ...


def _load_ytdlp() -> _YtdlModule:
    try:
        return cast("_YtdlModule", importlib.import_module("yt_dlp"))
    except ModuleNotFoundError as exc:
        raise ToolError(
            "Download indisponível: yt-dlp não instalado.",
            code="unavailable",
            hint="Instale com: uv sync",
        ) from exc


def validate_url(url: str) -> str:
    """Garante que a URL é um endereço http(s) que dá para buscar.

    Qualquer site é aceito. A checagem existe só para barrar esquemas que não
    são download da web: ``file://`` faria o yt-dlp ler o disco fora do
    workspace, o que quebraria a fronteira do projeto.

    Raises:
        ToolError: Se a URL não for http(s).
    """
    cleaned = url.strip()
    parsed = urlparse(cleaned)
    if parsed.scheme not in _ALLOWED_SCHEMES or not parsed.netloc:
        raise ToolError(
            f"'{url}' não é uma URL http(s) válida.",
            code="invalid_argument",
            hint="Informe o endereço completo da página do vídeo, começando com https://.",
        )
    return cleaned


def safe_title(title: str) -> str:
    """Converte o título do vídeo em um nome de arquivo seguro."""
    cleaned = _SAFE_TITLE.sub("_", title).strip("_")[:_MAX_TITLE].strip("_")
    return cleaned or "video"


@dataclass(frozen=True, slots=True)
class Downloader:
    """Consulta e baixa vídeos de qualquer site, com erros amigáveis."""

    ffmpeg_bin: str = "ffmpeg"
    timeout_seconds: float = 900.0

    def _base_params(self) -> dict[str, object]:
        params: dict[str, object] = {
            "quiet": True,
            "no_warnings": True,
            "noprogress": True,
            "socket_timeout": min(self.timeout_seconds, 60.0),
            "noplaylist": True,
        }
        # O yt-dlp exige um caminho real em ffmpeg_location. Um nome nu como
        # "ffmpeg" é tratado como inexistente e o merge de vídeo+áudio falha.
        ffmpeg = shutil.which(self.ffmpeg_bin)
        if ffmpeg is not None:
            params["ffmpeg_location"] = ffmpeg
        return params

    def info(self, url: str) -> VideoInfo:
        """Lê metadados do vídeo sem baixar.

        Raises:
            ToolError: Se o vídeo não puder ser acessado.
        """
        params = self._base_params()
        params["skip_download"] = True
        data = self._extract(params, validate_url(url), download=False)
        return parse_info(data)

    def download(self, url: str, target_dir: Path, quality: VideoQuality = "720p") -> Path:
        """Baixa o vídeo para ``target_dir`` e devolve o caminho do arquivo final.

        Raises:
            ToolError: Se o download falhar.
        """
        target_dir.mkdir(parents=True, exist_ok=True)
        params = self._base_params()
        params["format"] = _FORMATS[quality]
        params["outtmpl"] = str(target_dir / f"%(title).{_MAX_TITLE}B_%(id).{_MAX_ID}B.%(ext)s")
        params["restrictfilenames"] = True
        if quality == "audio":
            params["postprocessors"] = [
                {"key": "FFmpegExtractAudio", "preferredcodec": "m4a"},
            ]
        else:
            params["merge_output_format"] = "mp4"
        started = time.time()
        data = self._extract(params, validate_url(url), download=True)
        return _final_path(data, target_dir, started)

    def _extract(self, params: dict[str, object], url: str, *, download: bool) -> dict[str, object]:
        ytdl = _load_ytdlp()
        try:
            return _run_extract(ytdl, params, url, download=download)
        except ToolError as exc:
            # DRM, login e vídeo removido não mudam de resultado com outra
            # tentativa. Falha do extractor, sim: vale seguir para as próximas.
            if not _worth_generic_retry(exc.message):
                raise
            failure = exc
        # O genérico ignora quem "conhece" o site, lê o HTML e procura a fonte
        # do vídeo por conta própria.
        try:
            return _run_extract(
                ytdl, {**params, "force_generic_extractor": True}, url, download=download
            )
        except ToolError:
            pass
        # Nem o genérico achou. Varre a página aqui e tenta cada candidato: é o
        # que resolve portais que escondem o arquivo dentro de um iframe.
        last = failure
        for candidate in discover_sources(url, timeout=min(self.timeout_seconds, _PAGE_TIMEOUT)):
            try:
                return _run_extract(ytdl, params, candidate, download=download)
            except ToolError as exc:
                # O vídeo foi encontrado e mesmo assim não veio. Esse motivo diz
                # mais ao agente do que o "Unsupported URL" da página.
                last = exc
        raise last


def _run_extract(
    ytdl: _YtdlModule, params: dict[str, object], url: str, *, download: bool
) -> dict[str, object]:
    try:
        with ytdl.YoutubeDL(params) as instance:
            data = instance.extract_info(url, download=download)
    except ytdl.utils.DownloadError as exc:
        message = _clean_error(str(exc))
        raise ToolError(
            f"yt-dlp falhou: {message}", code="download_failed", hint=_hint_for(message)
        ) from exc
    if data is None:
        raise ToolError(
            "yt-dlp não devolveu informações do vídeo.",
            code="download_failed",
            hint=_DEFAULT_HINT,
        )
    return data


def _worth_generic_retry(message: str) -> bool:
    return not any(pattern.search(message) for pattern in _NO_RETRY)


def _hint_for(message: str) -> str:
    for pattern, hint in _FAILURE_HINTS:
        if pattern.search(message):
            return hint
    return _DEFAULT_HINT


def _clean_error(raw: str) -> str:
    return raw.replace("ERROR: ", "").strip()[-500:]


class _SourceFinder(HTMLParser):
    """Coleta, de uma página HTML, as mídias diretas e os iframes."""

    def __init__(self) -> None:
        """Começa com as duas listas vazias."""
        super().__init__(convert_charrefs=True)
        self.media: list[str] = []
        self.frames: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        """Guarda o ``src`` das tags que costumam apontar para o vídeo."""
        is_media = tag in _MEDIA_TAGS
        if not is_media and tag not in _FRAME_TAGS:
            return
        values = {name: value for name, value in attrs if value}
        source = values.get("src") or values.get("data-src")
        if source is None:
            return
        target = self.media if is_media else self.frames
        target.append(source)


def discover_sources(url: str, *, timeout: float) -> list[str]:
    """Procura na própria página as URLs reais do vídeo.

    Último recurso, quando nenhum extractor do yt-dlp reconhece o site. Junta as
    mídias diretas da página, os iframes e as mídias que estiverem dentro deles.

    Args:
        url: Página a inspecionar.
        timeout: Segundos de espera por requisição.

    Returns:
        Candidatos sem repetição, do mais provável para o menos provável.
    """
    media, frames = _scan(url, timeout)
    inner: list[str] = []
    for frame in frames[:_MAX_FRAMES]:
        found, _ = _scan(frame, timeout)
        inner.extend(found)
    return _unique([*media, *frames, *inner])[:_MAX_CANDIDATES]


def _scan(url: str, timeout: float) -> tuple[list[str], list[str]]:
    """Devolve as mídias diretas e os iframes de uma página, como URLs absolutas."""
    page = _fetch(url, timeout)
    if page is None:
        return [], []
    finder = _SourceFinder()
    finder.feed(page)
    media = [urljoin(url, item) for item in finder.media]
    # Player montado por JavaScript deixa a URL solta no meio do script, fora de
    # qualquer tag: a tag sozinha não basta.
    media.extend(_MEDIA_URL.findall(unescape(page)))
    frames = [urljoin(url, item) for item in finder.frames]
    return _http_only(media), _http_only(frames)


def _fetch(url: str, timeout: float) -> str | None:
    """Baixa o HTML de uma página. Devolve ``None`` quando não dá para ler."""
    if urlparse(url).scheme not in _ALLOWED_SCHEMES:
        return None
    request = Request(url, headers={"User-Agent": _USER_AGENT})  # noqa: S310
    try:
        # S310: o esquema é validado logo acima, só http(s) chega aqui.
        with urlopen(request, timeout=timeout) as response:  # noqa: S310
            raw: bytes = response.read(_MAX_PAGE_BYTES)
            charset = str(response.headers.get_content_charset() or "utf-8")
    except OSError, ValueError, HTTPException:
        return None
    return raw.decode(charset, errors="replace")


def _http_only(urls: Iterable[str]) -> list[str]:
    """Descarta candidatos que não sejam http(s), como ``javascript:`` e ``data:``."""
    return [item for item in urls if urlparse(item).scheme in _ALLOWED_SCHEMES]


def _unique(urls: Iterable[str]) -> list[str]:
    """Remove repetidos preservando a ordem."""
    return list(dict.fromkeys(urls))


def _final_path(data: dict[str, object], target_dir: Path, started: float) -> Path:
    """Descobre o arquivo que o yt-dlp acabou de gravar.

    Args:
        data: Retorno do ``extract_info``.
        target_dir: Pasta de destino do download.
        started: Instante em que o download começou, para reconhecer o que é novo.
    """
    reported = _reported_path(data)
    if reported is not None:
        return reported
    # O extractor genérico nem sempre preenche requested_downloads, e o nome do
    # arquivo já passou pelo saneamento do yt-dlp. Sobra olhar o que apareceu na
    # pasta durante este download.
    fresh = [
        item
        for item in target_dir.iterdir()
        if item.is_file()
        and item.suffix != ".part"
        and item.stat().st_mtime >= started - _MTIME_SLACK
    ]
    if fresh:
        return max(fresh, key=lambda item: item.stat().st_mtime)
    raise ToolError(
        "Download terminou mas o arquivo final não foi encontrado.",
        code="download_failed",
        hint="Use list_files na pasta de destino para procurar o arquivo baixado.",
    )


def _reported_path(data: dict[str, object]) -> Path | None:
    downloads = data.get("requested_downloads")
    if isinstance(downloads, list):
        for item in downloads:
            if isinstance(item, dict):
                filepath = item.get("filepath")
                if isinstance(filepath, str) and filepath:
                    return Path(filepath)
    filepath = data.get("filepath")
    if isinstance(filepath, str) and filepath:
        return Path(filepath)
    return None


def parse_info(data: dict[str, object]) -> VideoInfo:
    """Converte o dicionário bruto do yt-dlp em ``VideoInfo``."""
    raw_chapters = data.get("chapters")
    chapters: list[Chapter] = []
    if isinstance(raw_chapters, list):
        for item in raw_chapters:
            if not isinstance(item, dict):
                continue
            chapters.append(
                Chapter(
                    title=str(item.get("title") or ""),
                    start=float(_num(item.get("start_time"))),
                    end=float(_num(item.get("end_time"))),
                )
            )
    return VideoInfo(
        id=str(data.get("id") or ""),
        title=str(data.get("title") or "video"),
        extractor=str(data.get("extractor_key") or data.get("extractor") or "Generic"),
        channel=_opt_str(data.get("channel") or data.get("uploader")),
        duration=float(_num(data.get("duration"))),
        view_count=_opt_int(data.get("view_count")),
        upload_date=_opt_str(data.get("upload_date")),
        description=str(data.get("description") or ""),
        thumbnail=_opt_str(data.get("thumbnail")),
        chapters=chapters,
    )


def _num(value: object) -> float:
    return float(value) if isinstance(value, (int, float)) else 0.0


def _opt_str(value: object) -> str | None:
    return value if isinstance(value, str) and value else None


def _opt_int(value: object) -> int | None:
    return value if isinstance(value, int) else None
