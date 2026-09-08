---
name: nova-tool
description: Cria uma tool MCP nova na versão Rust deste projeto seguindo a arquitetura obrigatória (um arquivo por tool, função pura devolvendo ToolResult, struct Params com JsonSchema, register com description para o agente, teste espelhado). Use quando pedirem "cria uma tool", "adiciona uma tool de X", "nova ferramenta para o agente".
---

# Criar uma tool nova

Leia `.claude/rules/arquitetura.md` antes de começar. Siga estes passos na ordem.

1. **Definir**: nome em `snake_case` com verbo primeiro, domínio (`video`, `audio`, `files`, `jobs`, `media` ou novo), parâmetros e a struct de retorno.
2. **Escolher um modelo** e copiar a estrutura:
   - Tool simples: `domains/files/delete.rs`
   - Tool com `background`: `domains/video/cut.rs`
3. **Criar** `domains/<dominio>/<nome>.rs` com as quatro partes: struct `Result`, função pura `ToolResult<...>`, struct `Params` (`Deserialize + JsonSchema`) e `register`; declarar `pub mod` e chamar `register` no `mod.rs` do domínio.
4. **Escrever a description do `register` para o agente**: o que faz, quando usar, o que acontece com o original, o que devolve. Cada campo de `Params` leva um `///` de uma linha.
5. **Erros** com `ToolError::with_hint(msg, ErrorCode::X, hint)`. O `hint` diz o próximo passo do agente.
6. **Teste** em `tests/domains/<dominio>/test_<nome>.rs` (declarado no `mod.rs` de testes) testando a função pura: caminho feliz, argumento inválido, arquivo inexistente. Se usa ffmpeg, comece com `skip_without_ffmpeg!();`.
7. **README**: adicionar a linha na tabela de tools.
8. **Validar**:
   ```bash
   cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
   ```

Não termine com nenhum dos três falhando.
