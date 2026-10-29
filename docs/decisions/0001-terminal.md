# 0001 — Abordagem do terminal embutido

Data: 2026-10-29. Status: aceita. Origem: Tarefa 0.2 (spike descartável).

## Decisão

**(b) `iced_term` 0.8 + PTY local rodando `docker exec -it <id> sh -c '...'`.**
A variante (a) (backend customizado alimentado pelo exec do bollard) **não é viável** com
`iced_term` 0.8.0. O botão de terminal externo (Tarefa 5.1) continua sendo feito sempre, pois (b)
exige a CLI `docker` instalada.

## Evidência (código-fonte de `iced_term-0.8.0` e `alacritty_terminal-0.25.1`)

- `iced_term::settings::BackendSettings` só tem `program`, `args`, `env`, `working_directory`
  (`src/settings.rs`). Não há trait/callback/canal para injetar uma fonte de bytes.
- `iced_term::backend::Backend::new` (`src/backend.rs`, ~l.143-190) cria sempre
  `alacritty_terminal::tty::new(&tty::Options{ shell: Some(Shell::new(program,args)), .. })`,
  `EventLoop::new(term, event_proxy, pty, ..)` e `Notifier(pty_event_loop.channel())`.
  `Terminal::new(id, Settings)` chama isso diretamente; não existe construtor alternativo.
- Os campos `Terminal.backend` e `Backend.{term,notifier,..}` são `pub(crate)`/privados; a única
  entrada de bytes é `backend::Command::Write(Vec<u8>)`, que vai para o `Notifier` (escreve no
  PTY), e o redimensionamento é `Command::Resize`, que chama `notifier.on_resize` (TIOCSWINSZ no PTY).
  A saída só entra pelo `EventLoop` lendo o fd do PTY. Logo não há como empurrar bytes do bollard
  para o emulador (`Term`) sem forkar a crate.
- Consequência: o único "backend" possível é um processo filho com PTY local. Em (b) o resize
  chega ao container porque a CLI `docker exec -it` propaga SIGWINCH/tamanho do PTY para o exec.

## Verificação feita (automática)

- `cargo build --example spike_term` compila (iced 0.14 + iced_term 0.8).
- `timeout 8 cargo run --example spike_term` com container `alpine sleep 600` rodando: sem
  falha antes do timeout (exit 124). Não foi possível ver a janela nem digitar (agente sem GUI).

## Verificação manual pendente (humano)

1. `docker run -d --name doknyu-spike-term alpine sleep 600`
2. Recriar `examples/spike_term.rs` com o código abaixo; `cargo run --example spike_term`.
3. Digitar `ls /` (deve listar o FS do alpine), `top` (deve renderizar; `q` sai), Ctrl+C,
   setas, e redimensionar a janela e conferir `tput cols` / `stty size` (muda com a janela).
4. `exit` deve fechar a janela (`Action::Shutdown`). Limpeza: `docker rm -f doknyu-spike-term`.
5. Se algum item falhar (principalmente resize ou `top`), reavaliar (c) para a Fase 5.

Código mínimo do exemplo:

```rust
use iced::{Element, Length, Subscription, Task, window};
use iced_term::{Command, Event as TermEvent, Terminal, TerminalView, actions::Action, settings};

#[derive(Debug, Clone)]
enum Msg {
    Term(TermEvent),
}

struct App {
    term: Terminal,
}

fn new() -> (App, Task<Msg>) {
    let container = std::env::var("SPIKE_CONTAINER").unwrap_or_else(|_| "doknyu-spike-term".into());
    let backend = settings::BackendSettings {
        program: "docker".into(),
        args: vec![
            "exec".into(),
            "-it".into(),
            container,
            "sh".into(),
            "-c".into(),
            "command -v bash >/dev/null && exec bash || exec sh".into(),
        ],
        ..Default::default()
    };
    let settings = settings::Settings { backend, ..Default::default() };
    (App { term: Terminal::new(0, settings).expect("pty") }, Task::none())
}

fn update(app: &mut App, msg: Msg) -> Task<Msg> {
    let Msg::Term(TermEvent::BackendCall(_, cmd)) = msg;
    if let Action::Shutdown = app.term.handle(Command::ProxyToBackend(cmd)) {
        return window::latest().and_then(window::close);
    }
    Task::none()
}

fn view(app: &App) -> Element<'_, Msg> {
    iced::widget::container(TerminalView::show(&app.term).map(Msg::Term))
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn subscription(app: &App) -> Subscription<Msg> {
    app.term.subscription().map(Msg::Term)
}

fn main() -> iced::Result {
    iced::application(new, update, view).subscription(subscription).run()
}
```

## Riscos

- **CLI `docker` obrigatória** no PATH do usuário (e acesso ao socket, `DOCKER_HOST`/contexto da
  CLI podem diferir do que o bollard usa). Se ausente: aba mostra aviso e só o botão externo.
- **API instável**: o README do `iced_term` avisa que não promete API estável; fixar `=0.8.x` ou
  `~0.8` e revisar a cada bump do iced.
- O stream da subscription do `iced_term` dá `panic!` se o canal de eventos fechar fora do
  `Exit` (`Terminal::new` não dá). Tratar fim de sessão via `Action::Shutdown` e não reusar o
  mesmo `id` de `Terminal` ao criar "Nova sessão" (usar `id` incremental; o `id` é o hash da
  subscription). Após o `Exit`, remover o `Terminal` **e** a subscription no mesmo `update`:
  subscription ativa sem `Terminal` faz `recv()` devolver `None` em loop (CPU 100%).
- **Deadlock se a subscription for pausada com a sessão viva:** `EventProxy::send_event` usa
  `blocking_send` num canal de 100 (`iced_term` `backend.rs:583`) e o `pty_read` do alacritty
  envia `Wakeup` segurando o lock do `Term`. Se a subscription parar (ex.: usuário troca para a
  aba Logs com `top` rodando), o canal enche, a thread do PTY bloqueia com o lock e o próximo
  `view` trava o app.
- `Shutdown` ocorre quando o PTY filho sai: cobre `exit`, mas também container parado/removido
  (a CLI sai com código != 0); a UI deve mostrar "Sessão encerrada" nos dois casos.
- `TERM`: o `iced_term` não chama `alacritty_terminal::tty::setup_env` e a CLI `docker` não
  repassa `TERM` ao container. Aberto pelo `.desktop`, o app tem `TERM` vazio/`dumb`. Portanto
  passar `env: {TERM: xterm-256color, COLORTERM: truecolor}` em `BackendSettings` **e**
  `-e TERM=xterm-256color` nos args do `docker exec` (vale também para a Tarefa 5.1).
- Shell padrão do `BackendSettings` é `/bin/bash`; sempre sobrescrever `program`.
- Dependência pesada: `alacritty_terminal` + `vte` aumentam compilação e binário.

## O que a Fase 5 deve ajustar

- Tarefa 5.2: seguir somente a **variante b**; remover/ignorar a variante a (`ExecSession`,
  `exec_shell`, `resize_exec`, testes `fake_exec_echoes_input` e `exec_runs_command`). A trait
  `DockerClient` não muda.
- Testes da variante b: `exec_closed_on_leave_detail`, `stopped_container_shows_hint`, e
  acrescentar um teste puro da função que monta `BackendSettings` (program `docker`, args
  `exec -it <id> sh -c '<cmd>'`) e `docker_cli_missing_shows_hint`.
- Reutilizar na 5.2 o comando e a detecção de CLI da 5.1 (`docker` no PATH).
- Gerenciar ids de `Terminal` (contador). A subscription (`term.subscription()`) fica ativa
  **enquanto o `Terminal` existir**, independente da aba visível; o `Terminal` é removido no mesmo
  `update` que deixa de produzir a subscription (sair do detalhe, trocar de container, `Exit`).
- Opção futura para eliminar a dependência da CLI: o próprio binário `doknyu` com um modo
  `--exec-bridge <id>` (bollard exec em tty, repassa stdin/stdout e converte SIGWINCH em
  `resize_exec`), usado como `program` do PTY. Não é necessário agora.
- `iced_term` permanece no `Cargo.toml` (`iced_term = "0.8"`).

## Fontes

- `~/.cargo/registry/src/*/iced_term-0.8.0/{README.md,src/settings.rs,src/backend.rs,src/terminal.rs}`
- `~/.cargo/registry/src/*/alacritty_terminal-0.25.1/src/tty/{mod.rs,unix.rs}`
- crates.io: iced_term 0.8.0 (depende de `iced ^0.14.0`).
