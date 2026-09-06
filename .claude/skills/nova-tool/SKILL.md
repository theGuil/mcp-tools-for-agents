---
name: nova-tool
description: Cria uma tool MCP nova neste projeto seguindo a arquitetura obrigatória (um arquivo por tool, função pura com @guarded, register com docstring para o agente, teste espelhado). Use quando pedirem "cria uma tool", "adiciona uma tool de X", "nova ferramenta para o agente".
---

# Criar uma tool nova

Leia `.claude/rules/arquitetura.md` antes de começar. Siga estes passos na ordem.

1. **Definir**: nome em `snake_case` com verbo primeiro, domínio (`video`, `audio`, `files`, `jobs` ou novo), parâmetros e o TypedDict de retorno.
2. **Escolher um modelo** e copiar a estrutura:
   - Tool simples: `domains/files/delete.py`
   - Tool com `background`: `domains/video/cut.py`
3. **Criar** `domains/<dominio>/<nome>.py` com as quatro partes: `Result` TypedDict, função pura `@guarded`, `register`, e chamada no `__init__.py` do domínio.
4. **Escrever a docstring da `_tool` para o agente**: o que faz, quando usar, o que acontece com o original, o que devolve. `Args:` com uma linha por parâmetro.
5. **Erros** com `ToolError(msg, code=..., hint=...)`. O `hint` diz o próximo passo do agente.
6. **Teste** em `tests/domains/<dominio>/test_<nome>.py` testando a função pura: caminho feliz, argumento inválido, arquivo inexistente. Se usa ffmpeg, `pytestmark = pytest.mark.ffmpeg`.
7. **README**: adicionar a linha na tabela de tools.
8. **Validar**:
   ```bash
   uv run ruff check . && uv run ruff format --check . && uv run mypy && uv run pytest
   ```

Não termine com nenhum dos três falhando.
