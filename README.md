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
    "media": {
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
| video | `concat_videos` | Junta vídeos em sequência |
| video | `detect_scenes` | Encontra mudanças de cena |
| video | `extract_frame` | Salva um frame como imagem |
| video | `add_text_overlay` | Escreve título ou descrição sobre a imagem |
| video | `create_subtitles` | Gera um .srt a partir de trechos com tempos |
| video | `burn_subtitles` | Grava legendas de um .srt no vídeo |
| video | `set_video_metadata` | Embute título, descrição e autor no arquivo |
| video | `add_narration` | Mistura um áudio de narração no vídeo |
| video | `list_templates` | Lista os templates visuais disponíveis |
| video | `apply_template` | Shorts 9:16, quadrado, 16:9, título de abertura, marca d'água |
| video | `add_banner` | Faixa com fundo colorido e texto no topo ou rodapé, o tempo todo ou num intervalo |
| audio | `extract_audio` | Separa a trilha de áudio |
| audio | `transcribe_audio` | Transcreve fala com timestamps (extra `transcribe`) |
| youtube | `get_youtube_info` | Título, duração, descrição e capítulos, sem baixar |
| youtube | `download_youtube_video` | Baixa o vídeo (yt-dlp) para o workspace |
| jobs | `job_status` | Estado de um job em background |
| jobs | `job_result` | Saída de um job concluído |

Ative só o que precisa com `MCP_DOMAINS=video,files`.

## Fluxo: do link do YouTube ao corte publicado

O agente orquestra, o servidor executa. Um roteiro típico:

1. `get_youtube_info` para ver duração, descrição e capítulos.
2. `download_youtube_video` (background) e `job_result` para pegar o arquivo.
3. `transcribe_audio`, `detect_scenes` e `extract_frame` para "assistir" e escolher os trechos.
4. `cut_video` com os minutos escolhidos.
5. `create_subtitles` + `burn_subtitles` para legendar, `add_text_overlay` para o título,
   `add_narration` para a locução, `apply_template` para o formato da rede.
6. `set_video_metadata` com título e descrição finais.

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
