# Fase 5 — Terminal no container

> Ver [README.md](README.md) e **`docs/decisions/0001-terminal.md`** (resultado do spike da Fase 0),
> que define qual variante abaixo implementar. A Tarefa 5.1 (terminal externo) é feita sempre.

**Entrega:** aba Terminal no detalhe do container + botão "Abrir no terminal do sistema".

## Adaptação às Fases 2.5 e 2.6 (vale sobre o texto abaixo)

- "Detalhe" é o dock inferior da Fase 2.5; `src/views/detail/` é o da Fase 3 (conteúdo das abas).
  "Sair do detalhe" = fechar o dock (`CloseDock`) ou trocar o container do dock.
- O `iced_term` usa `fonts::MONO` e cores derivadas da paleta ativa (fundo `bg`, texto `text`,
  cursor `accent`); ao trocar o tema, o terminal acompanha.
- Com o terminal focado, os atalhos de uma letra e as setas são do terminal: o `keymap` só trata
  teclas com Ctrl+Shift (palette, `DockTerminal`) e Esc **não** fecha o dock quando o terminal tem
  foco (Esc é do shell). Fechar o dock pelo teclado: Ctrl+J.
- O `App` não sabe onde está o foco e `keymap` converte Esc em `Escape` mesmo com `captured`:
  guarde `terminal_focused: bool` (atualizado ao focar/desfocar o terminal) e, nesse caso, em
  `App::on_key` deixe Esc e as teclas sem Ctrl para o shell antes do `keymap` (não confie no
  `iced_term` marcar o evento como capturado).
- Ações registradas: `DockTerminal` (Ctrl+Shift+T) e `OpenExternalTerminal` (palette).

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
    `wezterm` → `["wezterm", "start", "--"]`; `xfce4-terminal` → `["xfce4-terminal", "-x"]`
    (o `-e` dele recebe uma string única); demais → `[<bin>, "-e"]`. `$TERMINAL` pode trazer
    argumentos (`kitty --single-instance`): a primeira palavra é o programa, procurado no PATH, e
    as demais ficam antes da flag de execução.
  - Comando executado: `docker exec -it -e TERM=xterm-256color <id> sh -c 'command -v bash >/dev/null && exec bash || exec sh'`.
  - Produzir também `pub fn exec_args(container_id: &str) -> Vec<String>` (args após `docker`,
    reutilizados pela 5.2) e `pub fn docker_cli_available() -> bool`.
  - Nenhum terminal ou CLI `docker` ausente → `Err(DockerError::Other(..))` → toast explicando.
- Botão no cabeçalho do detalhe (só com container `Running`).

- [ ] **Passo 1: Testes que falham:** `env_terminal_wins`;
  `gnome_terminal_uses_double_dash`; `falls_back_to_xterm`; `none_available_returns_none`.
- [ ] **Passo 2–4:** FAIL → implementar → PASS.
- [ ] **Passo 5:** Manual: botão abre o terminal do sistema dentro do container.
- [ ] **Passo 6:** Commit `feat: open container shell in system terminal`.

### Tarefa 5.2: Aba Terminal embutida (variante b — PTY local, ver ADR 0001)

**Arquivos:**
- Criar: `src/views/detail/terminal.rs`
- Modificar: `src/views/detail/mod.rs`, `src/app.rs`, `locales/*.yml`
- **Não** modificar `src/docker/*`: a trait `DockerClient` não muda.

**Interfaces:**
- Consome: `terminal_launcher::{exec_args, docker_cli_available}` (Tarefa 5.1).
- Produz:
  ```rust
  pub fn backend_settings(container_id: &str) -> iced_term::settings::BackendSettings;
  // program "docker", args = exec_args(id), env TERM=xterm-256color e COLORTERM=truecolor
  pub struct TerminalState { pub session: Option<iced_term::Terminal>, pub active_id: Option<u64>,
                            pub next_id: u64, pub ended: bool }
  impl TerminalState { pub fn close(&mut self); } // zera session e active_id juntos
  ```
  - A sessão é criada ao abrir a aba Terminal pela primeira vez; `id` = `next_id` incremental.
  - **A subscription do terminal fica ativa enquanto `session.is_some()`**, mesmo com outra aba
    visível (pausá-la com a sessão viva trava o app; ver ADR). `session` vira `None` no mesmo
    `update` que encerra a subscription: sair do detalhe, trocar de container ou `Shutdown`.
  - `Shutdown` (o usuário digitou `exit`, ou o container parou) → `session = None`, `ended = true`,
    mensagem "Sessão encerrada" + botão "Nova sessão".
  - Container parado → "Inicie o container para abrir um terminal".
  - CLI `docker` ausente → aviso + só o botão do terminal externo.
  - Testes unitários **não** constroem `Terminal` real (isso abre um PTY e roda `docker`); eles
    verificam `backend_settings` e as transições de `TerminalState` via `active_id`
    (invariante: `active_id.is_some() == session.is_some()` fora dos testes).

- [ ] **Passo 1: Testes que falham:**
  - `backend_settings_runs_docker_exec`: program `"docker"`; args começam com
    `["exec", "-it", "-e", "TERM=xterm-256color", "<id>", "sh", "-c"]`; env contém `TERM=xterm-256color`.
  - `exec_closed_on_leave_detail`: com `active_id = Some(1)`, `Navigate(Containers)` → `active_id.is_none()` e `session.is_none()`.
  - `shutdown_marks_ended`.
  - `stopped_container_shows_hint`.
  - `docker_cli_missing_shows_hint`.
- [ ] **Passo 2–4:** FAIL → implementar → PASS.
- [ ] **Passo 5:** Manual: aba Terminal → `ls`, `top`, `vi`, setas e Ctrl+C funcionam; `echo $TERM`
  mostra `xterm-256color` (abrindo o app pelo terminal **e** pelo lançador); com `top` rodando,
  trocar para Logs por 30 s e voltar → app responsivo; redimensionar a janela ajusta `stty size`;
  sair do detalhe → `docker top <id>` sem o shell do exec.
- [ ] **Passo 6:** Commit `feat(ui): embedded container terminal tab`.
