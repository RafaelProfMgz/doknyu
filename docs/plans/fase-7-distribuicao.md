# Fase 7 — Distribuição: Flathub, AUR, COPR, nixpkgs e Homebrew

> **Para agentes:** SUB-SKILL OBRIGATÓRIA: use superpowers:subagent-driven-development (recomendado)
> ou superpowers:executing-plans. Ver [README.md](README.md) e
> [execucao-agentes.md](execucao-agentes.md). Os passos usam checkbox (`- [ ]`).

**Objetivo:** o doknyu instalável pelos canais nativos das principais distros, sem depender do
usuário baixar um `.deb` ou AppImage do GitHub.

**Arquitetura:** tudo de empacotamento fica em `packaging/<canal>/`, sempre construindo a partir do
tarball da tag publicada na Fase 6 (`v0.1.0`) e do `Cargo.lock` commitado. O código do app só muda
para se adaptar ao sandbox do Flatpak (Tarefa 7.2).

**Spec:** [`docs/specs/2026-10-29-doknyu-design.md`](../specs/2026-10-29-doknyu-design.md), seção 6.
Consome a Fase 6 (App ID `io.github.rafaelprofmgz.Doknyu`, ícones `hicolor`, `.desktop`, metainfo,
release `v0.1.0` com tarball).

## Restrições globais

- Valem as restrições de [README.md](README.md).
- Todo build de pacote é **offline e reprodutível** a partir da tag: `cargo build --release --locked`
  (Flatpak: fontes do cargo declaradas no manifesto; AUR/COPR/Nix: `cargo fetch --locked` ou o
  equivalente do canal).
- Os pacotes instalam `.desktop`, ícones e metainfo nos caminhos padrão
  (`/usr/share/applications`, `/usr/share/icons/hicolor/<N>x<N>/apps`, `/usr/share/metainfo`) e as
  licenças em `/usr/share/licenses/doknyu/`.
- Docker é dependência **opcional** (o app abre e mostra o onboarding sem ele).
- **Nada é publicado por agente.** Abrir PR no Flathub, dar push no AUR, criar projeto no COPR e
  abrir PR no nixpkgs e publicar o tap do Homebrew são passos do humano; os agentes deixam tudo pronto e documentado em
  `packaging/README.md`.

## Foco de revisão

1. Flatpak com Docker rootless (socket em `$XDG_RUNTIME_DIR/docker.sock`) e com `DOCKER_HOST`
   definido no host → o app acha o socket ou mostra o onboarding certo (spike 7.1).
2. Usuário no grupo `docker` rodando o Flatpak → acesso ao socket funciona; fora do grupo → tela
   de permissão negada, não erro genérico (spike 7.1).
3. Abrir porta no navegador e detectar o tema dentro do sandbox → funcionam via portal (spike 7.1).
4. Terminal externo (Fase 5) dentro do Flatpak → ação some da palette e do dock, em vez de falhar
   (Tarefa 7.2, com teste).
5. Pacote AUR instalado e removido → não sobra arquivo fora do que o pacote declarou (`namcap`,
   Tarefa 7.3).

---

### Tarefa 7.1: Spike do Flatpak (descartável)

**Implementa:** `research-implementer`. **Revisa:** humano lê a decisão.

**Arquivos:**
- Criar: `docs/decisions/0002-flatpak.md`; rascunho do manifesto em `packaging/flatpak/` (pode ser
  reescrito na 7.2).

**Pergunta:** o doknyu funciona dentro do sandbox do Flatpak com permissões que o Flathub aceite?

- [ ] **Passo 1:** Manifesto mínimo com `org.freedesktop.Platform` (versão atual),
  `org.freedesktop.Sdk.Extension.rust-stable`, `finish-args`: `--socket=wayland`,
  `--socket=fallback-x11`, `--device=dri`, `--share=ipc`, `--filesystem=/run/docker.sock`,
  `--filesystem=xdg-run/docker.sock`. Fontes do cargo geradas com `flatpak-cargo-generator.py`.
- [ ] **Passo 2:** `flatpak-builder --user --install --force-clean build packaging/flatpak/<manifesto>`
  e `flatpak run io.github.rafaelprofmgz.Doknyu`.
- [ ] **Passo 3:** Registrar no ADR, com evidência (saída de comando ou descrição do que apareceu):
  - acesso ao socket com usuário no grupo `docker` (lista carrega) e fora dele (tela de permissão);
  - `/var/run/docker.sock` é symlink para `/run/docker.sock` no host? O que o app precisa tentar;
  - Docker rootless e `DOCKER_HOST` (o ambiente do host não entra no sandbox: como configurar);
  - links de portas abrem o navegador (portal OpenURI);
  - tema claro/escuro segue o sistema (portal Settings);
  - onde fica `~/.config/doknyu/theme` (esperado: `~/.var/app/<id>/config/doknyu/theme`);
  - como detectar que está em Flatpak (`/.flatpak-info` existe);
  - terminal embutido (Fase 5) funciona; terminal externo não (sem `flatpak-spawn --host`).
- [ ] **Passo 4:** Precedentes no Flathub: buscar apps publicados com `--filesystem=/run/docker.sock`
  (ou similar) e citar no ADR como justificativa para a revisão do Flathub.
- [ ] **Passo 5:** Recomendação: seguir para 7.2 / ajustar o app antes / desistir do Flathub. Commit
  `docs: flatpak spike decision`.

### Tarefa 7.1b: Ponte de exec própria (`doknyu --exec-bridge`)

**Implementa:** `rust-implementer`. **Origem:** ADR 0002 (spike): no Flatpak não há CLI `docker`,
então a aba Terminal (que roda `docker exec -it` num PTY, ADR 0001) não funciona. Decisão do
humano: o próprio binário faz a ponte entre o PTY do `iced_term` e o exec da API do Docker.

**Arquivos:**
- Criar: `src/exec_bridge.rs`, `tests/exec_bridge.rs` (integração, `#[ignore]`)
- Modificar: `src/main.rs` (modo `--exec-bridge`), `src/lib.rs`, `src/docker/mod.rs` e
  `src/docker/bollard.rs` (operação de exec interativo, sem tipos do bollard fora de `docker/`),
  `src/views/detail/terminal.rs` (`backend_settings`), `src/terminal_launcher.rs`, `Cargo.toml`

**Comportamento:**
- `doknyu --exec-bridge <container>`: conecta pelo mesmo `docker::connect()` do app (respeita
  `DOCKER_HOST`), cria um exec com `tty: true`, stdin/stdout anexados, `Env=["TERM=xterm-256color",
  "COLORTERM=truecolor"]` e o mesmo comando de `terminal_launcher` (`bash` se houver, senão `sh`).
- Se o stdin for um TTY: põe em modo raw (restaura ao sair, inclusive em erro), lê o tamanho
  (TIOCGWINSZ) e chama o resize do exec no início e a cada SIGWINCH.
- Copia stdin → exec e exec → stdout até o exec terminar; sai com o código do exec
  (`inspect_exec`), ou 1 com mensagem em stderr se a conexão falhar.
- `terminal::backend_settings` passa a usar `std::env::current_exe()` com
  `["--exec-bridge", id]`: a aba Terminal deixa de exigir a CLI `docker` (remover o aviso de
  "CLI ausente" e o campo/checagem associados).
- O terminal externo (`terminal_launcher`) também passa a executar `<exe> --exec-bridge <id>`.
- Sem crates grandes novas: termios e ioctl via `rustix` (features `termios`) ou `libc`.

- [ ] **Passo 1: Testes que falham:** `parses_exec_bridge_args` (`["doknyu","--exec-bridge","web"]`
  → `Some("web")`; sem id → erro de uso); `backend_settings_runs_the_bridge` (program é o exe
  atual, args `["--exec-bridge", id]`); `launcher_runs_the_bridge`; integração ignorada
  `bridge_runs_a_command`: com stdin em pipe (não TTY), enviar `echo ponte-ok; exit\n` ao
  `alpine` de teste e ver `ponte-ok` no stdout e código 0.
- [ ] **Passo 2–4:** FAIL → implementar → PASS (`cargo test -- --ignored` com Docker no ar).
- [ ] **Passo 5:** Manual: aba Terminal com `top`, `vi`, resize da janela (`stty size`), Ctrl+C,
  `exit`; sem a CLI `docker` no PATH continua funcionando.
- [ ] **Passo 6:** Commit `feat: built-in exec bridge for the terminal`.

### Tarefa 7.2: Manifesto Flatpak, adaptação do app e CI

**Implementa:** `research-implementer`. **Só começa se o ADR 0002 recomendar seguir.**

**Arquivos:**
- Criar: `packaging/flatpak/io.github.rafaelprofmgz.Doknyu.yml`,
  `packaging/flatpak/cargo-sources.json`, `scripts/update-cargo-sources.sh`,
  `.github/workflows/flatpak.yml`, `src/sandbox.rs`
- Modificar: `src/lib.rs`, `src/actions.rs` (disponibilidade do terminal externo),
  `assets/*.metainfo.xml` (se o ADR pedir), `packaging/README.md`

**Interfaces:**
- Escopo extra vindo do ADR 0002: sem `DOCKER_HOST`, tentar também
  `$XDG_RUNTIME_DIR/docker.sock` (Docker rootless) antes de desistir; em Flatpak, socket ausente
  mostra o onboarding "sem acesso ao socket" (como liberar com `flatpak override`) em vez de
  "Docker não instalado". O terminal embutido funciona via Tarefa 7.1b.
- Produz: `pub fn in_flatpak() -> bool` (verifica `/.flatpak-info`) e
  `pub fn in_flatpak_at(root: &Path) -> bool` (testável).
  `actions::available(OpenExternalTerminal)` é `false` quando `in_flatpak()`.

- [ ] **Passo 1: Testes que falham:** `detects_flatpak_info` (diretório temporário com e sem
  `.flatpak-info`); `external_terminal_hidden_in_flatpak` (palette não lista a ação).
- [ ] **Passo 2–4:** FAIL → implementar → PASS (testes e clippy com e sem `fake`).
- [ ] **Passo 5:** Manifesto final apontando para o tarball da tag (`type: archive`, `sha256`),
  instalando binário, `.desktop`, ícones e metainfo; `flatpak-builder --user --install` e
  `flatpak run` → app abre e lista containers.
- [ ] **Passo 6:** `flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest
  packaging/flatpak/io.github.rafaelprofmgz.Doknyu.yml` e o mesmo para `repo` → sem erros (as
  exceções de `finish-args` justificadas ficam documentadas em `packaging/README.md`).
- [ ] **Passo 7:** `flatpak.yml`: em PR e em tag, build com `flatpak/flatpak-github-actions`;
  `scripts/update-cargo-sources.sh` falha o CI se `cargo-sources.json` estiver desatualizado em
  relação ao `Cargo.lock`.
- [ ] **Passo 8:** `packaging/README.md`, seção Flathub: passo a passo para o humano abrir o PR em
  `flathub/flathub` (branch `new-pr`), texto de justificativa das permissões e checklist do
  revisor do Flathub.
- [ ] **Passo 9:** Commit `build: flatpak manifest, sandbox detection and ci`.

### Tarefa 7.3: AUR (Arch Linux)

**Implementa:** `research-implementer`.

**Arquivos:**
- Criar: `packaging/aur/doknyu/PKGBUILD`, `packaging/aur/doknyu-bin/PKGBUILD`,
  `packaging/aur/doknyu-git/PKGBUILD`, um `.SRCINFO` em cada, `scripts/aur-check.sh`
- Modificar: `packaging/README.md`

**Conteúdo:**
- `doknyu`: compila do tarball da tag (`prepare`: `cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"`;
  `build`: `cargo build --frozen --release`; `check`: `cargo test --frozen`), `makedepends=(cargo)`.
- `doknyu-bin`: usa o binário do GitHub Release (Fase 6), `provides`/`conflicts` com `doknyu`.
- `doknyu-git`: segue `main`, `pkgver()` a partir de `git describe`.
- Os três: `depends=(gcc-libs glibc libxkbcommon wayland vulkan-icd-loader fontconfig)` (ajustar
  pelo `namcap`), `optdepends=('docker: gerenciar containers locais')`, `license=('MIT' 'Apache-2.0')`,
  instalam `.desktop`, ícones, metainfo e licenças.

- [ ] **Passo 1:** `scripts/aur-check.sh` roda num container `archlinux:latest`: `makepkg -si` de cada
  PKGBUILD, `namcap PKGBUILD` e `namcap *.pkg.tar.zst`, e `makepkg --printsrcinfo` comparado ao
  `.SRCINFO` commitado.
- [ ] **Passo 2:** O script passa sem erros para os três pacotes; o binário instalado roda
  `doknyu --version` (adicionar a flag se não existir, com teste).
- [ ] **Passo 3:** `packaging/README.md`, seção AUR: criar conta e chave SSH, `git clone
  ssh://aur@aur.archlinux.org/<pacote>.git`, copiar PKGBUILD e `.SRCINFO`, push; como atualizar a
  cada release (`updpkgsums`, `pkgrel`).
- [ ] **Passo 4:** Commit `build: aur packages`.

### Tarefa 7.4: COPR (Fedora) e nixpkgs

**Implementa:** `research-implementer`.

**Arquivos:**
- Criar: `packaging/rpm/doknyu.spec`, `packaging/nix/package.nix`, `flake.nix` (só `packages` e
  `devShells`), `scripts/rpm-check.sh`
- Modificar: `packaging/README.md`

- [ ] **Passo 1:** `.spec` com crates vendorizadas (`cargo vendor` num `Source1` gerado na release)
  e build offline; `scripts/rpm-check.sh` roda `rpmbuild -ba` e `rpmlint` num container
  `fedora:latest`; o RPM instala e `doknyu --version` responde.
- [ ] **Passo 2:** `package.nix` com `rustPlatform.buildRustPackage` (`cargoLock.lockFile`),
  `nativeBuildInputs`/`buildInputs` necessários e `postInstall` com desktop, ícones e metainfo;
  `nix build .#doknyu` num container `nixos/nix` → binário roda `--version`.
- [ ] **Passo 3:** `packaging/README.md`, seções COPR (criar projeto, `copr-cli build` com o
  `.spec` e o SRPM) e nixpkgs (copiar `package.nix` para `pkgs/by-name/do/doknyu/` e abrir PR).
- [ ] **Passo 4:** Commit `build: rpm spec for copr and nix package`.


### Tarefa 7.5: Tap do Homebrew (Linux)

**Implementa:** `research-implementer`. Referência: https://docs.brew.sh/Adding-Software-to-Homebrew
e https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap.

**Decisão:** `homebrew-core` exige notoriedade (estrelas, uso) que um projeto novo não tem; o
caminho é um **tap próprio** `RafaelProfMgz/homebrew-tap` com a fórmula `doknyu`. A fórmula é
Linux-only (`depends_on :linux`), compila do tarball da tag com `cargo install` via
`std_cargo_args`, e instala `.desktop`, ícones e metainfo em `share/`.

**Arquivos:**
- Criar: `packaging/homebrew/doknyu.rb`, `scripts/brew-check.sh`
- Modificar: `packaging/README.md`

- [ ] **Passo 1:** Fórmula com `desc`, `homepage`, `url` do tarball `v0.1.0`, `sha256`,
  `license any_of: ["MIT", "Apache-2.0"]`, `depends_on "rust" => :build`,
  `depends_on :linux`, `install` e `test do` rodando `doknyu --version`.
- [ ] **Passo 2:** `scripts/brew-check.sh` roda num container `homebrew/brew`:
  `brew style`, `brew audit --strict --new` e `brew install --build-from-source` da fórmula
  num tap local; `doknyu --version` responde.
- [ ] **Passo 3:** `packaging/README.md`, seção Homebrew: criar o repositório
  `homebrew-tap`, copiar a fórmula para `Formula/doknyu.rb`, `brew tap RafaelProfMgz/tap` e
  `brew install doknyu`; como atualizar `url`/`sha256` a cada release.
- [ ] **Passo 4:** Commit `build: homebrew tap formula`.

---

**Fim de fase:** `phase-reviewer` e parada para o humano publicar em cada canal seguindo
`packaging/README.md`.
