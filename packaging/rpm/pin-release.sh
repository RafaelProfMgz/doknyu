#!/usr/bin/env bash
# Gera o SRPM do COPR a partir de uma release já publicada.
#
#   packaging/rpm/pin-release.sh v0.1.0     # depois de publicar a tag v0.1.0 no GitHub
#
# Baixa o tarball da tag (o Source0 do .spec), gera o Source1 (as crates vendorizadas, com
# make-vendor.sh) e roda `rpmbuild -bs` num container fedora:latest. O resultado fica em
# packaging/rpm/out/ (ignorado pelo git): doknyu-<versão>-1.<dist>.src.rpm, que é o que o
# `copr-cli build` recebe (ver packaging/README.md, "COPR"). Confere que `Version:` do .spec é a da
# tag. Não publica nada.
set -euo pipefail

TAG="${1:-}"
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "uso: $0 vX.Y.Z" >&2; exit 2; }
VER="${TAG#v}"

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/../.." && pwd)"

spec_ver="$(sed -n 's/^Version:[[:space:]]*//p' "$DIR/doknyu.spec" | head -1)"
[ "$spec_ver" = "$VER" ] || { echo "doknyu.spec diz Version: $spec_ver, não $VER; atualize-o (e o %changelog) antes" >&2; exit 1; }
if git -C "$ROOT" rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    cargo_ver="$(git -C "$ROOT" show "refs/tags/$TAG:Cargo.toml" | sed -n 's/^version = "\(.*\)"/\1/p' | head -1)"
    [ "$cargo_ver" = "$VER" ] || { echo "Cargo.toml da tag $TAG diz $cargo_ver, não $VER" >&2; exit 1; }
fi

mkdir -p "$DIR/out"
trap 'docker rm -f "doknyu-rpm-pin-$$" >/dev/null 2>&1 || true' EXIT
docker run --rm --name "doknyu-rpm-pin-$$" -e "VER=$VER" -e "UID_HOST=$(id -u)" -e "GID_HOST=$(id -g)" \
    -v "$DIR:/rpm:ro" -v "$DIR/out:/out" fedora:latest bash -euo pipefail -c '
    dnf -y install rpm-build rpmlint cargo rust cargo-rpm-macros cargo-vendor-filterer curl tar xz >/dev/null
    useradd -m builder
    runuser -u builder -- bash -euo pipefail -c "
        mkdir -p ~/rpmbuild/{SOURCES,SPECS} ~/src
        cp /rpm/doknyu.spec ~/rpmbuild/SPECS/
        curl --proto =https --tlsv1.2 -fsSL -o ~/rpmbuild/SOURCES/doknyu-$VER.tar.gz \
            https://github.com/RafaelProfMgz/doknyu/archive/refs/tags/v$VER/doknyu-$VER.tar.gz
        tar -C ~/src -xzf ~/rpmbuild/SOURCES/doknyu-$VER.tar.gz
        /rpm/make-vendor.sh ~/src/doknyu-$VER ~/rpmbuild/SOURCES/doknyu-$VER-vendor.tar.xz
        rpmbuild -bs ~/rpmbuild/SPECS/doknyu.spec
        rpmlint ~/rpmbuild/SRPMS/*.src.rpm || true
    "
    # cat (e não cp) mantém o dono dos arquivos do host.
    for f in /home/builder/rpmbuild/SRPMS/*.src.rpm; do cat "$f" > "/out/$(basename "$f")"; done
    chown "$UID_HOST:$GID_HOST" /out/*.src.rpm 2>/dev/null || true
'
echo "SRPM em packaging/rpm/out/. Para enviar ao COPR: copr-cli build <projeto> packaging/rpm/out/doknyu-$VER-1.*.src.rpm"
