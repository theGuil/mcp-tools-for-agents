#!/usr/bin/env sh
# Baixa o binário `mcp-tools` da última release do GitHub para `bin/mcp-tools`.
#
# É o que o Claude Code roda no início de cada sessão (hook SessionStart em
# `.claude/settings.json`) para ter o servidor MCP pronto sem compilar nada.
# Também pode ser rodado à mão: `sh scripts/install-mcp-tools.sh`.
#
# Variáveis:
#   MCP_TOOLS_VERSION  tag da release (ex.: v0.1.0). Padrão: latest.
#   MCP_TOOLS_REPO     repositório owner/nome. Padrão: theGuil/mcp-tools-for-agents.
#   MCP_TOOLS_DIR      pasta de destino. Padrão: <raiz do repositório>/bin.
#   MCP_TOOLS_FORCE    "true" baixa de novo mesmo que a versão instalada seja a mesma.
set -eu

repo="${MCP_TOOLS_REPO:-theGuil/mcp-tools-for-agents}"
version="${MCP_TOOLS_VERSION:-latest}"
root="$(cd "$(dirname "$0")/.." && pwd)"
dest="${MCP_TOOLS_DIR:-$root/bin}"
binary="$dest/mcp-tools"
stamp="$dest/.mcp-tools-version"

os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Linux) platform="unknown-linux-gnu" ;;
  Darwin) platform="apple-darwin" ;;
  *) echo "install-mcp-tools: sistema '$os' não suportado por este script (use a release do Windows à mão)." >&2; exit 1 ;;
esac
case "$arch" in
  x86_64 | amd64) cpu="x86_64" ;;
  aarch64 | arm64) cpu="aarch64" ;;
  *) echo "install-mcp-tools: arquitetura '$arch' não suportada." >&2; exit 1 ;;
esac
target="$cpu-$platform"
asset="mcp-tools-$target.tar.gz"

if [ "$version" = "latest" ]; then
  url="https://github.com/$repo/releases/latest/download/$asset"
else
  url="https://github.com/$repo/releases/download/$version/$asset"
fi

# Descobre a tag real de "latest" para não baixar de novo a cada sessão.
resolved="$version"
if [ "$version" = "latest" ]; then
  resolved="$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest" 2>/dev/null | sed 's#.*/tag/##')" || resolved="latest"
  [ -n "$resolved" ] || resolved="latest"
fi

if [ -x "$binary" ] && [ "${MCP_TOOLS_FORCE:-false}" != "true" ] && [ -f "$stamp" ] \
   && [ "$(cat "$stamp")" = "$resolved" ] && [ "$resolved" != "latest" ]; then
  echo "install-mcp-tools: $resolved já instalado em $binary"
  exit 0
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "install-mcp-tools: baixando $url"
if ! curl -fsSL --retry 3 --retry-delay 2 -o "$tmp/$asset" "$url"; then
  echo "install-mcp-tools: download falhou. Confira se existe uma release com o anexo $asset em https://github.com/$repo/releases" >&2
  if [ -x "$binary" ]; then
    echo "install-mcp-tools: mantendo o binário já existente em $binary" >&2
    exit 0
  fi
  exit 1
fi

mkdir -p "$dest"
tar -xzf "$tmp/$asset" -C "$tmp"
mv "$tmp/mcp-tools" "$binary"
chmod +x "$binary"
printf '%s\n' "$resolved" > "$stamp"
echo "install-mcp-tools: $resolved instalado em $binary"
