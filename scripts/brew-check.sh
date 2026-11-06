#!/usr/bin/env bash
# Valida packaging/homebrew/doknyu.rb num container homebrew/brew (Homebrew no Linux).
#
#   scripts/brew-check.sh
#
# Cria um tap local descartável (`doknyu-local/check`, sem git, só dentro do container), copia a
# fórmula para ele e roda: `brew style`, `brew audit --strict --new`,
# `brew install --build-from-source` (compila do tarball da tag que a fórmula fixa, então precisa
# de rede e leva vários minutos), `doknyu --version`, `brew test` e `brew uninstall`, e confere
# que o `--version` bate com o do Cargo.toml. Container `doknyu-spike-brew-check-*`, removido ao fim.
# Não publica nada e não usa sudo.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FORMULA="$ROOT/packaging/homebrew/doknyu.rb"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
NAME="doknyu-spike-brew-check-$$"
trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT

# Se a fórmula ainda aponta para outra versão que a do Cargo.toml, o teste de versão falharia.
grep -q "/tags/v$VERSION.tar.gz" "$FORMULA" \
    || { echo "FALHA: a url de $FORMULA não é a v$VERSION (rode packaging/homebrew/pin-release.sh v$VERSION)" >&2; exit 1; }

docker run --rm -i --name "$NAME" -e "VERSION=$VERSION" -e HOMEBREW_NO_AUTO_UPDATE=1 \
    -e HOMEBREW_NO_ANALYTICS=1 -e HOMEBREW_NO_INSTALL_FROM_API=1 \
    -v "$FORMULA:/in/doknyu.rb:ro" homebrew/brew:latest bash -euo pipefail -s <<'INNER'
brew --version
brew tap-new --no-git doknyu-local/check >/dev/null
tap="$(brew --repository doknyu-local/check)"
cp /in/doknyu.rb "$tap/Formula/doknyu.rb"
f=doknyu-local/check/doknyu

echo "--- brew style"
brew style "$f"

echo "--- brew audit --strict --new"
brew audit --strict --new "$f"

echo "--- brew install --build-from-source"
brew install --build-from-source --verbose "$f"

echo "--- doknyu --version"
got="$(doknyu --version)"; echo "doknyu --version => $got"
[ "$got" = "doknyu $VERSION" ] || { echo "FALHA: --version"; exit 1; }

echo "--- brew test"
brew test "$f"

echo "--- arquivos instalados"
(cd "$(brew --prefix "$f")" && find bin share -type f | sort)
echo "--- bibliotecas linkadas / rpath"
readelf -d "$(brew --prefix "$f")/bin/doknyu" | grep -E 'NEEDED|RUNPATH|RPATH'

echo "--- brew uninstall"
# Sem autoremove: ele listaria as ~100 dependências de build que sobraram no container.
HOMEBREW_NO_AUTOREMOVE=1 brew uninstall "$f"
command -v doknyu >/dev/null && { echo "FALHA: doknyu ainda no PATH"; exit 1; }
echo "BREW-CHECK: OK"
INNER
