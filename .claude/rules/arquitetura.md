# Arquitetura do projeto

Siga exatamente esta estrutura. Não invente camadas, não mova responsabilidades.

## Camadas e o que cada uma pode fazer

| Camada | Responsabilidade | Pode importar | NÃO pode |
|---|---|---|---|
| `config.py` | Ler `os.environ` e montar `Settings` | `core.errors` | Ser lido por ninguém além de `server.py` e testes |
| `core/` | ffmpeg, paths, jobs, errors | só `core` | Importar `mcp`, `config` ou `domains` |
| `domains/<area>/<tool>.py` | Uma tool: valida, chama `core`, devolve TypedDict | `core`, `domains.Runtime` | Importar outro domínio, ler `os.environ`, chamar `subprocess` |
| `domains/<area>/__init__.py` | `register(mcp, runtime)` chamando o `register` de cada tool | os arquivos do próprio domínio | Ter lógica |
| `server.py` | Montar `Runtime` e registrar domínios | tudo | Ter lógica de tool |
| `tests/` | Espelhar `core/` e `domains/` | tudo | — |

Se algo é comum a dois domínios, vai para `core/`. Nunca um domínio importa o outro.

## Anatomia obrigatória de uma tool

Um arquivo por tool em `domains/<area>/<nome>.py`. Sempre estas quatro partes, nesta ordem:

```python
"""Tool ``nome_da_tool``: uma frase do que faz."""

from __future__ import annotations

from typing import TYPE_CHECKING, TypedDict

from core.errors import ErrorPayload, ToolError, guarded

if TYPE_CHECKING:
    from mcp.server.mcpserver import MCPServer

    from domains import Runtime


# 1. Resultado tipado
class NomeDaToolResult(TypedDict):
    """Uma frase."""

    output: str


# 2. Implementação pura, decorada com @guarded, recebe Runtime como 1º argumento
@guarded
def nome_da_tool(runtime: Runtime, path: str) -> NomeDaToolResult:
    """Implementação pura, testável sem MCP."""
    source = runtime.workspace.existing(path)
    ...
    return NomeDaToolResult(output=runtime.workspace.relative(target))


# 3. register expõe no servidor. A docstring de _tool é o que o agente lê.
def register(mcp: MCPServer[None], runtime: Runtime) -> None:
    """Expõe a tool no servidor."""

    @mcp.tool(name="nome_da_tool")
    def _tool(path: str) -> NomeDaToolResult | ErrorPayload:
        """O que faz, em uma frase, para o agente.

        Quando usar, o que acontece com o original, o que devolve.

        Args:
            path: Arquivo, relativo ao workspace.
        """
        return nome_da_tool(runtime, path)
```

4. Adicionar a chamada `nome.register(mcp, runtime)` no `__init__.py` do domínio.
5. Conferir o `.mcp.json` na raiz (ver seção "Registro no Claude Code" no `CLAUDE.md`).

## Tool com operação longa

Separe o trabalho em `_do_xxx(runtime, ...)` e ofereça `background: bool = False`:

```python
@guarded
def cut_video(runtime, path, start, end, *, reencode=False, background=False):
    if background:
        return runtime.jobs.submit("cut_video", lambda: _do_cut(runtime, path, start, end, reencode=reencode))
    return _do_cut(runtime, path, start, end, reencode=reencode)
```

Tipo de retorno: `Result | JobSubmitted` na função pura, `Result | JobSubmitted | ErrorPayload` na `_tool`.

## Erros

- Falha esperada (arquivo não existe, argumento inválido, ffmpeg falhou): `raise ToolError(msg, code=..., hint=...)`.
- Códigos existem em `core/errors.py` (`ErrorCode`). Só crie um novo se nenhum servir.
- `hint` sempre diz o que o agente deve fazer a seguir. Ex: "Use probe_video para conferir a duração."
- Qualquer outra exceção é bug e deve subir. Nunca `except Exception` em tool.

## Caminhos

- Entrada do agente: `runtime.workspace.existing(path)` (precisa existir) ou `.resolve(path)` (pode não existir).
- Saída ao lado do original: `runtime.workspace.output_for(source, "tag", extension)`.
- Devolver ao agente: sempre `runtime.workspace.relative(path)`, nunca caminho absoluto.

## FFmpeg

- Só `core/ffmpeg.py` chama `subprocess`. Tool usa `runtime.ffmpeg.run([...])` e `runtime.ffmpeg.probe(path)`.
- Sem `shell=True`, sem montar string de comando.

## Domínio novo

1. Criar `domains/<nome>/__init__.py` com `register(mcp, runtime)`.
2. Adicionar o nome em `DomainName` no `config.py`.
3. Adicionar em `MCP_DOMAINS` no `.env.example` e no `env` do `.mcp.json`.
4. Criar `tests/domains/<nome>/__init__.py`.

## Testes

- Espelho: `domains/video/cut.py` → `tests/domains/video/test_cut.py`.
- Testar a função pura (`cut_video(runtime, ...)`), nunca a `_tool`.
- Fixtures em `tests/conftest.py`: `runtime`, `workspace`, `jobs`, `sample_video`.
- `unwrap` e `unwrap_job` em `tests/helpers.py` para estreitar o tipo de retorno.
- Teste que precisa de ffmpeg leva `pytestmark = pytest.mark.ffmpeg`.
- Sempre cobrir: caminho feliz, argumento inválido, arquivo inexistente.

## Qualidade

Antes de commitar, os três precisam passar:

```bash
uv run ruff check . && uv run ruff format --check .
uv run mypy
uv run pytest
```

Ruff está em `select = ["ALL"]` e mypy em `strict`. Não adicione `# noqa` nem `# type: ignore` sem código de erro e sem motivo.

## Convenções

- `from __future__ import annotations` em todo arquivo.
- Imports que só servem para tipo vão dentro de `if TYPE_CHECKING:`.
- Docstrings em português, estilo Google.
- Nomes de tool em `snake_case`, verbo primeiro: `cut_video`, `extract_audio`, `list_files`.
- Ao criar tool, atualizar a tabela de tools no `README.md`.
