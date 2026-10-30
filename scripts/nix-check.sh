#!/usr/bin/env bash
# Valida flake.nix e packaging/nix/package.nix num container nixos/nix (flakes habilitados).
#
#   scripts/nix-check.sh              # build completo
#   scripts/nix-check.sh --no-build   # só `nix flake check --no-build` e `nix eval` (rápido)
#
# O flake só enxerga arquivos rastreados pelo git, então a árvore de trabalho (inclusive o que ainda
# não foi commitado) vai para um repositório descartável em /stage. O `src` e as crates vêm dessa
# árvore (override do flake); o `fetchFromGitHub` do package.nix só é usado depois da tag
# (packaging/nix/pin-release.sh). Faz `nix build .#doknyu`, confere `result/bin/doknyu --version`,
# o rpath das bibliotecas dlopen e os arquivos instalados. Se não existir flake.lock, o gerado é
# copiado para a raiz (commite-o). Não publica nada e não usa sudo.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="${1:-build}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"

STAGE="$(mktemp -d)"
NAME="doknyu-spike-nix-check-$$"
trap 'rm -rf "$STAGE"; docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT

mkdir -p "$STAGE/src" "$STAGE/out"
git -C "$ROOT" ls-files -z --cached --others --exclude-standard \
    | tar -C "$ROOT" --null -T - --ignore-failed-read -cf - | tar -C "$STAGE/src" -xf -

docker run --rm -i --name "$NAME" -e "MODE=$MODE" -e "VERSION=$VERSION" \
    -e 'NIX_CONFIG=experimental-features = nix-command flakes
sandbox = false
accept-flake-config = true' \
    -v "$STAGE/src:/in:ro" -v "$STAGE/out:/out" \
    nixos/nix:latest sh -s <<'INNER'
set -eu
cp -r /in /stage && cd /stage
git config --global safe.directory '*'
git config --global user.name nix-check; git config --global user.email nix-check@localhost
git init -q . && git add -A && git commit -qm snapshot
sys="$(nix eval --impure --raw --expr builtins.currentSystem)"

echo "--- nix flake check --no-build"
nix flake check --no-build
echo "--- nix eval"
nix eval ".#packages.$sys.doknyu.name"
nix eval ".#devShells.$sys.default.name"
cp flake.lock /out/flake.lock

[ "$MODE" = "--no-build" ] && { echo "NIX-CHECK (sem build): OK"; exit 0; }

echo "--- nix build .#doknyu"
nix build .#doknyu -L --no-link --print-out-paths > /tmp/out.txt
out="$(cat /tmp/out.txt)"; echo "out: $out"
got="$("$out/bin/doknyu" --version)"; echo "doknyu --version => $got"
[ "$got" = "doknyu $VERSION" ] || { echo "FALHA: --version"; exit 1; }

echo "--- rpath (dlopen)"
nix shell nixpkgs#patchelf -c patchelf --print-rpath "$out/bin/doknyu" | tr ':' '\n' | grep -E 'wayland|xkbcommon|vulkan|libGL|libx11|libxcb|libxcursor|libxi' || { echo "FALHA: rpath sem as libs dlopen"; exit 1; }

echo "--- arquivos instalados"
(cd "$out" && find share -type f | sort)
for f in share/applications/io.github.rafaelprofmgz.Doknyu.desktop share/metainfo/io.github.rafaelprofmgz.Doknyu.metainfo.xml \
    share/icons/hicolor/scalable/apps/io.github.rafaelprofmgz.Doknyu.svg share/icons/hicolor/512x512/apps/io.github.rafaelprofmgz.Doknyu.png \
    share/licenses/doknyu/LICENSE-MIT share/licenses/doknyu/LICENSE-APACHE share/licenses/doknyu/LICENSES-fonts.md; do
    [ -f "$out/$f" ] || { echo "FALHA: falta $f"; exit 1; }
done
echo "NIX-CHECK: OK"
INNER

# flake.lock gerado: só entra na raiz se ainda não existir (para atualizar, apague-o e rode de novo).
[ -f "$ROOT/flake.lock" ] || { cp "$STAGE/out/flake.lock" "$ROOT/flake.lock"; echo "flake.lock criado na raiz"; }
