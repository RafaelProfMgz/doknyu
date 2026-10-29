# Fase 0 — Fundação

> Ver [README.md](README.md) para restrições globais e interfaces compartilhadas.

**Entrega:** repositório git com crate compilando, janela iced vazia abrindo, CI rodando e uma
decisão registrada sobre o `iced_term`.

---

### Tarefa 0.1: Projeto Cargo + janela vazia + CI

**Arquivos:**
- Criar: `Cargo.toml`, `src/main.rs`, `.gitignore`, `LICENSE-MIT`, `LICENSE-APACHE`, `README.md`,
  `.github/workflows/ci.yml`, `rustfmt.toml`

**Interfaces:**
- Produz: binário `doknyu` que abre uma janela com título "doknyu" e texto "Hello".

- [ ] **Passo 1:** `git init`, `cargo init --name doknyu`.
- [ ] **Passo 2:** Pesquisar no crates.io as versões estáveis mais recentes **compatíveis entre si**
  de `iced` e `iced_term` (o `iced_term` costuma fixar uma versão específica de `iced`; a versão do
  `iced` é escolhida para casar com ela). Fixar em `Cargo.toml`:
  `iced` (features `tokio`, `canvas`, `advanced`), `bollard`, `tokio` (`full`), `futures`,
  `async-trait`, `thiserror`, `anyhow`, `tracing`, `tracing-subscriber` (`env-filter`),
  `rust-i18n`, `sys-locale`, `open` (abrir URLs). Dev: `tokio` com `test-util`.
  `[features] fake = []` para rodar a UI com `FakeClient`.
- [ ] **Passo 3:** `src/main.rs` inicializa `tracing_subscriber` com `RUST_LOG` e roda um
  `iced::application` mínimo.
- [ ] **Passo 4:** Rodar `cargo run`. Esperado: abre uma janela com "Hello".
- [ ] **Passo 5:** CI `.github/workflows/ci.yml` (ubuntu-latest): instalar deps de sistema do iced
  (`libxkbcommon-dev libwayland-dev libvulkan-dev pkg-config`), `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo test -- --ignored`
  (integração; runner já tem Docker).
- [ ] **Passo 6:** `cargo fmt --check && cargo clippy --all-targets -- -D warnings`. Esperado: sem saída de erro.
- [ ] **Passo 7:** Commit `chore: scaffold doknyu crate, CI and licenses`.

### Tarefa 0.2: Spike do `iced_term` com backend customizado (descartável)

Pergunta a responder: o `iced_term` consegue exibir um processo que **não** é um PTY local
(precisamos alimentá-lo com bytes vindos do `docker exec` via bollard e mandar o teclado de volta)?

**Arquivos:**
- Criar: `examples/spike_term.rs` (será apagado), `docs/decisions/0001-terminal.md`

- [ ] **Passo 1:** Ler a documentação/exemplos do `iced_term` na versão fixada: ele aceita backend
  customizado (fonte de bytes + sink de input + resize) ou só PTY local?
- [ ] **Passo 2:** Se só PTY local: testar abrir PTY rodando `docker exec -it <id> sh` (com um
  `docker run -d --name spike alpine sleep 600`). Se aceita backend custom: ligar ao exec do bollard
  (`create_exec` com `attach_stdin/stdout/tty = true`, `start_exec` não-detached).
- [ ] **Passo 3:** `cargo run --example spike_term`. Esperado: digitar `ls /` mostra o FS do alpine;
  `top` renderiza; redimensionar a janela ajusta colunas.
- [ ] **Passo 4:** Registrar em `docs/decisions/0001-terminal.md` qual caminho a Fase 5 usa:
  **(a)** `iced_term` + exec do bollard, **(b)** `iced_term` + PTY rodando a CLI `docker exec -it`,
  ou **(c)** só terminal externo. Critério: (a) > (b) > (c); (b) exige a CLI `docker` instalada,
  então mantém-se o botão de terminal externo de qualquer forma.
- [ ] **Passo 5:** Apagar `examples/spike_term.rs`; `docker rm -f spike`. Commit
  `docs: record terminal approach decision`.
