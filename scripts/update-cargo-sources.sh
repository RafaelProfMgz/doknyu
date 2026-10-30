#!/usr/bin/env bash
# Regenera packaging/flatpak/cargo-sources.json a partir do Cargo.lock.
#
#   scripts/update-cargo-sources.sh          # reescreve o arquivo
#   scripts/update-cargo-sources.sh --check  # só confere; sai com 1 se estiver desatualizado (CI)
#
# O gerador (flatpak-builder-tools) fica numa versão fixa: commit + SHA-256 do script. Ele roda
# com `python -I` num venv próprio (só `aiohttp` e `tomlkit`), em .tools/ (ignorado pelo git).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/packaging/flatpak/cargo-sources.json"
TOOLS="${TOOLS_DIR:-$ROOT/packaging/flatpak/.tools}"

GEN_COMMIT="41c20aa10819cdb2a4f3ca171758a96d1955c018"
GEN_SHA256="0a2db6be87d75910facef28ab46d4d6460802e8419ab850d0caa6a364d26b380"
GEN_URL="https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/$GEN_COMMIT/cargo/flatpak-cargo-generator.py"
GEN="$TOOLS/flatpak-cargo-generator-$GEN_COMMIT.py"

mode=write
case "${1:-}" in
    "") ;;
    --check) mode=check ;;
    *) echo "uso: $0 [--check]" >&2; exit 2 ;;
esac

mkdir -p "$TOOLS"
if [ ! -f "$GEN" ]; then
    curl --proto "=https" --tlsv1.2 -fsSL "$GEN_URL" -o "$GEN.part"
    mv "$GEN.part" "$GEN"
fi
echo "$GEN_SHA256  $GEN" | sha256sum --check --quiet - \
    || { echo "SHA-256 do gerador não confere; apague $GEN" >&2; exit 1; }

if [ ! -x "$TOOLS/venv/bin/python" ]; then
    python3 -m venv "$TOOLS/venv"
    "$TOOLS/venv/bin/pip" install --quiet 'aiohttp==3.14.5' 'tomlkit==0.15.2'
fi

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
"$TOOLS/venv/bin/python" -I "$GEN" "$ROOT/Cargo.lock" -o "$tmp"

if [ "$mode" = check ]; then
    if cmp -s "$tmp" "$OUT"; then
        echo "cargo-sources.json está em dia com o Cargo.lock."
    else
        echo "cargo-sources.json está desatualizado: rode scripts/update-cargo-sources.sh e faça commit." >&2
        exit 1
    fi
else
    mv "$tmp" "$OUT"
    trap - EXIT
    echo "Atualizado: $OUT"
fi
