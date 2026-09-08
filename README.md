# mcp-tools-for-agents

**Dê mãos ao seu agente de IA.**

Um servidor MCP local, em Python puro, que entrega ao seu agente ferramentas reais
de vídeo, áudio e arquivos. Sem API, sem rede, sem UI. O agente sobe o servidor como
subprocesso, recebe as tools e trabalha dentro de um workspace isolado.

Funciona com Claude Code, Claude Desktop, Cursor e qualquer host MCP.

## Por que esse projeto existe

Modelos são bons em decidir. São péssimos em executar `ffmpeg` na mão.
Este projeto fecha esse buraco com tools desenhadas **para o modelo ler**:

- **Erro que ensina.** Toda falha volta como `{"error", "code", "hint"}`. O `hint` diz
  ao agente o que fazer a seguir. Ele se corrige sozinho.
- **Workspace como fronteira.** Nada fora de `WORKSPACE_DIR` é lido ou escrito.
  Nem com `..`, nem com caminho absoluto.
- **Operação longa não trava.** Passe `background=true`, receba um `job_id`, acompanhe
  com `job_status`, busque com `job_result`.
- **Resposta sempre tipada.** Cada tool devolve um `TypedDict`. Zero surpresa.
- **Um arquivo por tool.** Abriu o arquivo, entendeu a tool. Criar uma nova leva minutos.

## Em 60 segundos

```bash
git clone https://github.com/theguil/mcp-tools-for-agents
cd mcp-tools-for-agents
uv sync
cp .env.example .env      # ajuste WORKSPACE_DIR
uv run mcp-tools          # servidor no ar, via stdio
```

Requisitos: Python 3.14 (o `uv` instala), FFmpeg e FFprobe no PATH.

## Plugar no agente

```json
{
  "mcpServers": {
    "mcp-tools-for-agents": {
      "command": "uv",
      "args": ["run", "--directory", "/caminho/mcp-tools-for-agents", "mcp-tools"],
      "env": { "WORKSPACE_DIR": "/dados/videos" }
    }
  }
}
```

Cole isso no `.mcp.json` (Claude Code), no `claude_desktop_config.json` (Claude Desktop)
ou no equivalente do seu host. Pronto: peça "corta os 10 primeiros segundos do intro.mp4"
e veja acontecer.

## Tools

| Domínio | Tool | O que faz |
|---|---|---|
| files | `list_files` | Lista arquivos do workspace, com filtro por tipo |
| files | `delete_file` | Apaga um arquivo |
| video | `probe_video` | Duração, resolução, fps, codecs |
| video | `cut_video` | Recorta um trecho, com ou sem re-encode |
| video | `concat_videos` | Junta vídeos em sequência, com corte seco ou 20 transições (fade, wipe, slide...) |
| video | `remove_silence` | Corta as pausas em que ninguém fala, com limiar em dB e margem |
| video | `detect_scenes` | Encontra mudanças de cena |
| video | `extract_frame` | Salva um frame como imagem |
| video | `add_text_overlay` | Escreve título ou descrição sobre a imagem |
| video | `create_subtitles` | Gera um .srt a partir de trechos com tempos |
| video | `burn_subtitles` | Grava legendas de um .srt ou .ass no vídeo, com cor, tamanho e posição |
| video | `set_video_metadata` | Embute título, descrição e autor no arquivo |
| video | `add_narration` | Mistura um áudio de narração no vídeo |
| video | `add_sound_effects` | Insere efeitos sonoros (vine boom, ding, whoosh) em instantes do vídeo, buscando no Freesound se preciso |
| video | `list_templates` | Lista os templates visuais disponíveis |
| video | `apply_template` | Shorts 9:16, quadrado, 16:9, título de abertura, marca d'água |
| video | `add_banner` | Faixa com fundo colorido e texto no topo ou rodapé, o tempo todo ou num intervalo |
| video | `add_background_music` | Música de fundo em loop, com fade e ducking automático quando há fala |
| video | `add_fade` | Fade de entrada e saída na imagem e no som |
| video | `change_speed` | Acelera ou desacelera o vídeo todo ou um trecho, mantendo o tom da voz |
| video | `zoom_video` | Punch-in de impacto ou zoom progressivo (Ken Burns) em um trecho |
| video | `create_dynamic_subtitles` | Legenda animada palavra por palavra (estilo TikTok) em .ass |
| video | `create_thumbnail` | Capa do vídeo com título grande, nos tamanhos de YouTube, Shorts e feed |
| video | `smart_crop` | Reenquadra 16:9 para 9:16 seguindo o rosto de quem fala (extra `vision`) |
| video | `export_for_platform` | Codifica o vídeo final com o preset de TikTok, Reels, Shorts, YouTube ou X |
| audio | `extract_audio` | Separa a trilha de áudio |
| audio | `transcribe_audio` | Transcreve fala com timestamps por trecho e por palavra (extra `transcribe`) |
| audio | `normalize_audio` | Normaliza o volume para o loudness da plataforma (EBU R128, duas passadas) |
| media | `get_video_info` | Título, duração, descrição e capítulos de uma URL, sem baixar |
| media | `download_video` | Baixa o vídeo de qualquer site (yt-dlp) para o workspace |
| media | `search_sound_effects` | Busca efeitos sonoros gratuitos no Freesound por descrição em texto |
| media | `download_sound_effect` | Baixa um efeito do Freesound (MP3) para a pasta `sfx/` do workspace |
| jobs | `job_status` | Estado de um job em background |
| jobs | `job_result` | Saída de um job concluído |

Ative só o que precisa com `MCP_DOMAINS=video,files`.

### Download de qualquer fonte

`download_video` e `get_video_info` aceitam qualquer URL http(s). São três
tentativas, nesta ordem:

1. **Extractor nativo do yt-dlp** — cobre mais de mil sites (YouTube, Vimeo,
   Twitch, X, TikTok, Instagram, Facebook e afins).
2. **Extractor genérico** — lê o HTML da página e procura `<video>`, `<source>`,
   HLS `.m3u8`, DASH `.mpd`, players conhecidos e JSON-LD.
3. **Varredura própria** — quando nem o genérico acha, o servidor busca a página,
   junta as mídias diretas e desce um nível nos `iframe`. É o que resolve portais
   de aula como o `eaulas.usp.br`, que escondem o MP4 dentro do player embutido.

Não há como baixar conteúdo com DRM (Netflix, Disney+, cursos com Widevine) nem
páginas que exigem login. Nesses casos o `hint` do erro diz para não insistir, em
vez de mandar o agente tentar de novo à toa.

### Efeitos sonoros do Freesound

`add_sound_effects` recebe uma lista de efeitos com o instante em que cada um
toca e aplica tudo em um único passo do ffmpeg, sem re-encodar o vídeo. Cada
efeito vem de uma de três origens:

- `query`: descrição em inglês (`"vine boom"`, `"record scratch"`, `"ding"`); a
  tool busca no [Freesound](https://freesound.org), baixa o primeiro resultado
  para `sfx/` e usa;
- `sound_id`: um resultado escolhido com `search_sound_effects`;
- `audio`: um arquivo que já está no workspace.

```json
{"path": "corte.mp4", "effects": [
  {"query": "vine boom", "start": 3.2},
  {"query": "record scratch", "start": 7.0, "volume": 0.8},
  {"audio": "sfx/risada.mp3", "start": 12.5}
]}
```

Os sons vêm do preview MP3 (128 kbps) do Freesound, que basta para vídeo
curto e não exige OAuth. A chave da API já vem embutida em `config.py`;
`FREESOUND_API_KEY` no ambiente substitui. O resultado informa a licença de
cada som: CC0 e CC BY servem para qualquer uso (CC BY pede crédito ao autor),
CC BY-NC é só para uso não comercial.

### Extras opcionais

Duas tools dependem de bibliotecas pesadas que não vêm por padrão:

```bash
uv sync --extra transcribe   # faster-whisper: transcribe_audio
uv sync --extra vision       # opencv-python-headless: smart_crop com mode="face"
```

O `smart_crop` usa o [YuNet](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet),
detector de rostos oficial do OpenCV Zoo. O modelo ONNX (230 KB, licença MIT) já vem
em `core/models`, então não há download em tempo de execução. Sem o extra, a tool
continua funcionando com `mode="center"`.

## Fluxo: do link ao corte publicado

O agente orquestra, o servidor executa. Um roteiro típico para um corte de
TikTok, Reels ou Shorts a partir de um vídeo horizontal:

1. `get_video_info` para ver duração, descrição e capítulos.
2. `download_video` (background) e `job_result` para pegar o arquivo.
3. `transcribe_audio` (com tempos por palavra), `detect_scenes` e `extract_frame`
   para "assistir" e escolher os trechos.
4. `cut_video` com os minutos escolhidos e `remove_silence` para tirar as pausas.
5. `smart_crop` para virar 9:16 seguindo o rosto (ou `apply_template` "shorts" para
   manter o quadro inteiro sobre fundo desfocado).
6. `change_speed` para acelerar partes lentas e `zoom_video` para dar ênfase nas
   frases fortes.
7. `create_dynamic_subtitles` + `burn_subtitles` para a legenda animada palavra por
   palavra, `add_text_overlay` ou `add_banner` para o título.
8. `add_narration` para locução, `add_sound_effects` para os efeitos de impacto,
   `add_background_music` para a trilha com ducking, `add_fade` para o acabamento.
9. `normalize_audio` para o volume padrão da plataforma e `export_for_platform`
   para o MP4 final.
10. `create_thumbnail` para a capa e `set_video_metadata` com título e descrição.

Para um vídeo longo de YouTube o roteiro é o mesmo sem o passo 5, com
`concat_videos` (com `transition="fade"`) para juntar os blocos e
`export_for_platform` com `youtube` ou `youtube_4k`.

## Como é por dentro

```
server.py     cria o MCPServer e registra os domínios
config.py     único lugar que lê variáveis de ambiente
core/         ffmpeg, paths, jobs, errors. Não conhece MCP.
domains/      um pacote por área, um arquivo por tool
tests/        espelha a estrutura acima
```

Uma tool inteira, do jeito que todas são:

```python
@guarded
def delete_file(runtime: Runtime, path: str) -> DeleteFileResult:
    target = runtime.workspace.existing(path)
    size = target.stat().st_size
    target.unlink()
    return DeleteFileResult(deleted=runtime.workspace.relative(target), freed_bytes=size)


def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    @mcp.tool(name="delete_file")
    def _tool(path: str) -> DeleteFileResult | ErrorPayload:
        """Apaga permanentemente um arquivo do workspace.

        Args:
            path: Arquivo, relativo ao workspace.
        """
        return delete_file(runtime, path)
```

Função pura e testável em cima, registro com docstring para o agente embaixo.
É só isso.

## Contribuir

Quer uma tool nova? Copie `domains/files/delete.py`, troque o miolo, escreva o teste
espelhado em `tests/`, adicione a linha na tabela acima. As regras completas estão em
`.claude/rules/arquitetura.md`, e se você usa Claude Code, `/nova-tool` faz o roteiro.

Antes do PR, os três precisam passar:

```bash
uv run ruff check . && uv run ruff format --check .
uv run mypy
uv run pytest
```

Ruff em `ALL`, mypy em `strict`. Rígido de propósito: é o que mantém cada tool
pequena, previsível e fácil de confiar.

## Licença

MIT.
