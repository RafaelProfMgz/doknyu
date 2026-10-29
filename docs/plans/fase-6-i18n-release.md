# Fase 6 — Polimento, i18n e release

> Ver [README.md](README.md).

**Entrega:** v0.1.0 publicada no GitHub com AppImage e .deb, interface completa em en e pt-BR.

---

### Tarefa 6.1: Cobertura de traduções

**Arquivos:**
- Modificar: `locales/en.yml`, `locales/pt-BR.yml`
- Criar: `tests/locales.rs`

- [ ] **Passo 1: Testes que falham:**
  - `locales_have_same_keys`: o conjunto de chaves de `en.yml` == o de `pt-BR.yml`.
  - `no_hardcoded_ui_strings`: varre `src/views/**/*.rs` e falha se encontrar `text("` seguido de
    letra (literal sem `t!`); exceções explícitas numa lista no próprio teste (ex.: `"—"`, `"…"`).
- [ ] **Passo 2–4:** FAIL → corrigir chaves/strings → PASS.
- [ ] **Passo 5:** Manual: `LANG=pt_BR.UTF-8 cargo run` e `LANG=en_US.UTF-8 cargo run` → todas as telas
  (inclusive onboarding e diálogos) no idioma certo, com acentuação correta.
- [ ] **Passo 6:** Commit `feat(i18n): complete en and pt-BR translations`.

### Tarefa 6.2: Identidade do app e integração com o desktop

**Implementa:** `research-implementer` (ferramentas `rsvg-convert`, `appstreamcli`, `desktop-file-validate`).

**Decisões fixas:**
- **App ID:** `io.github.rafaelprofmgz.Doknyu` (nome dos arquivos de desktop, ícone e metainfo; o
  binário continua `doknyu`).
- **Ícone:** monograma **"d"** geométrico cuja barriga é uma caixa/container de cantos arredondados
  (traço grosso e uniforme, sem detalhes finos), na cor `accent` `#74ade8` sobre um quadrado
  arredondado `#282c33` com borda de 1px `#363c46`. Precisa ser legível em 16 px: no máximo duas
  formas, sem texto, sem gradiente, sem nada que lembre o logo oficial do Docker. `viewBox` 128×128.
- **Janela:** ícone da janela a partir do PNG embutido; tamanho inicial 1200×760, mínimo 900×600.

**Arquivos:**
- Criar:
  - `assets/icons/io.github.rafaelprofmgz.Doknyu.svg` (fonte única do ícone);
  - `assets/icons/hicolor/<N>x<N>/apps/io.github.rafaelprofmgz.Doknyu.png` para N = 16, 24, 32,
    48, 64, 128, 256, 512 (gerados com `rsvg-convert`, commitados);
  - `scripts/render-icons.sh` (regera os PNGs a partir do SVG);
  - `assets/io.github.rafaelprofmgz.Doknyu.desktop`;
  - `assets/io.github.rafaelprofmgz.Doknyu.metainfo.xml`;
  - `docs/screenshots/` (PNGs tirados pelo humano; ver Passo 6).
- Modificar: `src/main.rs` (ícone, tamanhos da janela), `Cargo.toml` (feature de imagem do iced se
  `window::icon::from_file_data` exigir; senão decodificar com `iced::window::icon::from_rgba`).

**Conteúdo:**
- `.desktop`: `Name=doknyu`, `Comment=Manage your Docker containers`,
  `Comment[pt_BR]=Gerencie seus containers Docker`, `Exec=doknyu`,
  `Icon=io.github.rafaelprofmgz.Doknyu`, `Categories=Development;System;`,
  `Keywords=docker;container;devops;`, `StartupWMClass` igual ao app id que o iced/winit define
  (definir explicitamente em `main.rs` com o App ID).
- metainfo: `id` = App ID, `metadata_license` CC0-1.0, `project_license` `MIT OR Apache-2.0`,
  `name`, `summary` (en e pt-BR), `description` curta, `launchable`, `url` homepage e bugtracker
  (repositório GitHub), `content_rating type="oars-1.1"` (vazio), `releases` com `0.1.0`,
  `screenshots` apontando para URLs `raw.githubusercontent.com/RafaelProfMgz/doknyu/<tag>/docs/screenshots/…`,
  `branding` com as cores `#74ade8` (claro) e `#282c33` (escuro), `requires`/`recommends` com
  `display_length` ≥ 900 e `control` keyboard/pointing.

- [ ] **Passo 1:** Desenhar o SVG; rodar `scripts/render-icons.sh`; conferir à vista os PNGs de 16,
  32 e 256 px (anexar os três no relatório).
- [ ] **Passo 2:** `desktop-file-validate assets/*.desktop` → sem erros.
- [ ] **Passo 3:** `appstreamcli validate --no-net assets/*.metainfo.xml` → sem erros (avisos sobre
  screenshots ausentes são aceitos até o Passo 6).
- [ ] **Passo 4:** Teste `window_icon_decodes` em `src/main.rs` (ou módulo auxiliar): o PNG embutido
  vira um `window::Icon` sem erro.
- [ ] **Passo 5:** `cargo run --features fake`: ícone aparece na barra de tarefas/alt-tab; janela não
  encolhe abaixo de 900×600.
- [ ] **Passo 6:** Humano tira 3 screenshots (lista escura, dock aberto, palette em tema claro) em
  1200×760 e salva em `docs/screenshots/`; `appstreamcli validate --no-net` sem avisos.
- [ ] **Passo 7:** Commit `feat: app identity with icon, desktop entry and metainfo`.

### Tarefa 6.3: Empacotamento e release

**Arquivos:**
- Criar: `.github/workflows/release.yml`, `packaging/appimage/` (AppDir/recipe), `Cargo.toml`
  (`[package.metadata.deb]` para `cargo-deb`, instalando o `.desktop`, os ícones `hicolor` e o
  metainfo nos caminhos padrão de `/usr/share`)
- Modificar: `README.md` (instalação, permissões do grupo `docker`, screenshots, `--features fake` para contribuir)

- [ ] **Passo 1:** `cargo deb` local → `.deb` instala com `sudo apt install ./target/debian/*.deb`;
  `doknyu` aparece no menu de apps.
- [ ] **Passo 2:** AppImage via `linuxdeploy` (com plugin para bibliotecas GPU/Wayland se necessário) →
  `./doknyu-x86_64.AppImage` abre em Ubuntu 22.04+ e Fedora atual (testar em container/VM).
- [ ] **Passo 3:** `release.yml`: em tag `v*`, build release, gerar `.deb` + AppImage + `SHA256SUMS` e
  anexar ao GitHub Release.
- [ ] **Passo 4:** `git tag v0.1.0 && git push --tags` (só com aprovação do mantenedor) → release com 3 artefatos.
- [ ] **Passo 5:** Commit `ci: release workflow for deb and AppImage`.

Flatpak, AUR e outros canais ficam na [Fase 7](fase-7-distribuicao.md).
