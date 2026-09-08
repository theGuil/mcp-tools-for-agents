# Arquitetura do projeto (versão Rust)

Siga exatamente esta estrutura. Não invente camadas, não mova responsabilidades.
É a mesma arquitetura da versão Python, com a árvore na raiz do crate:
`server.rs`, `config.rs`, `core/`, `domains/`, `tests/` (o `Cargo.toml` aponta
`lib.rs` e `main.rs` para cá em vez de `src/`).

## Camadas e o que cada uma pode fazer

| Camada | Responsabilidade | Pode importar | NÃO pode |
|---|---|---|---|
| `config.rs` | Ler `std::env` e montar `Settings` | `core::errors` | Ser lido por ninguém além de `server.rs`, `domains/mod.rs` (tipo `Runtime`) e testes |
| `core/` | ffmpeg, paths, jobs, errors, binários externos, rede | só `core` | Importar `rmcp`, `config` ou `domains` |
| `domains/<area>/<tool>.rs` | Uma tool: valida, chama `core`, devolve struct `Serialize` | `core`, `domains::{McpServer, Runtime}` | Importar outro domínio, ler `std::env`, chamar `std::process` |
| `domains/<area>/mod.rs` | `register(mcp, runtime)` chamando o `register` de cada tool | os arquivos do próprio domínio | Ter lógica |
| `domains/mod.rs` | `Runtime`, `McpServer` (fachada do `rmcp`) e `register_domains` | tudo | Ter lógica de tool |
| `server.rs` | Montar `Runtime`, implementar `ServerHandler` e subir o stdio | tudo | Ter lógica de tool |
| `tests/` | Espelhar `core/` e `domains/` | tudo | — |

Se algo é comum a dois domínios, vai para `core/`. Nunca um domínio importa o outro.
Só `core/process.rs` chama `std::process`; só `core/http.rs` cria cliente HTTP.

## Anatomia obrigatória de uma tool

Um arquivo por tool em `domains/<area>/<nome>.rs`. Sempre estas quatro partes, nesta ordem:

```rust
//! Tool `nome_da_tool`: uma frase do que faz.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::core::errors::{guarded, ErrorCode, ToolError, ToolResult};
use crate::domains::{McpServer, Runtime};

// 1. Resultado tipado (o TypedDict do Python)
/// Uma frase.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NomeDaToolResult {
    pub output: String,
}

// 2. Implementação pura, recebe &Runtime, devolve ToolResult (o @guarded do Python
//    é o `Result`: Err(ToolError) vira ErrorPayload no registro)
/// Implementação pura, testável sem MCP.
pub fn nome_da_tool(runtime: &Runtime, path: &str) -> ToolResult<NomeDaToolResult> {
    let source = runtime.workspace.existing(path)?;
    // ...
    Ok(NomeDaToolResult { output: runtime.workspace.relative(&target) })
}

// 3. Parâmetros: os `///` de cada campo são o "Args:" da docstring, o agente lê
/// Parâmetros da tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Params {
    /// Arquivo, relativo ao workspace.
    pub path: String,
}

// 4. register expõe no servidor. A description é o que o agente lê.
/// Expõe a tool no servidor.
pub fn register(mcp: &mut McpServer, runtime: &Arc<Runtime>) {
    let runtime = Arc::clone(runtime);
    mcp.tool(
        "nome_da_tool",
        "O que faz, em uma frase, para o agente.\n\n\
         Quando usar, o que acontece com o original, o que devolve.",
        move |params: Params| guarded(nome_da_tool(&runtime, &params.path)),
    );
}
```

5. Adicionar `pub mod nome;` e a chamada `nome::register(mcp, runtime)` no `mod.rs` do domínio.
6. Conferir o `.mcp.json` na raiz (ver seção "Registro no Claude Code" no `CLAUDE.md`).

Parâmetros opcionais levam `#[serde(default)]` (ou `#[serde(default = "fn")]` para
valores não nulos). Enumerações de texto (`"9:16" | "1:1"`) viram `enum` com
`#[serde(rename_all = ...)]` ou `#[serde(rename = "...")]` e `JsonSchema`.

## Tool com operação longa

Separe o trabalho em `do_xxx(runtime, ...)` e ofereça `background: bool`. A
função pura recebe `&Arc<Runtime>` (o job precisa de uma cópia) e devolve
`ToolResult<MaybeJob<Result>>`:

```rust
pub fn cut_video(runtime: &Arc<Runtime>, path: &str, start: f64, end: f64, reencode: bool, background: bool)
    -> ToolResult<MaybeJob<CutVideoResult>> {
    if background {
        let runtime_job = Arc::clone(runtime);
        let path = path.to_string();
        return Ok(MaybeJob::Job(runtime.jobs.submit("cut_video", move || {
            do_cut(&runtime_job, &path, start, end, reencode)
        })));
    }
    Ok(MaybeJob::Done(do_cut(runtime, path, start, end, reencode)?))
}
```

`MaybeJob` serializa sem envelope: o agente recebe o resultado ou `{job_id, status, tool}`.

## Erros

- Falha esperada (arquivo não existe, argumento inválido, ffmpeg falhou):
  `return Err(ToolError::with_hint(msg, ErrorCode::X, hint))` ou `ToolError::new(msg, code)`.
- Códigos existem em `core/errors.rs` (`ErrorCode`). Só crie um novo se nenhum servir.
- `hint` sempre diz o que o agente deve fazer a seguir. Ex: "Use probe_video para conferir a duração."
- Qualquer outra falha é bug: nunca `unwrap()`/`expect()` em tool; propague com `?` ou converta em `ToolError`.

## Caminhos

- Entrada do agente: `runtime.workspace.existing(path)?` (precisa existir) ou `.resolve(path)?` (pode não existir).
- Saída ao lado do original: `runtime.workspace.output_for(&source, "tag", None)` (ou `Some("png")`).
- Devolver ao agente: sempre `runtime.workspace.relative(&path)`, nunca caminho absoluto.

## FFmpeg

- Só `core/ffmpeg.rs` monta a chamada. Tool usa `runtime.ffmpeg.run(args)?` e `runtime.ffmpeg.probe(&path)?`.
- Monte os argumentos com o macro `ffargs![...]` (aceita `&str`, `String`, `&Path`, `PathBuf`).
- Números em filtros e nomes: `core::numbers::format_g` faz o `{:g}` do Python; `round_to(x, 3)` faz o `round(x, 3)`.
- Texto em `drawtext`: `core::fonts::{require_font, escape_drawtext, escape_filter_path}`.

## Binários externos e portabilidade

`core/binaries.rs` localiza ffmpeg, ffprobe e yt-dlp (variável de ambiente,
pasta do executável, PATH, cache) e baixa sob demanda quando `MCP_AUTO_DOWNLOAD`
permite. Tool nunca procura binário: chama `runtime.ffmpeg` e `runtime.downloader`.
A fonte DejaVu e o modelo YuNet vêm embutidos no binário (`include_bytes!`).

## Domínio novo

1. Criar `domains/<nome>/mod.rs` com `register(mcp, runtime)` e `pub mod <nome>;` em `domains/mod.rs`.
2. Adicionar a variante em `DomainName` no `config.rs` (e em `ALL_DOMAINS` e no `match` de `register_domains`).
3. Adicionar em `MCP_DOMAINS` no `.env.example` e no `env` do `.mcp.json`.
4. Criar `tests/domains/<nome>/mod.rs` e declarar em `tests/domains/mod.rs`.

## Testes

- Espelho: `domains/video/cut.rs` → `tests/domains/video/test_cut.rs`, declarado em `tests/domains/video/mod.rs`.
- Testar a função pura (`cut_video(&rt.runtime, ...)`), nunca o `register`.
- Fixtures em `tests/conftest.rs`: `runtime()`, `workspace()`, `jobs()`, `sample_video(&workspace)`.
- `unwrap` e `unwrap_job` em `tests/helpers.rs` para estreitar `MaybeJob`.
- Teste que precisa de ffmpeg começa com `skip_without_ffmpeg!();`.
- Sempre cobrir: caminho feliz, argumento inválido, arquivo inexistente.
- Erros: `let err = f(...).unwrap_err(); assert_eq!(err.code, ErrorCode::InvalidArgument);`.

## Qualidade

Antes de commitar, os três precisam passar:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Não adicione `#[allow(...)]` sem motivo em comentário.

## Convenções

- Docs (`//!` e `///`) em português, estilo do projeto Python (frase curta, o que faz, quando usar).
- Nomes de tool em `snake_case`, verbo primeiro: `cut_video`, `extract_audio`, `list_files`.
- Ao criar tool, atualizar a tabela de tools no `README.md`.
