#!/usr/bin/env bash
# Fixa o manifesto do Flathub numa tag: reescreve o bloco de fonte (entre BEGIN-SOURCE e
# END-SOURCE) para `type: git` + `tag` + `commit` e confere que o cargo-sources.json bate com o
# Cargo.lock daquela tag.
#
#   packaging/flatpak/pin-release.sh v0.1.0     # a tag precisa existir localmente
#
# Depois, faça commit do manifesto (ver packaging/README.md, "Release"). Não dá push em nada.
set -euo pipefail

TAG="${1:-}"
[ -n "$TAG" ] || { echo "uso: $0 <tag>" >&2; exit 2; }

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/../.." && pwd)"
MANIFEST="$DIR/io.github.rafaelprofmgz.Doknyu.yml"
URL="https://github.com/RafaelProfMgz/doknyu.git"

COMMIT="$(git -C "$ROOT" rev-list -n1 "refs/tags/$TAG" 2>/dev/null)" \
    || { echo "a tag '$TAG' não existe localmente (git tag $TAG <commit>)" >&2; exit 1; }

# Confere o cargo-sources.json do commit da tag (o que o Flathub vai construir), num worktree
# temporário. O cache do gerador (.tools) é o do checkout atual, para não baixar de novo.
WT="$(mktemp -d)"
cleanup() { git -C "$ROOT" worktree remove --force "$WT" >/dev/null 2>&1 || rm -rf "$WT"; }
trap cleanup EXIT
git -C "$ROOT" worktree add --detach "$WT" "$COMMIT" >/dev/null 2>&1
TOOLS_DIR="${TOOLS_DIR:-$DIR/.tools}" "$WT/scripts/update-cargo-sources.sh" --check \
    || { echo "cargo-sources.json da tag $TAG não bate com o Cargo.lock dela; corrija e refaça a tag." >&2; exit 1; }

tmp="$(mktemp)"
awk -v url="$URL" -v tag="$TAG" -v commit="$COMMIT" '
    /# BEGIN-SOURCE/ {
        print
        print "      # Gerado por packaging/flatpak/pin-release.sh; não edite à mão."
        print "      - type: git"
        print "        url: " url
        print "        tag: " tag
        print "        commit: " commit
        skipping = 1; next
    }
    /# END-SOURCE/ { skipping = 0 }
    !skipping { print }
' "$MANIFEST" > "$tmp"
grep -q "commit: $COMMIT" "$tmp" || { echo "falha ao reescrever o manifesto" >&2; rm -f "$tmp"; exit 1; }
mv "$tmp" "$MANIFEST"
echo "Manifesto fixado em $TAG ($COMMIT)."
