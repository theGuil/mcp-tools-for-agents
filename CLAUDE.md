# mcp-tools-for-agents

## Objetivo

Servidor MCP (Model Context Protocol) em Rust que expõe tools de vídeo, áudio e
arquivos para **agentes de IA usarem localmente**. O agente (Claude Code, Claude
Desktop, Cursor ou um agente próprio) sobe este servidor como subprocesso via stdio
e passa a ter ferramentas concretas para operar arquivos de mídia dentro de um
workspace isolado.

A distribuição é um **binário único e portátil** que roda em qualquer máquina só
baixando e dando start, sem Python, OpenCV ou FFmpeg instalados (o FFmpeg e o
yt-dlp são localizados ou baixados sob demanda por `core/binaries.rs`; a fonte e o
modelo de rosto vão embutidos no executável).

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
scripts/      install-mcp-tools.sh: baixa o binário da última release para bin/
.github/      workflows/release.yml: compila e publica a release a cada tag v*
server.rs     cria o McpServer, monta o Runtime e registra os domínios
config.rs     único lugar que lê variáveis de ambiente (Settings)
core/         infraestrutura: ffmpeg, paths, jobs, errors, binaries. NÃO conhece MCP.
domains/      um módulo por área (video, audio, files, jobs, media), um arquivo por tool
tests/        espelha core/ e domains/
lib.rs        declara os módulos (o Cargo.toml aponta para a raiz, não para src/)
main.rs       equivalente do __main__.py
```

Regras completas em `.claude/rules/arquitetura.md` e `.claude/rules/release.md`. Para criar uma tool nova use a
skill `/nova-tool`.

## Binário: sempre a última release, nunca build local

O Claude Code **não compila** este projeto para usar as tools. O binário vem da
**última release do GitHub** (`https://github.com/theGuil/mcp-tools-for-agents/releases/latest`):

1. O hook `SessionStart` em `.claude/settings.json` roda `scripts/install-mcp-tools.sh`
   no início de cada sessão. O script detecta o sistema (Linux/macOS, x86_64/arm64),
   baixa o `mcp-tools-<target>.tar.gz` da release mais recente e extrai o executável
   em `bin/mcp-tools` (pasta ignorada pelo git). Se a versão instalada já é a última,
   não baixa de novo.
2. O `.mcp.json` aponta para `./bin/mcp-tools`.
3. A release é gerada pelo workflow `.github/workflows/release.yml` quando uma tag
   `v*` é enviada: `git tag v0.1.0 && git push origin v0.1.0`. Ele compila com
   `--features full` para Linux, macOS e Windows e anexa os pacotes à release.

Regras para o agente:

- Se o servidor `mcp-tools-for-agents` não conectar, rode
  `sh scripts/install-mcp-tools.sh` e depois `/mcp`. Não substitua por
  `cargo build` no `.mcp.json`.
- Só use `cargo build --release` para **desenvolver** uma tool nova e testá-la
  localmente. O binário de `target/` não é o que o `.mcp.json` usa.
- Uma tool nova só chega ao Claude Code depois de uma **nova tag** e da release
  publicada. Ao terminar uma tool, lembre o usuário de publicar a release.
- Para forçar o download de novo: `MCP_TOOLS_FORCE=true sh scripts/install-mcp-tools.sh`.
  Para fixar uma versão: `MCP_TOOLS_VERSION=v0.1.0`.

Detalhes em `.claude/rules/release.md`.

## Registro no Claude Code (`.mcp.json`)

O `.mcp.json` da raiz aponta para `./bin/mcp-tools`, o binário baixado da última
release (ver seção acima). As tools são descobertas em tempo de execução pelo
`tools/list`, então uma tool nova dentro de um domínio existente aparece sozinha
assim que uma release nova é publicada e baixada. Mesmo assim, **ao criar ou
alterar uma tool, confira o `.mcp.json`** e atualize-o quando:

- a tool pertence a um **domínio novo**: inclua o nome em `MCP_DOMAINS` no `env`;
- a tool depende de uma **variável de ambiente nova** lida em `config.rs`: adicione no `env`;
- o nome do binário (`[[bin]]` em `Cargo.toml`) mudar.

Depois, valide com `/mcp` no Claude Code: o servidor `mcp-tools-for-agents` deve
aparecer conectado e a tool nova deve estar na lista.

## Comandos

```bash
sh scripts/install-mcp-tools.sh         # baixa a última release para bin/mcp-tools (o que o .mcp.json usa)
cargo build --release --features full   # binário completo em target/release/mcp-tools (só para desenvolver)
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
