#!/usr/bin/env bash
# Valida packaging/rpm/doknyu.spec num container fedora:latest, a partir da árvore local.
#
#   scripts/rpm-check.sh
#
# O Source0 (tarball da tag) ainda não existe, então o teste gera o `doknyu-<versão>.tar.gz` do
# repositório local (árvore de trabalho, inclusive o que não foi commitado) e o Source1
# (`doknyu-<versão>-vendor.tar.xz`) com packaging/rpm/make-vendor.sh. Depois: `rpmbuild -ba`
# (build offline com as crates vendorizadas), `rpmlint` no .spec, no SRPM e no RPM, `dnf install` do
# RPM, `doknyu --version`, `dnf remove`. Falha se o build, o `--version` ou o `rpmlint` (erros `E:`)
# falharem; avisos `W:` são impressos e conferidos à mão. Não publica nada; sem sudo no host.
# Com a tag publicada, o SRPM de verdade sai de packaging/rpm/pin-release.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
NAME="doknyu-spike-rpm-check-$$"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"; docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT

# Tarball no layout do GitHub (`doknyu-<versão>/`), só com arquivos do git (rastreados + novos).
mkdir -p "$STAGE/tree/doknyu-$VERSION"
git -C "$ROOT" ls-files -z --cached --others --exclude-standard \
    | tar -C "$ROOT" --null -T - --ignore-failed-read -cf - | tar -C "$STAGE/tree/doknyu-$VERSION" -xf -
tar -C "$STAGE/tree" -czf "$STAGE/doknyu-$VERSION.tar.gz" "doknyu-$VERSION"
rm -rf "$STAGE/tree"

docker run --rm -i --name "$NAME" -e "VERSION=$VERSION" \
    -v "$ROOT/packaging/rpm:/rpm:ro" -v "$STAGE:/stage:ro" \
    fedora:latest bash -s <<'INNER'
set -uo pipefail
fail=0
dnf -y install rpm-build rpmlint cargo rust cargo-rpm-macros cargo-vendor-filterer \
    desktop-file-utils appstream gcc tar xz >/dev/null 2>&1 || { echo "FALHA: dnf install das ferramentas"; exit 1; }
useradd -m builder
runuser -u builder -- bash -s <<'BUILDER'
set -euo pipefail
mkdir -p ~/rpmbuild/{SOURCES,SPECS}
cp /rpm/doknyu.spec ~/rpmbuild/SPECS/
cp "/stage/doknyu-$VERSION.tar.gz" ~/rpmbuild/SOURCES/
mkdir ~/src && tar -C ~/src -xzf "/stage/doknyu-$VERSION.tar.gz"
echo "--- make-vendor.sh"
/rpm/make-vendor.sh ~/src/doknyu-$VERSION ~/rpmbuild/SOURCES/doknyu-$VERSION-vendor.tar.xz
BUILDER
[ $? -eq 0 ] || { echo "FALHA: vendor"; exit 1; }

echo "--- rpmlint doknyu.spec"
runuser -u builder -- rpmlint /home/builder/rpmbuild/SPECS/doknyu.spec 2>&1 | tee /tmp/lint-spec.txt
grep -q ' E: ' /tmp/lint-spec.txt && { echo "FALHA: rpmlint .spec"; fail=1; }

echo "--- rpmbuild -ba (build offline)"
if ! runuser -u builder -- rpmbuild -ba /home/builder/rpmbuild/SPECS/doknyu.spec > /tmp/build.log 2>&1; then
    grep -v '^   /usr/share/cargo/registry' /tmp/build.log | tail -60; echo "FALHA: rpmbuild"; exit 1
fi
grep -E '^(Wrote:|test result|License|# ?[A-Za-z]|Provides|Requires)' /tmp/build.log | head -40
grep -n 'Downloading\|Updating crates.io' /tmp/build.log | head -3 && { echo "FALHA: o build tocou a rede"; fail=1; }

rpm=$(ls /home/builder/rpmbuild/RPMS/*/doknyu-$VERSION-*.rpm | head -1)
srpm=$(ls /home/builder/rpmbuild/SRPMS/doknyu-*.src.rpm | head -1)
echo "--- rpmlint srpm e rpm"
runuser -u builder -- rpmlint "$srpm" "$rpm" 2>&1 | tee /tmp/lint-rpm.txt
grep -q ' E: ' /tmp/lint-rpm.txt && { echo "FALHA: rpmlint dos pacotes"; fail=1; }
echo "--- conteúdo"; rpm -qlp "$rpm" | grep -v -E 'hicolor/[0-9x]+/apps'
echo "--- requires"; rpm -qp --requires "$rpm" | grep -v '^rpmlib\|^/' | sort -u

echo "--- dnf install, roda, remove"
dnf -y install "$rpm" >/dev/null 2>&1 || { echo "FALHA: dnf install"; exit 1; }
out="$(doknyu --version 2>&1)"; echo "doknyu --version => $out"
[ "$out" = "doknyu $VERSION" ] || { echo "FALHA: --version"; fail=1; }
dnf -y remove doknyu >/dev/null 2>&1 || { echo "FALHA: dnf remove"; fail=1; }
[ ! -e /usr/bin/doknyu ] || { echo "FALHA: sobrou /usr/bin/doknyu"; fail=1; }

echo; [ "$fail" -eq 0 ] && echo "RPM-CHECK: OK" || echo "RPM-CHECK: FALHOU"
exit "$fail"
INNER
