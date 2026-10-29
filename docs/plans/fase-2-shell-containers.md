# Fase 2 — Shell do app, onboarding e Containers

> Ver [README.md](README.md). Consome tudo da Fase 1.

**Entrega:** app abre, detecta o estado do Docker (onboarding se houver problema), mostra a lista
de containers com ações e atualiza em tempo real via eventos.

Testes de `update` usam `FakeClient` e executam a `Task` retornada até concluir. Se a versão do
iced não expuser como rodar uma `Task` em teste, extrair a lógica para funções `async fn` puras
chamadas pela `Task` e testar essas funções + o `update` com as mensagens de resultado.

---

### Tarefa 2.1: Esqueleto do app + i18n + sidebar

**Arquivos:**
- Criar: `src/app.rs`, `src/views/mod.rs`, `src/views/sidebar.rs`, `src/views/widgets.rs`,
  `locales/en.yml`, `locales/pt-BR.yml`
- Modificar: `src/main.rs`, `src/lib.rs`

**Interfaces:**
- Produz:
  ```rust
  pub enum Screen { Containers, Volumes, Networks, Detail(String /*container id*/) }
  pub enum Connection { Checking, Ok, Failed(DockerError) }
  pub struct App { client: Arc<dyn DockerClient>, conn: Connection, screen: Screen,
                   containers: ContainersState, toasts: Vec<Toast>, /* demais estados nas fases seguintes */ }
  pub enum Message { Navigate(Screen), Pinged(Result<(), DockerError>), Retry,
                     DismissToast(usize), Containers(containers::Msg), Docker(DockerEvent), EventsEnded, Tick, /* ... */ }
  pub struct Toast { pub text: String, pub details: Option<String> }
  impl App { pub fn new(client: Arc<dyn DockerClient>) -> (Self, Task<Message>);
             pub fn update(&mut self, m: Message) -> Task<Message>;
             pub fn view(&self) -> Element<'_, Message>;
             pub fn subscription(&self) -> Subscription<Message>; }
  ```
  - `widgets.rs`: `status_badge(&ContainerState) -> Element`, `confirm_dialog(...)`, `toast_stack(...)`.
  - `pub fn friendly_error(e: &DockerError) -> String` em `src/views/widgets.rs` (usa `t!`).
- `main.rs`: cria o client e passa para `App::new` — `docker::connect()` por padrão; com a feature
  `fake` e `DOKNYU_FAKE=1`, `FakeClient::with_sample_data()`. Erro de `connect()` não dá panic: o app
  abre e o onboarding mostra o erro (ver Tarefa 2.2).
- i18n: `rust_i18n::i18n!("locales", fallback = "en")`; locale via `sys_locale::get_locale()`,
  normalizando `pt_BR`/`pt-BR`/`pt` → `pt-BR`.

- [ ] **Passo 1: Testes que falham:** `navigate_changes_screen`;
  `friendly_error_conflict_is_human` (não contém "409");
  `locale_normalization` (`"pt_BR.UTF-8"` → `"pt-BR"`, `"en_US"` → `"en"`, `"fr_FR"` → `"en"`).
- [ ] **Passo 2:** `cargo test` → FAIL.
- [ ] **Passo 3:** Implementar; sidebar com Containers / Volumes / Redes (Volumes e Redes mostram
  "em breve" até a Fase 4).
- [ ] **Passo 4:** `cargo test` → PASS; `cargo run --features fake` mostra a sidebar e navega.
- [ ] **Passo 5:** Commit `feat(ui): app skeleton with sidebar, toasts and i18n`.

### Tarefa 2.2: Onboarding (estado da conexão)

**Arquivos:**
- Criar: `src/views/onboarding.rs`
- Modificar: `src/app.rs`

**Interfaces:**
- Produz: `pub fn view(err: &DockerError) -> Element<'_, Message>`, com textos da spec:
  - `NotInstalled`: explicação + botão que abre `https://docs.docker.com/engine/install/`.
  - `NotRunning`: bloco copiável `sudo systemctl start docker` + botão "Tentar novamente".
  - `PermissionDenied`: explicação do grupo `docker`, bloco copiável
    `sudo usermod -aG docker $USER`, aviso "saia e entre na sessão de novo".
- Comportamento: `App::new` dispara `ping`. `Pinged(Err(e))` com `e.is_connection()` →
  `Connection::Failed(e)`; enquanto `Failed`, uma `Subscription` de tick a cada **3 s** dispara
  `Retry` → novo ping. `Pinged(Ok)` → `Connection::Ok` e carrega containers.

- [ ] **Passo 1: Testes que falham:** `ping_failure_shows_onboarding` (Fake com
  `set_ping_error(Some(NotRunning))` → `conn == Failed(NotRunning)`);
  `retry_recovers` (limpar erro, `Retry` → `Ok` e `list_containers` chamado).
- [ ] **Passo 2–4:** FAIL → implementar → PASS.
- [ ] **Passo 5:** Verificação manual: `sudo systemctl stop docker docker.socket`, `cargo run` → tela
  NotRunning; iniciar o Docker → app entra sozinho em ≤ 3 s.
- [ ] **Passo 6:** Commit `feat(ui): onboarding screens for docker connection problems`.

### Tarefa 2.3: Lista de containers + ações

**Arquivos:**
- Criar: `src/views/containers.rs`
- Modificar: `src/app.rs`

**Interfaces:**
- Produz:
  ```rust
  pub struct ContainersState { pub items: Vec<ContainerInfo>, pub query: String, pub show_stopped: bool,
                               pub pending: HashSet<String>, pub confirm_remove: Option<String> }
  pub enum Msg { Loaded(Result<Vec<ContainerInfo>, DockerError>), Query(String), ToggleStopped(bool),
                 Start(String), Stop(String), Restart(String),
                 AskRemove(String), ConfirmRemove { force: bool }, CancelRemove,
                 ActionDone(String, Result<(), DockerError>), OpenPort(u16), Open(String) }
  impl ContainersState { pub fn visible(&self) -> Vec<&ContainerInfo> }
  ```
  - `visible`: filtra por `query` (case-insensitive, nome **ou** imagem) e por `show_stopped`;
    ordena running primeiro, depois por nome.
  - Enquanto uma ação está em `pending`, os botões da linha ficam desabilitados com spinner/texto.
  - Remover running: o diálogo oferece "Forçar remoção"; remover parado: confirmação simples.
  - `ActionDone(_, Err(e))` → toast `friendly_error(&e)` com detalhes `format!("{e:?}")`.
  - `OpenPort(p)` → `open::that(format!("http://localhost:{p}"))`.
  - `Open(id)` → `Navigate(Screen::Detail(id))`.
  - `show_stopped` começa `true`.

- [ ] **Passo 1: Testes que falham:**
  - `visible_filters_by_name_or_image` (query `"POSTGRES"` acha `db` cuja imagem é `postgres:16`).
  - `visible_hides_stopped_when_toggled_off`.
  - `visible_sorts_running_first`.
  - `stop_calls_client_and_marks_pending` (`calls()` contém `"stop:<id>"`; `pending` contém id até `ActionDone`).
  - `remove_running_requires_force_dialog` (`AskRemove` → `confirm_remove == Some(id)`; `ConfirmRemove{force:true}` → `"remove:<id>:force"`).
  - `action_error_creates_toast` (`fail_next(Conflict)` → 1 toast).
  - `empty_list_shows_empty_state` (view não panica com 0 containers; texto `t!("containers.empty")`).
- [ ] **Passo 2–4:** FAIL → implementar → PASS.
- [ ] **Passo 5:** `cargo run --features fake`: busca, filtro, ações e diálogo funcionam.
- [ ] **Passo 6:** Commit `feat(ui): container list with search, filters and actions`.

### Tarefa 2.4: Tempo real via eventos + reconexão

**Arquivos:**
- Modificar: `src/app.rs`

**Interfaces:**
- Consome: `DockerClient::events()`.
- Produz: `Subscription` (ativa só com `Connection::Ok`) que converte o stream em
  `Message::Docker(ev)`; fim/erro do stream → `Message::EventsEnded`.
  - A lista **não** recarrega após cada ação: quem atualiza é o evento (o daemon e o `FakeClient`
    emitem eventos nas ações), evitando recarga dupla.
  - `Docker(ev)` com `kind == Container` → recarrega containers, com **debounce de 300 ms**
    (vários eventos seguidos = 1 recarga).
  - `EventsEnded` → novo `ping`; se falhar com erro de conexão → onboarding (reconexão da Tarefa 2.2).

- [ ] **Passo 1: Testes que falham:** `container_event_triggers_reload`;
  `burst_of_events_reloads_once` (5 eventos em < 300 ms → 1 `list_containers`, usando
  `tokio::time::pause`); `events_ended_with_daemon_down_goes_to_onboarding`.
- [ ] **Passo 2–4:** FAIL → implementar → PASS.
- [ ] **Passo 5:** Manual: com o app aberto, `docker run -d --name x alpine sleep 100` no terminal →
  aparece sem clicar; `sudo systemctl restart docker` → onboarding e volta sozinho.
- [ ] **Passo 6:** Commit `feat(ui): live updates from docker events with reconnection`.
