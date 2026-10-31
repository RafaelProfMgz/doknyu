#!/usr/bin/env bash
# Gera o Source1 do .spec: as crates do Cargo.lock vendorizadas (build offline no COPR/Koji).
#
#   make-vendor.sh <diretório-do-código-fonte> <arquivo-de-saída.tar.xz>
#
# Roda DENTRO de um Fedora (precisa de cargo e cargo-vendor-filterer, com rede); quem chama são
# scripts/rpm-check.sh e packaging/rpm/pin-release.sh. O vendor-filterer descarta as crates que só
# existem para Windows/macOS/etc., então o tarball só tem o que o Linux compila.
set -euo pipefail

SRC="${1:?uso: $0 <código-fonte> <saída.tar.xz>}"
OUT="${2:?uso: $0 <código-fonte> <saída.tar.xz>}"

cd "$SRC"
rm -rf vendor
# vendor-filterer não tem --locked: garante que o Cargo.lock está em dia e que continua igual depois.
cargo metadata --locked --format-version 1 >/dev/null
cp Cargo.lock "$OUT.lock"
cargo vendor-filterer --keep-dep-kinds=all \
    --platform=x86_64-unknown-linux-gnu --platform=aarch64-unknown-linux-gnu \
    --tier=2 vendor >/dev/null
cmp Cargo.lock "$OUT.lock" || { echo "o vendor alterou o Cargo.lock" >&2; exit 1; }
rm -f "$OUT.lock"
# Reprodutível: ordem e datas fixas.
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime=@0 -cf - vendor | xz -9 -T0 > "$OUT"
rm -rf vendor
echo "vendor: $OUT ($(du -h "$OUT" | cut -f1))"
