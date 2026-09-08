# mcp-tools-for-agents (versão Rust)

## Objetivo

Port em Rust do servidor MCP (Model Context Protocol) da raiz do repositório, com
**a mesma arquitetura, as mesmas pastas e as mesmas tools**. A diferença é o
objetivo de distribuição: um **binário único e portátil** que roda em qualquer
máquina só baixando e dando start, sem Python, `uv`, OpenCV ou FFmpeg instalados
(o FFmpeg e o yt-dlp são localizados ou baixados sob demanda por `core/binaries.rs`;
a fonte e o modelo de rosto vão embutidos no executável).

Não é uma API web. Não tem rede além dos downloads sob demanda e do Freesound.
Não tem UI. É uma caixa de ferramentas local desenhada para ser lida e chamada
por um modelo.

## Princípios

- **Erro é mensagem para o agente.** Toda tool devolve `{"error", "code", "hint"}` em
  falha. Nada de panic. O `hint` diz o que o agente pode fazer para se corrigir.
- **Workspace é a fronteira.** Todo caminho é relativo a `WORKSPACE_DIR`. Nada fora dele
  é lido ou escrito, mesmo com `..` ou caminho absoluto.
- **Operação longa vira job.** Tools demoradas aceitam `background=true`, devolvem
  `job_id`, e o agente acompanha com `job_status` e `job_result`.
- **Retorno sempre tipado.** Toda tool devolve uma struct `Serialize`. O agente sabe o que esperar.
- **Docstring é a interface.** A `description` passada a `mcp.tool` e os `///` da struct
  `Params` são o que o modelo lê para decidir usar a tool. Escreva para o modelo, não para o dev.
- **Portátil de verdade.** Nada de dependência de sistema no binário: rustls em vez de
  OpenSSL, `tract` em vez de OpenCV, whisper.cpp linkado estaticamente.

## Arquitetura

```
server.rs     cria o McpServer, monta o Runtime e registra os domínios
config.rs     único lugar que lê variáveis de ambiente (Settings)
core/         infraestrutura: ffmpeg, paths, jobs, errors, binaries. NÃO conhece MCP.
domains/      um módulo por área (video, audio, files, jobs, media), um arquivo por tool
tests/        espelha core/ e domains/
lib.rs        declara os módulos (o Cargo.toml aponta para a raiz, não para src/)
main.rs       equivalente do __main__.py
```

Regras completas em `.claude/rules/arquitetura.md`. Para criar uma tool nova use a
skill `/nova-tool`.

## Registro no Claude Code (`.mcp.json`)

O `.mcp.json` desta pasta aponta para `./target/release/mcp-tools`. As tools são
descobertas em tempo de execução pelo `tools/list`, então uma tool nova dentro de um
domínio existente aparece sozinha. Mesmo assim, **ao criar ou alterar uma tool,
confira o `.mcp.json`** e atualize-o quando:

- a tool pertence a um **domínio novo**: inclua o nome em `MCP_DOMAINS` no `env`;
- a tool depende de uma **variável de ambiente nova** lida em `config.rs`: adicione no `env`;
- o nome do binário (`[[bin]]` em `Cargo.toml`) mudar.

Depois, valide com `/mcp` no Claude Code: o servidor `mcp-tools-for-agents` deve
aparecer conectado e a tool nova deve estar na lista.

## Comandos

```bash
cargo build --release --features full   # binário completo em target/release/mcp-tools
cargo build --release                   # binário leve (sem transcrição nem visão)
cargo run                               # sobe o servidor via stdio
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test                              # testes ffmpeg são pulados se o binário não existir
```

## Stack

- Rust estável (1.85+), edição 2021
- `rmcp` (SDK oficial do MCP), `tokio`, `serde`/`schemars`, `ureq` (rustls)
- FFmpeg/FFprobe e yt-dlp como executáveis externos, localizados ou baixados por `core/binaries.rs`
- Features opcionais: `transcribe` (whisper-rs / whisper.cpp) e `vision` (tract-onnx +
  modelo YuNet embutido em `core/models`); `full` liga as duas
- `cargo fmt`, `cargo clippy -D warnings` e `cargo test` precisam passar antes de commitar.

## Idioma

Código, docstrings, comentários, commits e documentação em português do Brasil.
Nomes de tools, funções e variáveis em inglês (padrão do ecossistema MCP).
