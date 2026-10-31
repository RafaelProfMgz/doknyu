# Empacotamento

Tudo de empacotamento fica em `packaging/<canal>/`. Nada aqui é publicado por agente: abrir PR no
Flathub, dar push em AUR, criar projeto no COPR etc. são passos do humano.

- `appimage/`: AppImage e `.deb` (Fase 6, ver `.github/workflows/release.yml`).
- `flatpak/`: Flatpak / Flathub (Fase 7, Tarefa 7.2; decisões em `docs/decisions/0002-flatpak.md`).
- `aur/`: pacotes do AUR `doknyu`, `doknyu-bin` e `doknyu-git` (Fase 7, Tarefa 7.3).
- `rpm/`: `.spec` do COPR (Fedora) (Fase 7, Tarefa 7.4).
- `nix/` e `../flake.nix`: derivação para o nixpkgs e flake do repositório (Fase 7, Tarefa 7.4).

## Flathub

### Arquivos

| Arquivo | Para quê |
|---|---|
| `flatpak/io.github.rafaelprofmgz.Doknyu.yml` | Manifesto que vai para o Flathub. Fonte `type: git` (`url` + `commit`). |
| `flatpak/pin-release.sh` | Na release: fixa `tag` + `commit` no manifesto a partir de uma tag local e confere o `cargo-sources.json` da tag. |
| `flatpak/make-local-manifest.sh` | Gera `...Doknyu.local.yml` (ignorado pelo git): o mesmo manifesto, com a fonte trocada por `type: dir` (a árvore atual), para testar a branch. O CI do PR usa esse. |
| `flatpak/cargo-sources.json` | Crates do `Cargo.lock` (build offline). Gerado, não editar à mão. |
| `scripts/update-cargo-sources.sh` | Regenera o `cargo-sources.json`; com `--check` só confere (o CI falha se estiver velho). O gerador (`flatpak-cargo-generator.py`, flatpak-builder-tools `41c20aa1…`) tem commit e SHA-256 fixos no script. |
| `.github/workflows/flatpak.yml` | Em PR e em tag: `--check` + build com `flatpak/flatpak-github-actions` (fixado por SHA). |

Runtime: `org.freedesktop.Platform//26.08` (a 26.08 já existe e o `flatpak-builder-lint` avisava
`runtime-update-available` para a 25.08) com `rust-stable//26.08` (Rust 1.98; o MSRV do projeto é 1.89).

### Build e teste local

```sh
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install --user flathub org.flatpak.Builder org.freedesktop.Platform//26.08 \
    org.freedesktop.Sdk//26.08 org.freedesktop.Sdk.Extension.rust-stable//26.08
cd packaging/flatpak
./make-local-manifest.sh
# Dentro do flatpak-builder (que é um Flatpak) o rofiles-fuse não funciona e os diretórios de
# build têm de ficar sob $HOME (não /tmp):
flatpak run org.flatpak.Builder --user --install --force-clean --disable-rofiles-fuse \
    --state-dir=.flatpak-builder build-dir io.github.rafaelprofmgz.Doknyu.local.yml
RUST_LOG=debug,bollard=debug flatpak run io.github.rafaelprofmgz.Doknyu   # ver _ping e containers/json
```

Sem a janela à vista, confira pelos logs: `_ping` e `containers/json` aparecem e não há erro.
A aba Terminal roda `doknyu --exec-bridge <id>` (ponte própria, sem a CLI `docker`) e funciona no
sandbox; teste: `flatpak run --command=doknyu io.github.rafaelprofmgz.Doknyu --exec-bridge <id>`.

### Linter

```sh
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest \
    packaging/flatpak/io.github.rafaelprofmgz.Doknyu.yml        # limpo
flatpak run org.flatpak.Builder --user --disable-rofiles-fuse --state-dir=.flatpak-builder \
    --repo=repo --force-clean build-dir io.github.rafaelprofmgz.Doknyu.local.yml   # em packaging/flatpak
flatpak run --command=flatpak-builder-lint org.flatpak.Builder repo repo
```

`repo` aponta **dois erros esperados até existir a tag `v0.1.0`**: `appstream-missing-screenshots` e
`appstream-screenshots-not-mirrored-in-ostree`. As URLs do metainfo apontam para
`raw.githubusercontent.com/.../v0.1.0/docs/screenshots/*.png`, que dá 404 enquanto a tag não existe.
Depois da tag, rode de novo; deve ficar limpo. (O `manifest` do `.local.yml` acusa
`appid-filename-mismatch` pelo nome do arquivo; é só do arquivo gerado, ignore.)

### Docker dentro do sandbox

O app usa `/var/run/docker.sock` (coberto por `--filesystem=/run/docker.sock`, pois `/var/run` é
symlink para `/run`) e, se não existir, `$XDG_RUNTIME_DIR/docker.sock` (Docker rootless, coberto por
`--filesystem=xdg-run/docker.sock`). `DOCKER_HOST` do host **não** entra no sandbox:

```sh
flatpak override --user --env=DOCKER_HOST=unix:///run/user/1000/docker.sock io.github.rafaelprofmgz.Doknyu
```

No Flatpak, o onboarding "Docker não instalado" vira "O Doknyu não enxerga o socket
do Docker", com os comandos `flatpak override --user --filesystem=...` (botão Copiar). Usuário fora do
grupo `docker`: tela de permissão negada, como no pacote nativo. O terminal **externo** não existe no
Flatpak (sem emulador do host): a ação some da palette e do dock.

### Justificativa das permissões (texto para o PR no Flathub)

> **Permissions.** Doknyu is a desktop client for the Docker Engine API (containers, logs, stats,
> volumes, networks and a per-container terminal). The Docker unix socket is its only channel to the
> daemon, so it cannot work without it. Static permissions are the minimum:
>
> - `--socket=wayland`, `--socket=fallback-x11`, `--device=dri`, `--share=ipc`: GPU-rendered UI (iced/wgpu).
> - `--filesystem=/run/docker.sock`: the standard rootful socket (`/var/run` is a symlink to `/run`).
> - `--filesystem=xdg-run/docker.sock`: the rootless Docker socket in `$XDG_RUNTIME_DIR`.
>
> No `--share=network`, no `home`/`host`, no `org.freedesktop.Flatpak`. The terminal tab does not
> use the `docker` CLI: the app opens the exec through the Docker API itself (`doknyu --exec-bridge`),
> so no host command is needed. Access to the Docker socket is equivalent to root on the host; this
> is inherent to any Docker client. Same permissions are used
> by existing Flathub apps: `com.github.sdv43.whaler` (`/run/docker.sock` and `xdg-run/docker.sock`),
> `com.github.marhkb.Pods` (`/run/docker.sock`), `io.github.buonhobo.Katharsis` (`/run/docker.sock`)
> and `io.podman_desktop.PodmanDesktop` (`/run/docker.sock`, with broader permissions than ours).

### Passo a passo: PR em `flathub/flathub` (humano)

Antes: leia a *Generative AI policy* e os *requirements* do Flathub
(<https://docs.flathub.org/docs/for-app-authors/submission>); o texto do PR deve ser escrito por você.

1. Publique a release `v0.1.0` (Fase 6): crie a tag (screenshots acessíveis nas URLs do metainfo).
2. Com a tag criada localmente, rode `packaging/flatpak/pin-release.sh v0.1.0`: ele preenche `tag` e
   `commit` do manifesto (o repositório guarda o placeholder `vX.Y.Z`/`0000…` de propósito, para o
   manifesto não compilar por engano) e confere o `cargo-sources.json` contra o `Cargo.lock` **da
   tag**, num worktree temporário. Faça commit do manifesto fixado; no CI da tag, o mesmo pin é
   refeito e comparado com `github.sha`.
   Rode `flatpak-builder-lint manifest` e `repo` (devem ficar limpos).
3. Faça o fork de `flathub/flathub` com **"Copy the master branch only" desmarcado**
   (`gh repo fork --clone flathub/flathub`).
4. `git clone --branch=new-pr git@github.com:<usuario>/flathub.git && cd flathub`, depois
   `git checkout -b add-doknyu new-pr`.
5. Copie para a raiz do clone: `io.github.rafaelprofmgz.Doknyu.yml` e `cargo-sources.json`
   (nenhum outro arquivo; o `.desktop`, ícones e metainfo vêm do código-fonte).
6. Commit, push e abra o PR contra a base **`new-pr`** (não `master`), com o título
   `Add io.github.rafaelprofmgz.Doknyu`, mantendo o template, e cole a justificativa acima.
7. Responda aos revisores empurrando commits no mesmo branch (sem merge de `master`). Quando
   resolver os comentários, comente `bot, build`.
8. Aprovado: aceite o convite do repositório em até 1 semana (ative 2FA antes). Atualizações depois
   disso não passam por PR em `flathub/flathub`: vão no repositório do app.

Checklist:

- [ ] Tag `v0.1.0` publicada, screenshots do metainfo abrem (200, não 404)
- [ ] Ordem seguida: tag, `pin-release.sh <tag>`, commit do manifesto, PR no Flathub
- [ ] Manifesto sem placeholder (`vX.Y.Z`/`0000…`), `cargo-sources.json` em dia (o `pin-release.sh` já confere)
- [ ] `flatpak-builder-lint manifest` e `repo` limpos
- [ ] Build local instalado e testado com `flatpak run` (lista containers, aba Terminal abre)
- [ ] Policy de IA e requirements lidos; PR contra `new-pr`, título `Add <app id>`
- [ ] Justificativa das permissões no PR; nenhum `--share=network`, `home` ou `host`
- [ ] 2FA ligado antes de aceitar o convite

### A cada release

Crie a tag, rode `pin-release.sh <tag>` e atualize o `<release>` do metainfo (antes da tag).

## AUR (Arch Linux)

Três pacotes, cada um em `aur/<pacote>/` com `PKGBUILD` e `.SRCINFO` (o AUR lê o `.SRCINFO`; ele é
gerado, não editar à mão):

| Pacote | Fonte | Notas |
|---|---|---|
| `doknyu` | tarball da tag `v$pkgver` do GitHub | Compila (`cargo build --frozen --release`, `prepare` com `cargo fetch --locked`), roda `cargo test --release` no `check()` (reaproveita o build). |
| `doknyu-bin` | o `.deb` da release (`doknyu_$pkgver-1_amd64.deb`) | Não compila; o `prepare()` abre o `data.tar.*` do `.deb`. `provides=(doknyu)`, `conflicts=(doknyu doknyu-git)`. |
| `doknyu-git` | `main` | `pkgver()` de `git describe` (`0.1.0.r<N>.g<hash>`); sem tag ainda, cai em `r<contagem>.g<hash>`. Funciona já. |

Decisões (todas conferidas por `namcap`):

- `depends=(gcc-libs glibc libxkbcommon wayland vulkan-icd-loader fontconfig)`. O binário só linka
  `libgcc_s`, `libm` e `libc`; Wayland, xkbcommon e Vulkan são `dlopen` (winit/wgpu), então o `namcap`
  não os vê, mas sem eles a janela não abre. O `namcap` por isso avisa (`W:`, aceitos) "may not be
  needed" para `gcc-libs`, `libxkbcommon`, `wayland`, `vulkan-icd-loader` e `fontconfig`, e
  "Unused shared library ld-linux"; o único `E:` que apareceu (`hicolor-icon-theme`) virou dependência.
- Docker só em `optdepends` (o app abre e mostra o onboarding sem ele; o terminal usa a ponte
  `doknyu --exec-bridge`, não a CLI `docker`).
- `license=('MIT OR Apache-2.0' 'OFL-1.1' 'ISC')`: `MIT OR Apache-2.0` é a licença do projeto (expressão SPDX,
  em vez de `('MIT' 'Apache-2.0')`, que o AUR leria como "as duas valem"); o binário embute as fontes IBM Plex
  (OFL) e o Lucide (ISC), cujo texto vai em `LICENSES-fonts.md`. Os textos ficam em
  `/usr/share/licenses/<pacote>/` (`LICENSE-MIT`, `LICENSE-APACHE`, `LICENSES-fonts.md`).
- `options=('!lto')` nos pacotes que compilam (recomendação da ArchWiki para Rust: o LTO do makepkg
  não combina com o do cargo).
- Instalam o `.desktop`, o metainfo, o SVG e os PNGs de `hicolor` nos caminhos padrão.

### Os PKGBUILDs ainda têm placeholder

A tag `v0.1.0` e a release não existem, então `doknyu` e `doknyu-bin` têm `sha256sums` de zeros
(`SKIP` não é aceito para tarball de release). Depois de publicar a release (tag, tarball e o `.deb`
do `release.yml`), na raiz do repositório:

```sh
packaging/aur/pin-release.sh v0.1.0   # container archlinux: updpkgsums + makepkg --printsrcinfo
git diff packaging/aur                # só pkgver/pkgrel, sha256sums e .SRCINFO mudam
git commit -am "build: pin the aur packages at v0.1.0"
```

O script também confere o `.deb` contra o `SHA256SUMS` da release. Depois dele, o build completo
com as fontes reais (que o `scripts/aur-check.sh` não pode fazer antes da tag) é:

```sh
cd packaging/aur/doknyu && makepkg -sf && namcap PKGBUILD *.pkg.tar.zst
cd ../doknyu-bin && makepkg -sf && namcap PKGBUILD *.pkg.tar.zst
```

### Validação (`scripts/aur-check.sh`)

```sh
scripts/aur-check.sh              # os três; ou: scripts/aur-check.sh doknyu-git
```

Num container `archlinux:latest` (nome `doknyu-spike-aur-check-*`, removido ao fim), com um usuário sem
root para o `makepkg`: `namcap PKGBUILD`, `.SRCINFO` gerado igual ao commitado, `makepkg -s`
completo, `namcap` no pacote, `pacman -U`, `doknyu --version`, `pacman -R` e comparação dos arquivos
do sistema antes e depois (nada pode sobrar). Como a tag não existe, o build usa um **override só do
teste** (anexado a uma cópia do PKGBUILD) com as mesmas fontes tiradas da árvore de trabalho: um
tarball `doknyu-<versão>/`, um clone git local e o `.deb` de `target/debian/` (`cargo build --release
&& cargo deb --no-build`; sem ele, `doknyu-bin` só passa por `namcap PKGBUILD` e `.SRCINFO`).
Sai com erro se algo falhar ou se o `namcap` der `E:`.

### Passo a passo: publicar no AUR (humano)

Nada disso é feito por agente.

1. Crie a conta em <https://aur.archlinux.org/register>, cadastre sua chave SSH pública em "My
   Account" (`ssh-keygen -t ed25519 -f ~/.ssh/aur`; no `~/.ssh/config`: `Host aur.archlinux.org`,
   `IdentityFile ~/.ssh/aur`, `User aur`).
2. Rode `packaging/aur/pin-release.sh v0.1.0` (acima) e faça commit.
3. Para cada pacote (`doknyu`, `doknyu-bin`, `doknyu-git`):
   ```sh
   git clone ssh://aur@aur.archlinux.org/<pacote>.git /tmp/aur-<pacote>   # clone vazio na 1a vez
   cp packaging/aur/<pacote>/{PKGBUILD,.SRCINFO} /tmp/aur-<pacote>/
   cd /tmp/aur-<pacote> && git add PKGBUILD .SRCINFO
   git commit -m "Initial import of doknyu v0.1.0"
   git push        # a branch do AUR é master
   ```
   O AUR recusa o push sem `.SRCINFO` ou com `pkgname` diferente do repositório.
4. Marque `doknyu-bin` e `doknyu-git` como mantidos por você; confira em
   <https://aur.archlinux.org/packages/doknyu> e instale com `yay -S doknyu` numa máquina Arch.

### A cada release

1. Crie a tag, publique a release (o `release.yml` gera o `.deb` e o `SHA256SUMS`).
2. `packaging/aur/pin-release.sh vX.Y.Z`: atualiza `pkgver`, volta `pkgrel` para 1, refaz os
   `sha256sums` (`updpkgsums`) e os `.SRCINFO`. Se só o PKGBUILD mudar (sem versão nova), aumente
   `pkgrel` à mão e rode `makepkg --printsrcinfo > .SRCINFO` em cada pasta.
3. Rode `scripts/aur-check.sh` e faça o push de `doknyu` e `doknyu-bin` (o `doknyu-git` só precisa de
   push se o PKGBUILD mudar; o `pkgver()` acompanha o `main`).

## COPR (Fedora)

| Arquivo | Para quê |
|---|---|
| `rpm/doknyu.spec` | O `.spec`. `Source0` é o tarball da tag; `Source1` (`doknyu-%{version}-vendor.tar.xz`) são as crates vendorizadas, **não fica no repositório**. Build offline com `%cargo_prep -v vendor`, `%cargo_build`, `%cargo_install` e `%cargo_test` (reaproveita o build). |
| `rpm/make-vendor.sh` | Gera o `Source1` com `cargo vendor-filterer` (só crates de Linux x86_64/aarch64) e confere que o `Cargo.lock` não mudou. |
| `rpm/pin-release.sh` | Na release: baixa o tarball da tag, gera o vendor e o SRPM em `rpm/out/` (container `fedora:latest`). |
| `scripts/rpm-check.sh` | Validação local (abaixo). |

Decisões:

- `Requires` explícitos para o que o binário carrega com `dlopen` (o rpm não vê no ELF):
  `libwayland-client`, `libwayland-cursor`, `libxkbcommon`, `libxkbcommon-x11`, `libX11`, `libXcursor`,
  `libXi`, `libxcb`, `libglvnd-egl`, `vulkan-loader` (e `hicolor-icon-theme`, dono dos diretórios de ícones).
  `Recommends: mesa-vulkan-drivers` e `(moby-engine or docker-ce)`: Docker é só recomendação (o daemon pode ser
  remoto/rootless; o terminal usa `doknyu --exec-bridge`, não a CLI `docker`).
- `License:` junta a licença do projeto (`MIT OR Apache-2.0`), as das fontes embutidas (OFL-1.1, ISC) e a das
  crates (saída de `%{cargo_license_summary}`, conferida na hora do build; o detalhe por crate vai em
  `LICENSE.dependencies`, junto com `cargo-vendor.txt`, ambos em `%license`). **Ao atualizar o `Cargo.lock`, confira
  o `License:` com o resumo que o `rpmbuild` imprime.**
- `%check` valida o `.desktop` (`desktop-file-validate`) e o metainfo (`appstreamcli validate --no-net`).
- O `Release` é `1%{?dist}` com `%changelog` manual (o COPR constrói de SRPM, sem histórico git).

### Validação (`scripts/rpm-check.sh`)

```sh
scripts/rpm-check.sh
```

Num container `fedora:latest` (nome `doknyu-spike-rpm-check-*`, removido ao fim; sem `sudo` no host): gera o
tarball do código e o vendor a partir da árvore local (a tag não existe ainda), `rpmlint` do `.spec`,
`rpmbuild -ba` (offline), `rpmlint` do SRPM e do RPM, `dnf install` do RPM, `doknyu --version`, `dnf remove`.
Erros `E:` do `rpmlint` e qualquer acesso à rede durante o build falham o script; avisos `W:` são conferidos à mão
(o `invalid-url Source1` é esperado: o vendor é gerado, não baixado).

### Passo a passo: publicar no COPR (humano)

Nada disso é feito por agente.

1. Crie a conta (login pelo FAS) em <https://copr.fedorainfracloud.org>, gere o token em
   <https://copr.fedorainfracloud.org/api/> e salve em `~/.config/copr`. `sudo dnf install copr-cli`.
2. Crie o projeto (uma vez), com os chroots que o app suporta (o MSRV 1.89 exige Fedora recente):
   ```sh
   copr-cli create doknyu --chroot fedora-rawhide-x86_64 --chroot fedora-rawhide-aarch64 \
       --description "Desktop manager for Docker containers" \
       --instructions "sudo dnf copr enable <usuario>/doknyu && sudo dnf install doknyu"
   ```
   (acrescente os `fedora-<N>-x86_64`/`aarch64` das versões em suporte; `copr-cli list-chroots`.)
3. Publique a release `v0.1.0` (Fase 6), depois, na raiz do repositório:
   ```sh
   packaging/rpm/pin-release.sh v0.1.0          # SRPM em packaging/rpm/out/
   copr-cli build doknyu packaging/rpm/out/doknyu-0.1.0-1.*.src.rpm
   ```
   O COPR compila sem rede, só com o que está no SRPM (por isso o vendor).
4. Teste: `sudo dnf copr enable <usuario>/doknyu && sudo dnf install doknyu && doknyu --version`.

### A cada release

Crie a tag, atualize `Version:` e o `%changelog` do `.spec` (`Release` volta para 1; só o `.spec` mudou: aumente
o `Release`), confira o `License:` e rode `scripts/rpm-check.sh`, `packaging/rpm/pin-release.sh vX.Y.Z` e `copr-cli build`.

## nixpkgs

| Arquivo | Para quê |
|---|---|
| `nix/package.nix` | A derivação (`rustPlatform.buildRustPackage`, `fetchFromGitHub` da tag). `hash` e `cargoHash` são `lib.fakeHash` até existir a tag. |
| `nix/pin-release.sh` | Na release: calcula `hash` (tarball da tag) e `cargoHash` e atualiza `version` (container `nixos/nix`). |
| `../flake.nix` | Só `packages` (`doknyu`, `default`) e `devShells`. Constrói o mesmo `package.nix` **da árvore local** (`overrideAttrs` com `src` e `cargoDeps = importCargoLock { lockFile }`), é assim que se testa antes da tag. `flake.lock` fixa o `nixos-unstable`. |
| `scripts/nix-check.sh` | Validação (abaixo). |

Decisões:

- O `package.nix` usa `cargoHash`, não `cargoLock.lockFile`: o nixpkgs não aceita `Cargo.lock` copiado para a árvore
  (e `lockFile = "${src}/Cargo.lock"` seria IFD). O `lockFile` é usado no flake, para o teste local.
- Wayland, X11, xkbcommon, Vulkan e GL são `dlopen`: o `postFixup` põe as bibliotecas no rpath com
  `patchelf --add-rpath`, como o `halloy` (app iced/winit do nixpkgs) faz. `nix-check.sh` confere o rpath.
- Instala `.desktop`, metainfo, ícones (`hicolor`), LICENSE-MIT, LICENSE-APACHE e `LICENSES-fonts.md`
  (em `share/licenses/doknyu/`). `meta.license = [ asl20 mit ofl isc ]`. `maintainers` está vazio: ao abrir a PR,
  acrescente seu handle em `maintainers/maintainer-list.nix` (PR separada ou a mesma) e aqui.
- `doCheck` fica ligado: o `cargo test --release` do nix roda os 470 testes (os de Docker são `#[ignore]`).

### Validação (`scripts/nix-check.sh`)

```sh
scripts/nix-check.sh              # build completo (vários minutos: compila iced/wgpu)
scripts/nix-check.sh --no-build   # só `nix flake check --no-build` e `nix eval`
```

Num container `nixos/nix` (nome `doknyu-spike-nix-check-*`, removido ao fim; flakes habilitados, `sandbox = false`
porque o container não é privilegiado): a árvore de trabalho vira um repositório git descartável (o flake só vê
arquivos rastreados), `nix flake check --no-build`, `nix eval`, `nix build .#doknyu`, `result/bin/doknyu --version`,
rpath e arquivos instalados. Se não houver `flake.lock`, o gerado é copiado para a raiz; para atualizar o
`nixpkgs`, apague-o e rode de novo.

### Passo a passo: PR no nixpkgs (humano)

Nada disso é feito por agente. Leia o *contributing guide* do nixpkgs (<https://github.com/NixOS/nixpkgs/blob/master/pkgs/README.md>).

1. Publique a release `v0.1.0` (Fase 6) e rode `packaging/nix/pin-release.sh v0.1.0` (fixa `hash` e `cargoHash`); commit.
2. Faça o fork de `NixOS/nixpkgs`, crie um branch a partir do `master` e copie:
   ```sh
   mkdir -p pkgs/by-name/do/doknyu && cp packaging/nix/package.nix pkgs/by-name/do/doknyu/package.nix
   ```
3. Acrescente o seu handle em `maintainers/maintainer-list.nix` (commit separado, `maintainers: add <handle>`) e em
   `meta.maintainers`; adicione `passthru.updateScript = nix-update-script { };` (com `nix-update-script` nos argumentos).
4. `nix-build -A doknyu` (e `nixpkgs-review wip`), `nix run nixpkgs#nixfmt -- pkgs/by-name/do/doknyu/package.nix`.
5. Commit `doknyu: init at 0.1.0`, push e abra a PR contra `master` seguindo o template (marque as caixas que
   cumprir; o texto da PR deve ser escrito por você).

### A cada release

`packaging/nix/pin-release.sh vX.Y.Z` atualiza o `package.nix` do repositório; no nixpkgs, a atualização costuma ser
feita pelo `r-ryantm`/`nix-update` (por isso o `updateScript`), ou rode `nix-update doknyu`.
