# mcp-tools-for-agents

## Objetivo

Servidor MCP (Model Context Protocol) em Python puro que expõe tools de vídeo, áudio
e arquivos para **agentes de IA usarem localmente**. O agente (Claude Code, Claude
Desktop, Cursor ou um agente próprio) sobe este servidor como subprocesso via stdio,
no mesmo container ou máquina, e passa a ter ferramentas concretas para operar
arquivos de mídia dentro de um workspace isolado.

Não é uma API web. Não tem rede. Não tem UI. É uma caixa de ferramentas local
desenhada para ser lida e chamada por um modelo.

## Princípios

- **Erro é mensagem para o agente.** Toda tool devolve `{"error", "code", "hint"}` em
  falha. Nada de stack trace. O `hint` diz o que o agente pode fazer para se corrigir.
- **Workspace é a fronteira.** Todo caminho é relativo a `WORKSPACE_DIR`. Nada fora dele
  é lido ou escrito, mesmo com `..` ou caminho absoluto.
- **Operação longa vira job.** Tools demoradas aceitam `background=true`, devolvem
  `job_id`, e o agente acompanha com `job_status` e `job_result`.
- **Retorno sempre tipado.** Toda tool devolve um `TypedDict`. O agente sabe o que esperar.
- **Docstring é a interface.** A docstring da função registrada em `@mcp.tool` é o que o
  modelo lê para decidir usar a tool. Escreva para o modelo, não para o dev.

## Arquitetura

```
server.py     cria o MCPServer, monta o Runtime e registra os domínios
config.py     único lugar que lê variáveis de ambiente (Settings)
core/         infraestrutura: ffmpeg, paths, jobs, errors. NÃO conhece MCP.
domains/      um pacote por área (video, audio, files, jobs), um arquivo por tool
tests/        espelha core/ e domains/
```

Regras completas em `.claude/rules/arquitetura.md`. Para criar uma tool nova use a
skill `/nova-tool`.

## Registro no Claude Code (`.mcp.json`)

O `.mcp.json` na raiz é o que faz o Claude Code subir este servidor ao abrir o projeto.
As tools são descobertas em tempo de execução pelo `list_tools`, então uma tool nova
dentro de um domínio existente aparece sozinha. Mesmo assim, **ao criar ou alterar uma
tool, confira o `.mcp.json`** e atualize-o quando:

- a tool pertence a um **domínio novo**: inclua o nome em `MCP_DOMAINS` no `env`;
- a tool depende de uma **variável de ambiente nova** lida em `config.py`: adicione no `env`;
- o **entrypoint** (`mcp-tools` em `pyproject.toml`) ou os argumentos de `uv run` mudarem.

Depois, valide com `/mcp` no Claude Code: o servidor `mcp-tools-for-agents` deve
aparecer conectado e a tool nova deve estar na lista.

## Comandos

```bash
uv sync                              # instala dependências
uv run mcp-tools                     # sobe o servidor via stdio
uv run ruff check . && uv run ruff format --check .
uv run mypy
uv run pytest                        # testes ffmpeg são pulados se o binário não existir
```

## Stack

- Python 3.14 gerenciado pelo `uv`
- `mcp` (SDK oficial), FFmpeg/FFprobe no PATH
- Extra opcional `transcribe` (faster-whisper)
- Ruff com `select = ["ALL"]`, mypy `strict`. Ambos precisam passar antes de commitar.

## Idioma

Código, docstrings, comentários, commits e documentação em português do Brasil.
Nomes de tools, funções e variáveis em inglês (padrão do ecossistema MCP).
