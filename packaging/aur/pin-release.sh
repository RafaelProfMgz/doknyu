#!/usr/bin/env bash
# Fixa os PKGBUILDs do AUR numa release já publicada: troca o sha256 de placeholder pelo real
# (`updpkgsums`, que baixa o tarball da tag e o .deb da release) e regenera os três .SRCINFO.
#
#   packaging/aur/pin-release.sh v0.1.0     # depois de publicar a release v0.1.0 no GitHub
#
# Roda dentro de um container archlinux (makepkg não roda como root, então cria um usuário).
# Atualiza `pkgver` e zera `pkgrel` de doknyu e doknyu-bin. Depois faça commit; o push no AUR é do
# humano (ver packaging/README.md, "AUR"). Não publica nada.
set -euo pipefail

TAG="${1:-}"
[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "uso: $0 vX.Y.Z" >&2; exit 2; }
VER="${TAG#v}"

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/../.." && pwd)"

# Se a tag existe localmente, a versão do Cargo.toml dela tem de ser a do pacote.
if git -C "$ROOT" rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    cargo_ver="$(git -C "$ROOT" show "refs/tags/$TAG:Cargo.toml" | sed -n 's/^version = "\(.*\)"/\1/p' | head -1)"
    [ "$cargo_ver" = "$VER" ] || { echo "Cargo.toml da tag $TAG diz $cargo_ver, não $VER" >&2; exit 1; }
fi

for pkg in doknyu doknyu-bin doknyu-git; do
    [ -f "$DIR/$pkg/PKGBUILD" ] && touch "$DIR/$pkg/.SRCINFO"
done

docker run --rm --name "doknyu-aur-pin-$$" -e "VER=$VER" -v "$DIR:/aur" archlinux:latest bash -euo pipefail -c '
    pacman -Syu --noconfirm --needed base-devel pacman-contrib >/dev/null
    useradd -m builder
    install -d -o builder -g builder /home/builder/work
    for pkg in doknyu doknyu-bin doknyu-git; do
        cp -r "/aur/$pkg" "/home/builder/work/$pkg"
    done
    chown -R builder:builder /home/builder/work

    for pkg in doknyu doknyu-bin; do
        cd "/home/builder/work/$pkg"
        sed -i "s/^pkgver=.*/pkgver=$VER/; s/^pkgrel=.*/pkgrel=1/" PKGBUILD
        runuser -u builder -- updpkgsums
    done

    # Confere o .deb contra o SHA256SUMS que a própria release publica.
    cd /home/builder/work/doknyu-bin
    deb_sum="$(sed -n "s/^sha256sums=(\x27\(.*\)\x27)/\1/p" PKGBUILD)"
    curl --proto "=https" --tlsv1.2 -fsSL "https://github.com/RafaelProfMgz/doknyu/releases/download/v$VER/SHA256SUMS" \
        | grep -q "^$deb_sum  doknyu_$VER-1_amd64.deb\$" \
        || { echo "o sha256 do .deb nao bate com o SHA256SUMS da release" >&2; exit 1; }

    for pkg in doknyu doknyu-bin doknyu-git; do
        cd "/home/builder/work/$pkg"
        runuser -u builder -- makepkg --printsrcinfo > .SRCINFO
        # cat (e não cp) mantém o dono dos arquivos do host.
        cat .SRCINFO > "/aur/$pkg/.SRCINFO"
        cat PKGBUILD > "/aur/$pkg/PKGBUILD"
    done
'
echo "PKGBUILDs e .SRCINFO fixados em $TAG. Revise o diff e faça commit."
