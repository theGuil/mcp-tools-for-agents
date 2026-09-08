#!/usr/bin/env sh
# Sobe o servidor MCP garantindo que o binário exista antes.
#
# É o `command` do `.mcp.json`. O Claude Code abre os servidores stdio ao mesmo
# tempo em que roda o hook SessionStart, então em uma máquina nova o
# `bin/mcp-tools` pode ainda não existir quando a conexão é tentada. Este
# wrapper espera o hook terminar o download e, se ele não aparecer, baixa
# sozinho. Toda saída vai para stderr: stdout é o canal do protocolo MCP.
#
# Variáveis:
#   MCP_TOOLS_WAIT  segundos de espera pelo hook antes de baixar. Padrão: 20.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
binary="${MCP_TOOLS_DIR:-$root/bin}/mcp-tools"
stamp="${MCP_TOOLS_DIR:-$root/bin}/.mcp-tools-version"
wait="${MCP_TOOLS_WAIT:-20}"

# O hook grava o binário e só depois a marca de versão: esperar a marca evita
# executar um arquivo ainda sendo copiado.
i=0
while [ ! -x "$binary" ] || [ ! -f "$stamp" ]; do
  if [ "$i" -ge "$wait" ]; then
    echo "mcp-tools-run: binário ausente após ${wait}s, baixando agora." >&2
    sh "$root/scripts/install-mcp-tools.sh" >&2
    break
  fi
  i=$((i + 1))
  sleep 1
done

if [ ! -x "$binary" ]; then
  echo "mcp-tools-run: $binary não existe. Rode: sh scripts/install-mcp-tools.sh" >&2
  exit 1
fi

exec "$binary" "$@"
