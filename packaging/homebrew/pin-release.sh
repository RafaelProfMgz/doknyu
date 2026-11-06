#!/usr/bin/env bash
# Fixa packaging/homebrew/doknyu.rb numa release já publicada: troca `url` e `sha256` pelos do
# tarball da tag.
#
#   packaging/homebrew/pin-release.sh v0.1.0     # depois de publicar a tag v0.1.0 no GitHub
#
# Baixa o tarball da tag (https://github.com/RafaelProfMgz/doknyu/archive/refs/tags/<tag>.tar.gz),
# calcula o sha256 e reescreve a fórmula. Não precisa de Docker. Depois confira com
# `scripts/brew-check.sh`, faça commit e copie a fórmula para o tap (ver packaging/README.md,
# "Homebrew"). Não publica nada.
set -euo pipefail

TAG="${1:-}"
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "uso: $0 vX.Y.Z" >&2; exit 2; }
VER="${TAG#v}"

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/../.." && pwd)"
FORMULA="$DIR/doknyu.rb"

# Se a tag existe localmente, a versão do Cargo.toml dela tem de ser a do pacote.
if git -C "$ROOT" rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    cargo_ver="$(git -C "$ROOT" show "refs/tags/$TAG:Cargo.toml" | sed -n 's/^version = "\(.*\)"/\1/p' | head -1)"
    [ "$cargo_ver" = "$VER" ] || { echo "Cargo.toml da tag $TAG diz $cargo_ver, não $VER" >&2; exit 1; }
fi

URL="https://github.com/RafaelProfMgz/doknyu/archive/refs/tags/$TAG.tar.gz"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
curl -fsSL -o "$WORK/src.tar.gz" "$URL" || { echo "não consegui baixar $URL (a tag existe no GitHub?)" >&2; exit 1; }
SHA="$(sha256sum "$WORK/src.tar.gz" | cut -d' ' -f1)"

sed -i "s|^  url \".*\"|  url \"$URL\"|; s|^  sha256 \".*\"|  sha256 \"$SHA\"|" "$FORMULA"
{ grep -q "^  url \"$URL\"" "$FORMULA" && grep -q "^  sha256 \"$SHA\"" "$FORMULA"; } \
    || { echo "FALHA: não consegui reescrever url/sha256 em $FORMULA" >&2; exit 1; }

echo "doknyu.rb fixado em $TAG (sha256 $SHA). Revise o diff, rode scripts/brew-check.sh e faça commit."
