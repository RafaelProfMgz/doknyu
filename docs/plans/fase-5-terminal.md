# Fase 5 — Terminal no container

> Ver [README.md](README.md) e **`docs/decisions/0001-terminal.md`** (resultado do spike da Fase 0),
> que define qual variante abaixo implementar. A Tarefa 5.1 (terminal externo) é feita sempre.

**Entrega:** aba Terminal no detalhe do container + botão "Abrir no terminal do sistema".

---

### Tarefa 5.1: Terminal externo (sempre)

**Arquivos:**
- Criar: `src/terminal_launcher.rs`
- Modificar: `src/views/detail/mod.rs`

**Interfaces:**
- Produz:
  ```rust
  pub fn detect_terminal(env_terminal: Option<&str>, available: &dyn Fn(&str) -> bool) -> Option<Vec<String>>;
  pub fn launch(container_id: &str) -> Result<(), DockerError>;
  ```
  - Ordem de detecção: `$TERMINAL`; `x-terminal-emulator`; `gnome-terminal`; `konsole`; `kitty`;
    `alacritty`; `wezterm`; `foot`; `xfce4-terminal`; `xterm`.
  - Argumentos de execução por terminal: `gnome-terminal` → `["gnome-terminal", "--"]`;
    `wezterm` → `["wezterm", "start", "--"]`; demais → `[<bin>, "-e"]`.
  - Comando executado: `docker exec -it <id> sh -c 'command -v bash >/dev/null && exec bash || exec sh'`.
  - Nenhum terminal ou CLI `docker` ausente → `Err(DockerError::Other(..))` → toast explicando.
- Botão no cabeçalho do detalhe (só com container `Running`).

- [ ] **Passo 1: Testes que falham:** `env_terminal_wins`;
  `gnome_terminal_uses_double_dash`; `falls_back_to_xterm`; `none_available_returns_none`.
- [ ] **Passo 2–4:** FAIL → implementar → PASS.
- [ ] **Passo 5:** Manual: botão abre o terminal do sistema dentro do container.
- [ ] **Passo 6:** Commit `feat: open container shell in system terminal`.

### Tarefa 5.2: Aba Terminal embutida (variante decidida no spike)

**Arquivos:**
- Criar: `src/views/detail/terminal.rs`
- Modificar: `src/docker/mod.rs`, `src/docker/bollard.rs`, `src/docker/fake.rs`, `src/views/detail/mod.rs`

**Interfaces (variante a — exec do bollard):**
  ```rust
  pub struct ExecSession {
      pub output: DockerStream<Vec<u8>>,
      pub input: tokio::sync::mpsc::Sender<Vec<u8>>,
      pub exec_id: String,
  }
  // adicionar à trait DockerClient:
  async fn exec_shell(&self, id: &str, cols: u16, rows: u16) -> Result<ExecSession, DockerError>;
  async fn resize_exec(&self, exec_id: &str, cols: u16, rows: u16) -> Result<(), DockerError>;
  ```
  - `exec_shell` executa `["sh", "-c", "command -v bash >/dev/null && exec bash || exec sh"]` com
    `tty: true`, `attach_stdin/stdout/stderr: true`, env `TERM=xterm-256color`.
  - Fake: ecoa o input de volta no output.

**Variante b — PTY local:** sem mudança na trait; `terminal.rs` cria a sessão do `iced_term` rodando
`docker exec -it <id> sh -c '...'` (mesmo comando da 5.1).

**Comportamento comum:**
  - A sessão é criada ao abrir a aba (não ao abrir o detalhe) e encerrada ao sair do detalhe ou ao
    trocar de container.
  - Fim do shell (`exit`) → mensagem "Sessão encerrada" + botão "Nova sessão".
  - Container parado → aba mostra "Inicie o container para abrir um terminal".

- [ ] **Passo 1: Testes que falham:** (a) `fake_exec_echoes_input`; `exec_closed_on_leave_detail`;
  integração `#[ignore]` `exec_runs_command` (`echo doknyu\n` → output contém `doknyu`).
  (b) `exec_closed_on_leave_detail`; `stopped_container_shows_hint`.
- [ ] **Passo 2–4:** FAIL → implementar → PASS (`cargo test` e `cargo test -- --ignored`).
- [ ] **Passo 5:** Manual: aba Terminal → `ls`, `top`, `vi`, setas e Ctrl+C funcionam;
  redimensionar a janela ajusta colunas (`tput cols`).
- [ ] **Passo 6:** Commit `feat(ui): embedded container terminal tab`.

**Se o spike decidiu (c):** pular a Tarefa 5.2; a aba Terminal mostra o botão da 5.1 com explicação.
