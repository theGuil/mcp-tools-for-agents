# mcp-tools-for-agents

**Dê mãos ao seu agente de IA. Um binário, nenhuma instalação.**

Um servidor MCP local, em Rust, que entrega ao seu agente ferramentas reais de
vídeo, áudio e arquivos. Sem API, sem UI, sem runtime. São 34 tools compiladas em
um executável único: baixe o `mcp-tools` da sua plataforma, aponte o host MCP para
ele e pronto.

Funciona com Claude Code, Claude Desktop, Cursor e qualquer host MCP.

## Por que Rust

O servidor precisa de **um arquivo**:

- **Binário único por plataforma.** Linux (x86_64, arm64), macOS (Intel, Apple
  Silicon) e Windows. Sem runtime, sem dependência de sistema.
- **FFmpeg e yt-dlp sob demanda.** Se não estiverem no PATH nem ao lado do
  binário, o servidor baixa os builds estáticos oficiais na primeira chamada e
  guarda em uma pasta de cache. Desligue com `MCP_AUTO_DOWNLOAD=false`.
- **Fonte e modelo embutidos.** A DejaVu Sans Bold (para texto e legenda) e o
  YuNet (detector de rosto do OpenCV Zoo) vão dentro do executável.
- **Transcrição e visão nativas.** `transcribe_audio` usa o whisper.cpp
  (`whisper-rs`) e `smart_crop` roda o YuNet no `tract`, runtime ONNX em Rust
  puro. Nada de OpenCV instalado.
- **Mesmos princípios.** Erro que ensina (`{error, code, hint}`), workspace
  como fronteira, jobs em background, resposta sempre tipada, um arquivo por tool.

## Em 60 segundos

Baixe o pacote da sua plataforma na [página de releases](https://github.com/theGuil/mcp-tools-for-agents/releases/latest)
(ou compile, abaixo), descompacte e rode:

```bash
WORKSPACE_DIR=/dados/videos ./mcp-tools     # servidor no ar, via stdio
```

Na primeira tool que precisar de FFmpeg (ou de yt-dlp) o binário é baixado para
o cache. Se preferir, coloque `ffmpeg`, `ffprobe` e `yt-dlp` ao lado do
`mcp-tools` ou no PATH: eles têm prioridade e nada é baixado.

### Compilar do fonte

```bash
cargo build --release --features full   # binário completo (transcrição + visão)
cargo build --release                   # binário leve, sem os extras
```

Requisitos de build: Rust estável (1.85+); com `--features full`, também `cmake`
e um compilador C++ (o whisper.cpp é compilado e linkado estaticamente). O
executável fica em `target/release/mcp-tools` (~24 MB). No Linux ele depende só
da glibc e da libstdc++ do sistema, presentes em qualquer distribuição.

## Plugar no agente

```json
{
  "mcpServers": {
    "mcp-tools-for-agents": {
      "command": "/caminho/mcp-tools",
      "env": { "WORKSPACE_DIR": "/dados/videos" }
    }
  }
}
```

Cole isso no `.mcp.json` (Claude Code), no `claude_desktop_config.json` (Claude
Desktop) ou no equivalente do seu host. Pronto: peça "corta os 10 primeiros
segundos do intro.mp4" e veja acontecer.

### Claude Code neste repositório

Abrindo este repositório no Claude Code nada precisa ser compilado: o hook
`SessionStart` em `.claude/settings.json` roda `scripts/install-mcp-tools.sh`,
que baixa o `mcp-tools` da **última release** para `bin/` (pasta ignorada pelo
git), e o `.mcp.json` já aponta para `./bin/mcp-tools`. Para atualizar à mão ou
fixar uma versão:

```bash
sh scripts/install-mcp-tools.sh                        # última release
MCP_TOOLS_VERSION=v0.1.0 sh scripts/install-mcp-tools.sh   # versão fixa
MCP_TOOLS_FORCE=true sh scripts/install-mcp-tools.sh   # baixa de novo
```

### Publicar uma release

Automático. A cada merge em `producao`, o workflow `.github/workflows/release.yml`
cria a próxima tag `vX.Y.Z`, compila o binário completo (`--features full`) para
Linux x86_64 e arm64, macOS Intel e Apple Silicon e Windows, e anexa os pacotes
à release.

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
| video | `smart_crop` | Reenquadra 16:9 para 9:16 seguindo o rosto de quem fala (feature `vision`) |
| video | `export_for_platform` | Codifica o vídeo final com o preset de TikTok, Reels, Shorts, YouTube ou X |
| audio | `extract_audio` | Separa a trilha de áudio |
| audio | `transcribe_audio` | Transcreve fala com timestamps por trecho e por palavra (feature `transcribe`) |
| audio | `normalize_audio` | Normaliza o volume para o loudness da plataforma (EBU R128, duas passadas) |
| media | `get_video_info` | Título, duração, descrição e capítulos de uma URL, sem baixar |
| media | `download_video` | Baixa o vídeo de qualquer site (yt-dlp) para o workspace |
| media | `search_sound_effects` | Busca efeitos sonoros gratuitos no Freesound por descrição em texto |
| media | `download_sound_effect` | Baixa um efeito do Freesound (MP3) para a pasta `sfx/` do workspace |
| jobs | `job_status` | Estado de um job em background |
| jobs | `job_result` | Saída de um job concluído |

Ative só o que precisa com `MCP_DOMAINS=video,files`.

## O que é baixado, e quando

O binário não traz FFmpeg nem yt-dlp dentro (são ~150 MB e ~30 MB, com licenças
próprias). Ele os procura nesta ordem e para na primeira que encontrar:

1. o caminho em `FFMPEG_BIN`, `FFPROBE_BIN` ou `YTDLP_BIN`, se for um caminho;
2. a pasta onde o `mcp-tools` está;
3. o `PATH`;
4. a pasta de cache (`MCP_CACHE_DIR`);
5. download, se `MCP_AUTO_DOWNLOAD=true` (padrão).

| O quê | De onde | Quando |
|---|---|---|
| ffmpeg + ffprobe | builds estáticos BtbN (Linux, Windows) e evermeet.cx (macOS) | primeira tool de vídeo ou áudio |
| yt-dlp | release oficial no GitHub (executável standalone, sem Python) | primeiro `download_video` / `get_video_info` |
| modelo Whisper (`ggml-<size>.bin`) | Hugging Face, repositório `ggerganov/whisper.cpp` | primeiro `transcribe_audio` com aquele `model_size` |
| fonte DejaVu | embutida; gravada no cache só se o sistema não tiver fonte | primeira tool com texto |

Cache padrão: `~/.cache/mcp-tools-for-agents` (Linux), `~/Library/Caches/mcp-tools-for-agents`
(macOS), `%LOCALAPPDATA%\mcp-tools-for-agents` (Windows). Para um pacote 100% offline,
coloque esses arquivos na pasta do binário (`ffmpeg`, `ffprobe`, `yt-dlp`,
`models/ggml-base.bin`, `fonts/DejaVuSans-Bold.ttf`).

### Download de qualquer fonte

`download_video` e `get_video_info` aceitam qualquer URL http(s). São três
tentativas, nesta ordem:

1. **Extractor nativo do yt-dlp** — cobre mais de mil sites.
2. **Extractor genérico** — lê o HTML da página e procura `<video>`, `<source>`,
   HLS `.m3u8`, DASH `.mpd`, players conhecidos e JSON-LD.
3. **Varredura própria** — o servidor busca a página, junta as mídias diretas e
   desce um nível nos `iframe`.

Não há como baixar conteúdo com DRM nem páginas que exigem login. Nesses casos o
`hint` do erro diz para não insistir.

### Efeitos sonoros do Freesound

`add_sound_effects` recebe uma lista de efeitos com o instante em que cada um
toca e aplica tudo em um único passo do ffmpeg. Cada efeito vem de `query`
(busca no Freesound), `sound_id` (resultado de `search_sound_effects`) ou
`audio` (arquivo do workspace). A chave da API já vem embutida em `config.rs`;
`FREESOUND_API_KEY` no ambiente substitui.

### Features opcionais

Duas tools dependem de bibliotecas pesadas, ligadas por feature do Cargo:

```bash
cargo build --release --features transcribe   # whisper-rs: transcribe_audio
cargo build --release --features vision       # tract + YuNet: smart_crop com mode="face"
cargo build --release --features full         # as duas (é o que a release publica)
```

Sem a feature, a tool continua registrada e devolve um erro `unavailable`
explicando como habilitar; `smart_crop` segue funcionando com `mode="center"`.

## Configuração

Todas as variáveis estão em `.env.example`. As principais:

| Variável | Padrão | Para quê |
|---|---|---|
| `YTDLP_BIN` | `yt-dlp` | Nome ou caminho do yt-dlp |
| `MCP_AUTO_DOWNLOAD` | `true` | Baixar ffmpeg, yt-dlp e modelos sob demanda |
| `MCP_CACHE_DIR` | cache do sistema | Onde os downloads ficam |

## Como é por dentro

A árvore fica na raiz do crate (o `Cargo.toml` aponta para a raiz em vez de `src/`):

```
server.rs     cria o McpServer, monta o Runtime e registra os domínios
config.rs     único lugar que lê variáveis de ambiente (Settings)
core/         ffmpeg, paths, jobs, errors, binários externos, rede. Não conhece MCP.
domains/      um módulo por área, um arquivo por tool
tests/        espelha a estrutura acima
```

Uma tool inteira, do jeito que todas são:

```rust
pub fn delete_file(runtime: &Runtime, path: &str) -> ToolResult<DeleteFileResult> {
    let target = runtime.workspace.existing(path)?;
    let size = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
    std::fs::remove_file(&target).map_err(|error| {
        ToolError::new(format!("Não foi possível apagar '{path}': {error}"), ErrorCode::NotFound)
    })?;
    Ok(DeleteFileResult { deleted: runtime.workspace.relative(&target), freed_bytes: size })
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Arquivo, relativo ao workspace.
    pub path: String,
}

pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "delete_file",
        "Apaga permanentemente um arquivo do workspace.\n\n\
         Use para limpar saídas intermediárias. Não há lixeira nem desfazer.",
        move |params: Params| guarded(delete_file(&runtime, &params.path)),
    );
}
```

Função pura e testável em cima, registro com a description para o agente
embaixo. `Err(ToolError)` vira `{error, code, hint}` na resposta.

## Contribuir

Quer uma tool nova? Copie `domains/files/delete.rs`, troque o miolo, escreva o
teste espelhado em `tests/`, adicione a linha na tabela acima. As regras
completas estão em `.claude/rules/arquitetura.md`, e se você usa Claude Code,
`/nova-tool` faz o roteiro.

Antes do PR, os três precisam passar:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Os testes que usam ffmpeg são pulados quando o binário não está instalado.

## Licença

MIT. A fonte DejaVu embutida segue a licença Bitstream Vera
(`core/fonts/LICENSE-DejaVu.txt`); o modelo YuNet é MIT (OpenCV Zoo).
