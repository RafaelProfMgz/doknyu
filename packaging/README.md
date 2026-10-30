# Empacotamento

Tudo de empacotamento fica em `packaging/<canal>/`. Nada aqui é publicado por agente: abrir PR no
Flathub, dar push em AUR, criar projeto no COPR etc. são passos do humano.

- `appimage/`: AppImage e `.deb` (Fase 6, ver `.github/workflows/release.yml`).
- `flatpak/`: Flatpak / Flathub (Fase 7, Tarefa 7.2; decisões em `docs/decisions/0002-flatpak.md`).

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
