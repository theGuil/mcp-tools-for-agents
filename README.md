# mcp-tools-for-agents

Servidor MCP em Python puro com tools de edição de vídeo, áudio e arquivos,
feito para ser operado por um agente de IA no mesmo container, via stdio.

## Requisitos

- Python 3.14 (gerenciado pelo `uv`)
- FFmpeg e FFprobe no PATH

## Rodar

```bash
uv sync                      # instala dependências
cp .env.example .env         # ajuste WORKSPACE_DIR e afins
uv run mcp-tools             # sobe o servidor via stdio
```

## Plugar no agente

Qualquer host MCP (Claude Code, Claude Desktop, Cursor ou um agente próprio)
sobe o servidor como subprocesso:

```json
{
  "mcpServers": {
    "video": {
      "command": "uv",
      "args": ["run", "--directory", "/caminho/mcp-tools-for-agents", "mcp-tools"],
      "env": { "WORKSPACE_DIR": "/dados/videos" }
    }
  }
}
```

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
| audio | `extract_audio` | Separa a trilha de áudio |
| audio | `transcribe_audio` | Transcreve fala com timestamps (extra `transcribe`) |
| jobs | `job_status` | Estado de um job em background |
| jobs | `job_result` | Saída de um job concluído |

Toda tool devolve um dicionário tipado. Em erro, o formato é sempre
`{"error", "code", "hint"}`. Operações longas aceitam `background=true`
e devolvem um `job_id`.

## Estrutura

```
server.py     cria o MCPServer e registra os domínios
config.py     lê variáveis de ambiente
core/         ffmpeg, paths, jobs, errors. Não conhece MCP.
domains/      um pacote por área, um arquivo por tool
tests/        espelha a estrutura acima
```

## Qualidade

```bash
uv run ruff check . && uv run ruff format --check .
uv run mypy
uv run pytest
```
