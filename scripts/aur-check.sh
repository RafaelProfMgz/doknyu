#!/usr/bin/env bash
# Valida os PKGBUILDs de packaging/aur/ num container archlinux:latest.
#
#   scripts/aur-check.sh [doknyu] [doknyu-bin] [doknyu-git]     # sem argumentos: os três
#
# Para cada pacote:
#   - `namcap PKGBUILD` e `makepkg --printsrcinfo` comparado ao .SRCINFO commitado;
#   - build completo (`makepkg -s`), `namcap *.pkg.tar.zst`, instala (`pacman -U`), roda
#     `doknyu --version`, remove (`pacman -R`) e confere que não sobrou nenhum arquivo.
#
# Os PKGBUILDs apontam para a tag e para a release, que ainda podem não existir; por isso o build
# usa um OVERRIDE SÓ DO TESTE (acrescentado a uma cópia do PKGBUILD, o commitado não muda) com as
# mesmas fontes tiradas desta árvore de trabalho:
#   doknyu      tarball `doknyu-<versão>/` (o layout do tarball do GitHub) gerado do repositório local
#   doknyu-git  `git+file://` de um clone local com a árvore de trabalho commitada por cima
#   doknyu-bin  o .deb de target/debian/ (`cargo build --release && cargo deb --no-build`); sem
#               ele, esse pacote só passa por namcap PKGBUILD e .SRCINFO.
# Com a tag e a release publicadas, o build completo de doknyu e doknyu-bin com as fontes reais
# fica por conta do `packaging/aur/pin-release.sh` + `makepkg -s` (ver packaging/README.md).
#
# Falha se o build, o `--version`, o .SRCINFO ou a limpeza falharem, ou se o namcap der `E:`
# (avisos `W:` são impressos e conferidos à mão). Não publica nada.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKGS=("$@")
[ ${#PKGS[@]} -gt 0 ] || PKGS=(doknyu doknyu-bin doknyu-git)
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

# Clone local + árvore de trabalho (inclusive o que ainda não foi commitado) num commit descartável.
git clone -q --no-hardlinks "$ROOT" "$STAGE/src"
git -C "$STAGE/src" rm -rqf --ignore-unmatch . >/dev/null
git -C "$ROOT" ls-files -z --cached --others --exclude-standard \
    | tar -C "$ROOT" --null -T - --ignore-failed-read -cf - | tar -C "$STAGE/src" -xf -
git -C "$STAGE/src" add -A
git -C "$STAGE/src" -c user.name=aur-check -c user.email=aur-check@localhost \
    commit -q --allow-empty -m "snapshot of the working tree"
git -C "$STAGE/src" archive --prefix="doknyu-$VERSION/" HEAD | gzip -n > "$STAGE/doknyu-$VERSION.tar.gz"
shopt -s nullglob
debs=("$ROOT"/target/debian/doknyu_"$VERSION"-1_amd64.deb)
[ ${#debs[@]} -eq 0 ] || cp "${debs[0]}" "$STAGE/doknyu_$VERSION-1_amd64.deb"
[ -f "$STAGE/doknyu_$VERSION-1_amd64.deb" ] || echo "aviso: sem target/debian/*.deb; doknyu-bin só terá namcap PKGBUILD e .SRCINFO" >&2

docker run --rm -i --name "doknyu-spike-aur-check-$$" \
    -e "PKGS=${PKGS[*]}" -e "VERSION=$VERSION" \
    -v "$ROOT/packaging/aur:/aur:ro" -v "$STAGE:/stage:ro" \
    archlinux:latest bash -s <<'INNER'
set -uo pipefail
pacman -Syu --noconfirm --needed base-devel namcap rust git >/dev/null || exit 1
useradd -m builder
echo 'builder ALL=(ALL) NOPASSWD: ALL' > /etc/sudoers.d/builder
runuser -u builder -- git config --global safe.directory '*'
install -d -o builder -g builder /home/builder/work

# Arquivos do sistema (fora do banco do pacman e dos caches), para conferir a remoção.
snapshot() { find /usr /etc /opt /srv /root /home /var -xdev \
    -not -path '/var/lib/pacman/*' -not -path '/var/cache/*' -not -path '/var/log/*' \
    -not -path '/home/builder/*' -not -path '/usr/share/doc/*' 2>/dev/null | sort; }

fail=0
for pkg in $PKGS; do
    echo; echo "=================== $pkg ==================="
    w="/home/builder/work/$pkg"
    cp -r "/aur/$pkg" "$w"; chown -R builder:builder "$w"; cd "$w"

    echo "--- namcap PKGBUILD"
    namcap PKGBUILD | tee namcap-pkgbuild.txt
    grep -q ' E: ' namcap-pkgbuild.txt && { echo "FALHA: namcap PKGBUILD"; fail=1; }

    echo "--- .SRCINFO"
    runuser -u builder -- makepkg --printsrcinfo > generated.SRCINFO
    if diff -u .SRCINFO generated.SRCINFO; then echo ".SRCINFO confere"; else echo "FALHA: .SRCINFO desatualizado"; fail=1; fi

    # Override só do teste: as fontes reais (tag/release) ainda não existem.
    case "$pkg" in
        doknyu)     printf '%s\n' 'source=("$pkgname-$pkgver.tar.gz::file:///stage/doknyu-$pkgver.tar.gz")' "sha256sums=('SKIP')" >> PKGBUILD ;;
        doknyu-git) printf '%s\n' 'source=("$pkgname::git+file:///stage/src")' >> PKGBUILD ;;
        doknyu-bin)
            if [ ! -f "/stage/doknyu_$VERSION-1_amd64.deb" ]; then echo "(sem .deb local: build de doknyu-bin pulado)"; continue; fi
            printf '%s\n' 'source=("$pkgname-$pkgver.deb::file:///stage/doknyu_$pkgver-1_amd64.deb")' "sha256sums=('SKIP')" >> PKGBUILD ;;
    esac

    echo "--- makepkg -s (build completo)"
    if ! runuser -u builder -- makepkg -sf --noconfirm > build.log 2>&1; then
        tail -40 build.log; echo "FALHA: makepkg $pkg"; fail=1; continue
    fi
    grep -E '^==> (Making|Running|Finished|Starting|Entering)|test result|warning: ' build.log | head -30

    f=$(ls ./*.pkg.tar.zst | grep -v -- '-debug-' | head -1)
    echo "--- namcap $f"
    namcap "$f" | tee namcap-pkg.txt
    grep -q ' E: ' namcap-pkg.txt && { echo "FALHA: namcap do pacote"; fail=1; }
    echo "--- conteúdo"; bsdtar -tf "$f" | grep -v '/$' | grep -v -E 'hicolor/[0-9x]+/apps'

    echo "--- instala, roda, remove"
    snapshot > /tmp/before.txt
    pacman -U --noconfirm "$f" >/dev/null || { echo "FALHA: pacman -U"; fail=1; continue; }
    out="$(doknyu --version 2>&1)"; echo "doknyu --version => $out (rc=$?)"
    [ "$out" = "doknyu $VERSION" ] || { echo "FALHA: --version"; fail=1; }
    desktop-file-validate /usr/share/applications/io.github.rafaelprofmgz.Doknyu.desktop 2>/dev/null || true
    pacman -R --noconfirm "$(pacman -Qq | grep -E '^doknyu(-bin|-git)?$')" >/dev/null || { echo "FALHA: pacman -R"; fail=1; continue; }
    snapshot > /tmp/after.txt
    if diff /tmp/before.txt /tmp/after.txt; then echo "remoção limpa: nenhum arquivo sobrou"; else echo "FALHA: sobrou arquivo (acima)"; fail=1; fi
done

echo; [ "$fail" -eq 0 ] && echo "AUR-CHECK: OK" || echo "AUR-CHECK: FALHOU"
exit "$fail"
INNER
