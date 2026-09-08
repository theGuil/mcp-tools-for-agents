# Release e binário usado pelo Claude Code

O Claude Code usa o binário `bin/mcp-tools`, baixado da **última release** do
repositório no GitHub. Nunca aponte o `.mcp.json` para `target/`.

## Fluxo

| Passo | Quem faz | Onde |
|---|---|---|
| Compilar e publicar a release | GitHub Actions, ao receber uma tag `v*` | `.github/workflows/release.yml` |
| Baixar a última release | hook `SessionStart` do Claude Code | `.claude/settings.json` → `scripts/install-mcp-tools.sh` |
| Subir o servidor | Claude Code, via stdio | `.mcp.json` → `./bin/mcp-tools` |

## Publicar uma versão nova

1. Garanta `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` e `cargo test` verdes.
2. Atualize `version` em `Cargo.toml`.
3. Crie e envie a tag com o mesmo número: `git tag v0.2.0 && git push origin v0.2.0`.
4. Acompanhe o workflow `release` no GitHub. Ao terminar, a release aparece em
   `https://github.com/theGuil/mcp-tools-for-agents/releases` com um
   `mcp-tools-<target>.tar.gz` por plataforma (e `.zip` no Windows).
5. Na próxima sessão do Claude Code o hook baixa a versão nova sozinho. Para
   atualizar agora: `sh scripts/install-mcp-tools.sh`.

## Script `scripts/install-mcp-tools.sh`

- Detecta sistema e CPU, monta o nome do asset e baixa de
  `releases/latest/download/<asset>` (ou de uma tag fixa com `MCP_TOOLS_VERSION`).
- Guarda a tag instalada em `bin/.mcp-tools-version` e pula o download quando já
  está na última.
- Se o download falhar e já existir um binário, mantém o existente e sai com 0
  para não travar a sessão.
- Precisa de `curl` e `tar`, presentes em qualquer Linux e macOS. Precisa de rede
  para `github.com`.

## O que o agente deve fazer

- Servidor não conecta no `/mcp`: rode `sh scripts/install-mcp-tools.sh` e leia a
  saída. O erro mais comum é não existir release ainda, ou a rede bloquear o GitHub.
- Tool nova criada: ela só existe para o Claude Code depois de uma release nova.
  Termine o trabalho lembrando o usuário de subir a tag.
- Nunca commite `bin/` nem o binário.
