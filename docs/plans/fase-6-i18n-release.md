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

### Tarefa 6.2: Identidade e integração com o desktop

**Arquivos:**
- Criar: `assets/doknyu.svg`, `assets/doknyu.png` (256 px), `assets/doknyu.desktop`
- Modificar: `src/main.rs` (ícone da janela, tamanho mínimo 900×600, tema claro/escuro seguindo o sistema)

- [ ] **Passo 1:** `.desktop` com `Name=doknyu`, `Comment[pt_BR]=Gerencie seus containers Docker`,
  `Categories=Development;System;`, `Exec=doknyu`, `Icon=doknyu`.
- [ ] **Passo 2:** `desktop-file-validate assets/doknyu.desktop` → sem erros.
- [ ] **Passo 3:** Commit `feat: app icon and desktop entry`.

### Tarefa 6.3: Empacotamento e release

**Arquivos:**
- Criar: `.github/workflows/release.yml`, `packaging/appimage/` (AppDir/recipe), `Cargo.toml`
  (`[package.metadata.deb]` para `cargo-deb`)
- Modificar: `README.md` (instalação, permissões do grupo `docker`, screenshots, `--features fake` para contribuir)

- [ ] **Passo 1:** `cargo deb` local → `.deb` instala com `sudo apt install ./target/debian/*.deb`;
  `doknyu` aparece no menu de apps.
- [ ] **Passo 2:** AppImage via `linuxdeploy` (com plugin para bibliotecas GPU/Wayland se necessário) →
  `./doknyu-x86_64.AppImage` abre em Ubuntu 22.04+ e Fedora atual (testar em container/VM).
- [ ] **Passo 3:** `release.yml`: em tag `v*`, build release, gerar `.deb` + AppImage + `SHA256SUMS` e
  anexar ao GitHub Release.
- [ ] **Passo 4:** `git tag v0.1.0 && git push --tags` (só com aprovação do mantenedor) → release com 3 artefatos.
- [ ] **Passo 5:** Commit `ci: release workflow for deb and AppImage`.

Flatpak fica para depois do MVP (ver spec, seção 6).
