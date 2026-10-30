#!/usr/bin/env bash
# Fixa packaging/nix/package.nix numa release já publicada: troca os `lib.fakeHash` de placeholder
# pelo hash do tarball da tag (`src.hash`) e pelo hash das crates (`cargoHash`).
#
#   packaging/nix/pin-release.sh v0.1.0     # depois de publicar a tag v0.1.0 no GitHub
#
# Roda num container nixos/nix: calcula `src.hash` (nix-prefetch-url --unpack) e deixa o próprio
# `nix build` do pacote revelar o `cargoHash` (build com hash falso, lê a linha `got:`). Atualiza
# `version`. Depois faça commit; a PR no nixpkgs é do humano (ver packaging/README.md, "nixpkgs").
set -euo pipefail

TAG="${1:-}"
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "uso: $0 vX.Y.Z" >&2; exit 2; }
VER="${TAG#v}"

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/../.." && pwd)"

if git -C "$ROOT" rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    cargo_ver="$(git -C "$ROOT" show "refs/tags/$TAG:Cargo.toml" | sed -n 's/^version = "\(.*\)"/\1/p' | head -1)"
    [ "$cargo_ver" = "$VER" ] || { echo "Cargo.toml da tag $TAG diz $cargo_ver, não $VER" >&2; exit 1; }
fi

trap 'docker rm -f "doknyu-nix-pin-$$" >/dev/null 2>&1 || true' EXIT
docker run --rm --name "doknyu-nix-pin-$$" -e "VER=$VER" \
    -e 'NIX_CONFIG=experimental-features = nix-command flakes
sandbox = false' \
    -v "$DIR:/nix-pkg" nixos/nix:latest sh -euc '
    cp /nix-pkg/package.nix /tmp/package.nix
    sed -i "s/^  version = \".*\";/  version = \"$VER\";/" /tmp/package.nix
    sed -i "s/hash = .*;/hash = lib.fakeHash;/; s/cargoHash = .*;/cargoHash = lib.fakeHash;/" /tmp/package.nix

    base32="$(nix-prefetch-url --unpack --type sha256 "https://github.com/RafaelProfMgz/doknyu/archive/refs/tags/v$VER.tar.gz" 2>/dev/null)"
    src_hash="$(nix hash convert --hash-algo sha256 --to sri "$base32")"
    sed -i "s|^    hash = .*;|    hash = \"$src_hash\";|" /tmp/package.nix

    # Build com cargoHash falso: o nix imprime o hash certo na linha `got:`.
    expr="(import <nixpkgs> {}).callPackage /tmp/package.nix {}"
    nix-build --no-out-link -E "$expr" 2>/tmp/err.txt || true
    cargo_hash="$(sed -n "s/^ *got: *//p" /tmp/err.txt | head -1)"
    [ -n "$cargo_hash" ] || { cat /tmp/err.txt >&2; echo "nao consegui obter o cargoHash" >&2; exit 1; }
    sed -i "s|^  cargoHash = .*;|  cargoHash = \"$cargo_hash\";|" /tmp/package.nix

    nix-build --no-out-link -E "$expr"   # confirma que constroi de verdade
    cat /tmp/package.nix > /nix-pkg/package.nix
'
echo "package.nix fixado em $TAG. Revise o diff e faça commit."
