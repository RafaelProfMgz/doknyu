#!/usr/bin/env bash
# Prepara uma versão nova ANTES da tag: muda a versão em todos os lugares que a guardam.
#
#   scripts/bump-version.sh 0.2.0            # numa branch a partir da develop
#   scripts/bump-version.sh 0.2.0 2026-11-20 # data da release no metainfo/spec (padrão: hoje)
#
# Atualiza:
#   - Cargo.toml (`version`) e Cargo.lock (pacote doknyu)
#   - metainfo: novo <release> no topo de <releases> e a tag das URLs dos screenshots
#   - packaging/rpm/doknyu.spec: `Version:`, `Release: 1` e uma entrada nova no %changelog
#
# Os pins que dependem da tag publicada (Flatpak, AUR, Nix, Homebrew) ficam para depois da tag:
# o workflow `pin-release.yml` faz isso e abre o PR para a develop (ver CONTRIBUTING.md, "Release").
# Não faz commit, não cria tag, não dá push.
set -euo pipefail

NEW="${1:-}"
DATE="${2:-$(date -u +%Y-%m-%d)}"
[[ "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "uso: $0 X.Y.Z [AAAA-MM-DD]" >&2; exit 2; }
[[ "$DATE" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || { echo "data inválida: $DATE (use AAAA-MM-DD)" >&2; exit 2; }

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO="$ROOT/Cargo.toml"
LOCK="$ROOT/Cargo.lock"
META="$ROOT/assets/io.github.rafaelprofmgz.Doknyu.metainfo.xml"
SPEC="$ROOT/packaging/rpm/doknyu.spec"

OLD="$(sed -n 's/^version = "\(.*\)"/\1/p' "$CARGO" | head -1)"
[ -n "$OLD" ] || { echo "não achei a versão em Cargo.toml" >&2; exit 1; }

if [ "$OLD" = "$NEW" ]; then
    echo "Cargo.toml já está em $NEW; só confiro o resto."
else
    # A versão nova tem de ser maior que a atual (comparação numérica X.Y.Z).
    if [ "$(printf '%s\n%s\n' "$OLD" "$NEW" | sort -V | tail -1)" != "$NEW" ]; then
        echo "a versão nova ($NEW) não é maior que a atual ($OLD)" >&2
        exit 1
    fi
fi

# Cargo.toml: só a primeira linha `version =` (a do [package]).
sed -i "0,/^version = \".*\"/s//version = \"$NEW\"/" "$CARGO"

# Cargo.lock: a linha `version` logo depois de `name = "doknyu"`.
awk -v v="$NEW" '
    found && /^version = / { print "version = \"" v "\""; found = 0; next }
    { found = ($0 == "name = \"doknyu\"") ? 1 : found; print }
' "$LOCK" > "$LOCK.tmp" && mv "$LOCK.tmp" "$LOCK"

# Metainfo: <release> novo no topo (se ainda não existir) e screenshots apontando para a tag nova.
if ! grep -q "<release version=\"$NEW\"" "$META"; then
    sed -i "s|^\(\s*\)<releases>|&\n\1  <release version=\"$NEW\" date=\"$DATE\"/>|" "$META"
fi
sed -i "s|/RafaelProfMgz/doknyu/v[0-9.]*/docs/screenshots/|/RafaelProfMgz/doknyu/v$NEW/docs/screenshots/|g" "$META"

# Spec do COPR.
sed -i "s/^\(Version:\s*\).*/\1$NEW/; s/^\(Release:\s*\).*/\11%{?dist}/" "$SPEC"
if ! grep -q -- "- $NEW-1\$" "$SPEC"; then
    day="$(LC_ALL=C date -u -d "$DATE" '+%a %b %d %Y')"
    entry="* $day RafaelProfMgz <RafaelProfMgz@users.noreply.github.com> - $NEW-1\n- Update to $NEW"
    sed -i "s|^%changelog\$|%changelog\n$entry\n|" "$SPEC"
fi

# Conferências: tudo tem de estar na versão nova.
grep -q "^version = \"$NEW\"" "$CARGO" || { echo "falhou: Cargo.toml" >&2; exit 1; }
grep -A1 '^name = "doknyu"$' "$LOCK" | grep -q "^version = \"$NEW\"" || { echo "falhou: Cargo.lock" >&2; exit 1; }
grep -q "<release version=\"$NEW\" date=" "$META" || { echo "falhou: metainfo" >&2; exit 1; }
grep -q "^Version:\s*$NEW$" "$SPEC" || { echo "falhou: doknyu.spec" >&2; exit 1; }
if command -v appstreamcli >/dev/null; then
    appstreamcli validate --no-net "$META" >/dev/null || { echo "metainfo inválido depois do bump" >&2; exit 1; }
fi

echo "Versão $OLD → $NEW ($DATE). Confira com 'git diff' e faça commit:"
echo "  git commit -am \"chore(release): bump version to $NEW\""
