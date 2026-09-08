from __future__ import annotations

import os
import time
from pathlib import Path
from typing import Self

import pytest

from core.downloader import (
    Downloader,
    _final_path,
    _hint_for,
    _http_only,
    _scan,
    _worth_generic_retry,
    discover_sources,
    parse_info,
    safe_title,
    validate_url,
)
from core.errors import ToolError


class _FakeDownloadError(Exception):
    pass


class _FakeUtils:
    DownloadError = _FakeDownloadError


class _FakeInstance:
    def __init__(self, module: _FakeYtdl, forced: bool) -> None:
        self._module = module
        self._forced = forced

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *args: object) -> None:
        del args

    def extract_info(self, url: str, download: bool) -> dict[str, object] | None:
        del download
        return self._module.answer(url, forced=self._forced)

    def prepare_filename(self, info: dict[str, object]) -> str:
        del info
        return ""


class _FakeYtdl:
    """yt-dlp de mentira. Responde diferente conforme o extractor genérico for forçado.

    Uma resposta ``str`` vira ``DownloadError`` com aquela mensagem; ``dict`` é sucesso.
    """

    utils = _FakeUtils

    def __init__(
        self,
        native: dict[str, object] | str,
        generic: dict[str, object] | str,
        por_url: dict[str, dict[str, object] | str] | None = None,
    ) -> None:
        self.native = native
        self.generic = generic
        self.por_url = por_url or {}
        self.calls: list[bool] = []
        self.urls: list[str] = []

    def YoutubeDL(self, params: dict[str, object]) -> _FakeInstance:  # noqa: N802
        return _FakeInstance(self, bool(params.get("force_generic_extractor")))

    def answer(self, url: str, *, forced: bool) -> dict[str, object]:
        self.calls.append(forced)
        self.urls.append(url)
        reply = self.por_url.get(url, self.generic if forced else self.native)
        if isinstance(reply, str):
            raise _FakeDownloadError(reply)
        return reply


@pytest.fixture(autouse=True)
def _sem_rede(monkeypatch: pytest.MonkeyPatch) -> None:
    """Nenhum teste deste módulo busca página de verdade."""
    monkeypatch.setattr("core.downloader._fetch", lambda _url, _timeout: None)


def _install(monkeypatch: pytest.MonkeyPatch, fake: _FakeYtdl) -> None:
    monkeypatch.setattr("core.downloader._load_ytdlp", lambda: fake)


@pytest.mark.parametrize(
    "url",
    [
        "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        "https://youtu.be/dQw4w9WgXcQ",
        "  https://m.youtube.com/watch?v=abc  ",
        "https://eaulas.usp.br/portal/video?idItem=2345",
        "https://vimeo.com/123",
        "http://exemplo.com.br/aula/parte-1",
        "https://cdn.exemplo.com/videos/aula.mp4",
        "https://stream.exemplo.com/live/playlist.m3u8",
        "http://localhost:8080/video",
    ],
)
def test_validate_url_accepts_any_http_source(url: str) -> None:
    assert validate_url(url) == url.strip()


@pytest.mark.parametrize(
    "url",
    ["ftp://exemplo.com/x", "file:///etc/passwd", "abc", "", "https://", "www.exemplo.com/x"],
)
def test_validate_url_rejects_non_http(url: str) -> None:
    with pytest.raises(ToolError) as exc:
        validate_url(url)
    assert exc.value.code == "invalid_argument"


def test_safe_title() -> None:
    assert safe_title("Olá, mundo! (2024) / teste") == "Ol_mundo_2024_teste"
    assert safe_title("///") == "video"
    assert len(safe_title("a" * 200)) == 60


def test_parse_info_full() -> None:
    info = parse_info(
        {
            "id": "abc",
            "title": "Título",
            "extractor_key": "Youtube",
            "channel": "Canal",
            "duration": 120,
            "view_count": 10,
            "upload_date": "20240101",
            "description": "desc",
            "thumbnail": "http://x/y.jpg",
            "chapters": [{"title": "Intro", "start_time": 0, "end_time": 10}, "lixo"],
        }
    )
    assert info["title"] == "Título"
    assert info["extractor"] == "Youtube"
    assert info["duration"] == 120.0
    assert info["chapters"] == [{"title": "Intro", "start": 0.0, "end": 10.0}]


def test_parse_info_missing_fields() -> None:
    info = parse_info({})
    assert info["title"] == "video"
    assert info["extractor"] == "Generic"
    assert info["channel"] is None
    assert info["duration"] == 0.0
    assert info["chapters"] == []


@pytest.mark.parametrize(
    ("message", "trecho"),
    [
        ("This video is DRM protected", "DRM"),
        ("Private video. Sign in if you've been granted access", "login"),
        ("Video unavailable. This video has been removed", "não existe mais"),
        ("Unsupported URL: https://exemplo.com/pagina", "Nenhum vídeo foi encontrado"),
        ("Connection reset by peer", "abre no navegador"),
    ],
)
def test_hint_for_distingue_causas(message: str, trecho: str) -> None:
    assert trecho in _hint_for(message)


@pytest.mark.parametrize(
    ("message", "esperado"),
    [
        ("Unsupported URL: https://exemplo.com", True),
        ("Unable to extract player data", True),
        ("Connection reset by peer", True),
        ("This video is DRM protected", False),
        ("Please sign in to continue", False),
        ("Video unavailable, it was removed", False),
    ],
)
def test_worth_generic_retry(message: str, esperado: bool) -> None:
    assert _worth_generic_retry(message) is esperado


def test_info_usa_extractor_generico_quando_o_nativo_falha(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake = _FakeYtdl(
        native="Unsupported URL: https://eaulas.usp.br/portal/video?idItem=2345",
        generic={"id": "2345", "title": "Aula de matemática", "extractor_key": "Generic"},
    )
    _install(monkeypatch, fake)
    info = Downloader().info("https://eaulas.usp.br/portal/video?idItem=2345")
    assert info["title"] == "Aula de matemática"
    assert info["extractor"] == "Generic"
    assert fake.calls == [False, True]


def test_info_nao_repete_quando_exige_login(monkeypatch: pytest.MonkeyPatch) -> None:
    fake = _FakeYtdl(native="Private video. Sign in to continue", generic={"id": "x"})
    _install(monkeypatch, fake)
    with pytest.raises(ToolError) as exc:
        Downloader().info("https://exemplo.com/aula")
    assert exc.value.code == "download_failed"
    assert exc.value.hint is not None
    assert "login" in exc.value.hint
    assert fake.calls == [False]


def test_info_devolve_o_erro_do_nativo_quando_os_dois_falham(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake = _FakeYtdl(native="Unable to extract player data", generic="Unsupported URL")
    _install(monkeypatch, fake)
    with pytest.raises(ToolError) as exc:
        Downloader().info("https://exemplo.com/aula")
    assert "Unable to extract player data" in exc.value.message
    assert fake.calls == [False, True]


def test_info_sem_segunda_tentativa_quando_o_nativo_funciona(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake = _FakeYtdl(native={"id": "abc", "title": "Ok"}, generic="nunca chamado")
    _install(monkeypatch, fake)
    assert Downloader().info("https://youtu.be/abc")["title"] == "Ok"
    assert fake.calls == [False]


def test_final_path_usa_requested_downloads(tmp_path: Path) -> None:
    alvo = tmp_path / "video.mp4"
    alvo.touch()
    data: dict[str, object] = {"requested_downloads": [{"filepath": str(alvo)}]}
    assert _final_path(data, tmp_path, time.time()) == alvo


def test_final_path_usa_filepath_de_topo(tmp_path: Path) -> None:
    alvo = tmp_path / "video.mkv"
    alvo.touch()
    assert _final_path({"filepath": str(alvo)}, tmp_path, time.time()) == alvo


def test_final_path_pega_o_arquivo_novo_quando_o_extractor_nao_informa(tmp_path: Path) -> None:
    antigo = tmp_path / "de_outro_download.mp4"
    antigo.touch()
    os.utime(antigo, (1000, 1000))
    started = time.time()
    alvo = tmp_path / "Aula_de_matematica.mkv"
    alvo.touch()
    assert _final_path({}, tmp_path, started) == alvo


def test_final_path_ignora_arquivo_parcial(tmp_path: Path) -> None:
    started = time.time()
    (tmp_path / "video.mp4.part").touch()
    with pytest.raises(ToolError) as exc:
        _final_path({}, tmp_path, started)
    assert exc.value.code == "download_failed"


def test_final_path_sem_arquivo(tmp_path: Path) -> None:
    with pytest.raises(ToolError) as exc:
        _final_path({"id": "2345"}, tmp_path, time.time())
    assert exc.value.code == "download_failed"


PAGINA = "https://eaulas.usp.br/portal/video?idItem=2345"
EMBED = "https://eaulas.usp.br/portal/embed-video?idItem=2345"
MP4 = "https://cdn.eaulas.usp.br/route/2345/1350914719965.mp4?s=abc&v=0"


def _paginas(monkeypatch: pytest.MonkeyPatch, mapa: dict[str, str]) -> list[str]:
    """Serve HTML fixo no lugar da rede e devolve a lista de URLs buscadas."""
    buscadas: list[str] = []

    def fake_fetch(url: str, _timeout: float) -> str | None:
        buscadas.append(url)
        return mapa.get(url)

    monkeypatch.setattr("core.downloader._fetch", fake_fetch)
    return buscadas


def test_scan_acha_video_source_e_iframe(monkeypatch: pytest.MonkeyPatch) -> None:
    _paginas(
        monkeypatch,
        {
            PAGINA: (
                '<video src="/media/aula.mp4"></video>'
                '<source src="https://cdn.x/y.webm">'
                f'<iframe src="{EMBED}"></iframe>'
            )
        },
    )
    media, frames = _scan(PAGINA, 5.0)
    assert "https://eaulas.usp.br/media/aula.mp4" in media
    assert "https://cdn.x/y.webm" in media
    assert frames == [EMBED]


def test_scan_acha_url_solta_no_javascript(monkeypatch: pytest.MonkeyPatch) -> None:
    _paginas(monkeypatch, {PAGINA: f"<script>var p = {{ src: '{MP4}' }};</script>"})
    media, frames = _scan(PAGINA, 5.0)
    assert media == [MP4]
    assert frames == []


def test_scan_descarta_esquemas_que_nao_sao_http(monkeypatch: pytest.MonkeyPatch) -> None:
    _paginas(
        monkeypatch,
        {
            PAGINA: (
                '<iframe src="javascript:void(0)"></iframe><video src="data:video/mp4;base64,AA">'
            )
        },
    )
    assert _scan(PAGINA, 5.0) == ([], [])


def test_scan_pagina_inacessivel(monkeypatch: pytest.MonkeyPatch) -> None:
    _paginas(monkeypatch, {})
    assert _scan(PAGINA, 5.0) == ([], [])


def test_discover_sources_desce_um_nivel_no_iframe(monkeypatch: pytest.MonkeyPatch) -> None:
    buscadas = _paginas(
        monkeypatch,
        {
            PAGINA: f'<iframe src="{EMBED}"></iframe>',
            EMBED: f"<script>src: '{MP4}'</script>",
        },
    )
    # O iframe vem antes da mídia de dentro dele: pode ser um embed conhecido.
    assert discover_sources(PAGINA, timeout=5.0) == [EMBED, MP4]
    assert buscadas == [PAGINA, EMBED]


def test_discover_sources_sem_repetidos(monkeypatch: pytest.MonkeyPatch) -> None:
    _paginas(monkeypatch, {PAGINA: f'<video src="{MP4}"></video><script>"{MP4}"</script>'})
    assert discover_sources(PAGINA, timeout=5.0) == [MP4]


def test_http_only_filtra() -> None:
    assert _http_only(["https://a/b", "http://c/d", "data:x", "javascript:void(0)", "//x/y"]) == [
        "https://a/b",
        "http://c/d",
    ]


def test_info_varre_a_pagina_quando_nenhum_extractor_reconhece(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake = _FakeYtdl(
        native="Unsupported URL",
        generic="Unsupported URL",
        por_url={MP4: {"id": "x", "title": "Aula", "extractor_key": "Generic"}},
    )
    _install(monkeypatch, fake)
    _paginas(monkeypatch, {PAGINA: f'<video src="{MP4}"></video>'})
    assert Downloader().info(PAGINA)["title"] == "Aula"
    assert fake.urls == [PAGINA, PAGINA, MP4]


def test_info_erro_do_candidato_ganha_do_erro_da_pagina(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake = _FakeYtdl(
        native="Unsupported URL", generic="Unsupported URL", por_url={MP4: "HTTP Error 403"}
    )
    _install(monkeypatch, fake)
    _paginas(monkeypatch, {PAGINA: f'<video src="{MP4}"></video>'})
    with pytest.raises(ToolError) as exc:
        Downloader().info(PAGINA)
    assert "403" in exc.value.message


def test_info_erro_da_pagina_quando_a_varredura_nao_acha_nada(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    fake = _FakeYtdl(native="Unsupported URL", generic="Unsupported URL")
    _install(monkeypatch, fake)
    _paginas(monkeypatch, {PAGINA: "<html><body>sem vídeo</body></html>"})
    with pytest.raises(ToolError) as exc:
        Downloader().info(PAGINA)
    assert "Unsupported URL" in exc.value.message
    assert exc.value.hint is not None
    assert "Nenhum vídeo foi encontrado" in exc.value.hint
