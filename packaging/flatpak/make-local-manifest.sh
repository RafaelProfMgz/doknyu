#!/usr/bin/env bash
# Gera packaging/flatpak/io.github.rafaelprofmgz.Doknyu.local.yml: o manifesto do Flathub com a
# fonte `git` trocada por `type: dir` (a árvore atual do repositório), para testar a branch.
# O arquivo gerado é ignorado pelo git. Usado no build local e no CI do PR.
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ID="io.github.rafaelprofmgz.Doknyu"

awk '
    /# BEGIN-SOURCE/ {
        print "      - type: dir"
        print "        path: ../.."
        print "        skip:"
        print "          - target"
        print "          - .git"
        print "          - docs"
        print "          - packaging/flatpak/.tools"
        print "          - packaging/flatpak/.flatpak-builder"
        print "          - packaging/flatpak/build-dir"
        print "          - packaging/flatpak/repo"
        skipping = 1; next
    }
    /# END-SOURCE/ { skipping = 0; next }
    !skipping { print }
' "$DIR/$ID.yml" > "$DIR/$ID.local.yml"
echo "Gerado: $DIR/$ID.local.yml"
