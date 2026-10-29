# Fase 1 — Core Docker

> Ver [README.md](README.md) para restrições globais e interfaces compartilhadas (as assinaturas
> de `model`, `error` e `DockerClient` estão lá e são a fonte da verdade).

**Entrega:** camada core completa e testada, sem UI. `cargo test` passa sem Docker;
`cargo test -- --ignored` passa contra Docker real.

---

### Tarefa 1.1: `model.rs` + `error.rs` + classificação de erros

**Arquivos:**
- Criar: `src/model.rs`, `src/error.rs`, `src/lib.rs` (expor módulos para `tests/`)
- Modificar: `src/main.rs` (usar `doknyu::...`)

**Interfaces:**
- Produz: tipos do README; além de
  - `impl ContainerState { pub fn is_running(&self) -> bool; pub fn is_error(&self) -> bool }`
    (`is_error` = `Dead` ou `Exited { code != 0 }`)
  - `pub fn classify_connect_error(kind: std::io::ErrorKind, socket_exists: bool, cli_installed: bool) -> DockerError`
  - `impl DockerError { pub fn is_connection(&self) -> bool }` (true para NotInstalled/NotRunning/PermissionDenied)

- [ ] **Passo 1: Testes que falham** (em `src/error.rs`, `#[cfg(test)]`):
  ```rust
  use std::io::ErrorKind::*;
  assert_eq!(classify_connect_error(PermissionDenied, true, true), DockerError::PermissionDenied);
  assert_eq!(classify_connect_error(NotFound, false, false), DockerError::NotInstalled);
  assert_eq!(classify_connect_error(NotFound, false, true), DockerError::NotRunning);
  assert_eq!(classify_connect_error(ConnectionRefused, true, true), DockerError::NotRunning);
  assert!(matches!(classify_connect_error(TimedOut, true, true), DockerError::Other(_)));
  assert!(ContainerState::Exited { code: 137 }.is_error());
  assert!(!ContainerState::Exited { code: 0 }.is_error());
  ```
- [ ] **Passo 2:** `cargo test` → FAIL (não compila).
- [ ] **Passo 3:** Implementar os tipos e funções.
- [ ] **Passo 4:** `cargo test` → PASS.
- [ ] **Passo 5:** Commit `feat(core): add domain model and error classification`.

### Tarefa 1.2: Conversões bollard → model (`convert.rs`)

**Arquivos:**
- Criar: `src/docker/mod.rs` (só `pub mod convert;` por enquanto), `src/docker/convert.rs`,
  `tests/fixtures/*.json` (respostas reais capturadas com `curl --unix-socket /var/run/docker.sock`)

**Interfaces:**
- Produz (funções puras):
  - `pub fn container_from_summary(s: &bollard::models::ContainerSummary) -> ContainerInfo`
  - `pub fn raw_stats_from(s: &bollard::container::Stats) -> RawStats` (ajustar ao tipo da versão fixada)
  - `pub fn volume_from(v: &bollard::models::Volume, used_by: Vec<String>) -> VolumeInfo`
  - `pub fn network_from(n: &bollard::models::Network) -> NetworkInfo`
  - `pub fn log_line_from(o: bollard::container::LogOutput, timestamps: bool) -> LogLine`
  - `pub fn event_from(m: &bollard::models::EventMessage) -> Option<DockerEvent>`
  - `pub fn api_error_from(e: bollard::errors::Error) -> DockerError`
    (404 → `NotFound`, 409 → `Conflict`, demais status → `Api`, erros de IO → `classify_connect_error`
    com `socket_exists`/`cli_installed` verificados de verdade)

- [ ] **Passo 1: Testes que falham** usando fixtures desserializadas:
  - `container_name_strips_leading_slash`: `names: ["/web"]` → `name == "web"`.
  - `container_without_name_uses_short_id`: `names: None`, id `"abcdef1234567890"` → `name == "abcdef123456"`.
  - `container_exited_code_parsed`: `state: "exited"`, `status: "Exited (137) 2 minutes ago"` →
    `ContainerState::Exited { code: 137 }`.
  - `ports_dedup_ipv4_ipv6`: duas entradas iguais (0.0.0.0 e ::) para 8080→80 viram uma só
    `PortMapping { private: 80, public: Some(8080), protocol: "tcp" }`.
  - `network_subnet_from_ipam`: primeiro `config.subnet` ou `None`.
  - `event_ignores_unknown_types`: tipo `"image"` → `None`; `"container"`/`"start"` → `Some(...)`.
  - `api_error_404_is_not_found`, `api_error_409_is_conflict`.
- [ ] **Passo 2:** `cargo test convert` → FAIL.
- [ ] **Passo 3:** Implementar.
- [ ] **Passo 4:** `cargo test convert` → PASS.
- [ ] **Passo 5:** Commit `feat(core): convert bollard types into domain model`.

### Tarefa 1.3: Cálculo de stats (`stats.rs`)

**Arquivos:**
- Criar: `src/stats.rs`

**Interfaces:**
- Produz:
  - `pub fn sample_from_raw(raw: &RawStats) -> StatsSample`
    — CPU% = `(cpu_total - precpu_total) / (system - presystem) * online_cpus * 100`;
    mem_used = `mem_usage - mem_inactive_file` (saturating).
  - `pub struct StatsHistory { /* VecDeque<StatsSample>, capacidade 60 */ }` com
    `pub fn new() -> Self`, `pub fn push(&mut self, s: StatsSample)`,
    `pub fn latest(&self) -> Option<&StatsSample>`, `pub fn cpu_series(&self) -> Vec<f64>`,
    `pub fn mem_series(&self) -> Vec<f64>` (em bytes).

- [ ] **Passo 1: Testes que falham:**
  - `cpu_percent_basic`: cpu 200→400 (delta 200), system 1000→2000 (delta 1000), 4 CPUs → `80.0`.
  - `cpu_zero_when_system_delta_zero`: `system == presystem` → `0.0` (não NaN).
  - `cpu_zero_on_first_sample`: `precpu_total = 0, presystem = 0` → `0.0`.
  - `cpu_zero_when_counters_go_backwards`: `cpu_total < precpu_total` → `0.0`.
  - `mem_excludes_inactive_file`: usage 500, inactive 100 → `mem_used == 400`; inactive > usage → `0`.
  - `history_caps_at_60`: 61 pushes → `cpu_series().len() == 60` e o primeiro valor descartado.
- [ ] **Passo 2:** `cargo test stats` → FAIL.
- [ ] **Passo 3:** Implementar.
- [ ] **Passo 4:** `cargo test stats` → PASS.
- [ ] **Passo 5:** Commit `feat(core): compute cpu/memory stats with 60s history`.

### Tarefa 1.4: Trait `DockerClient` + `BollardClient`

**Arquivos:**
- Modificar: `src/docker/mod.rs` (trait + `DockerStream` conforme README; `pub fn connect() -> Arc<dyn DockerClient>`)
- Criar: `src/docker/bollard.rs`, `tests/docker_integration.rs`

**Interfaces:**
- Consome: `convert::*`, `classify_connect_error`.
- Produz: `pub struct BollardClient` implementando `DockerClient`;
  `pub fn connect() -> Arc<dyn DockerClient>` — usa `DOCKER_HOST` se
  `unix://...`, senão `/var/run/docker.sock`; **não** falha se o daemon estiver parado (o erro
  aparece no primeiro `ping`).
- Notas de implementação:
  - `logs`: `follow: true, stdout: true, stderr: true, timestamps: true, tail: tail.to_string()`.
  - `stats`: `stream: true`, mapeado com `convert::raw_stats_from`.
  - `events`: filtro `type=container,volume,network`.
  - `list_volumes`: preencher `used_by` com nomes dos containers (`list_containers(all=true)`) cujos
    `mounts` referenciam o volume.
  - `stop`: timeout de 10 s.
  - `prune_volumes`: retorna `volumes_deleted`.

- [ ] **Passo 1: Testes de integração que falham** (`#[ignore]`, `#[tokio::test]`), cada um com
  container `alpine` de nome único (`doknyu-it-<uuid curto>`) removido no fim mesmo em falha:
  - `lifecycle`: criar (via bollard direto no teste) `alpine sh -c 'echo hello; sleep 300'` →
    `start` → aparece em `list_containers(false)` com `Running` → `logs(id, 10)` traz linha
    `"hello"` em até 5 s → `stats` entrega 2 amostras → `stop` → `Exited` → `remove(id, false)` →
    sumiu de `list_containers(true)`.
  - `remove_missing_is_not_found`: `remove("nao-existe", false)` → `Err(DockerError::NotFound(_))`.
  - `events_see_start`: assinar `events()`, dar `start` num container → recebe
    `DockerEvent { kind: Container, action: "start", .. }` em até 5 s.
  - `volume_in_use_conflict`: volume montado em container → `remove_volume` →
    `Err(DockerError::Conflict(_))`.
- [ ] **Passo 2:** `cargo test -- --ignored` → FAIL.
- [ ] **Passo 3:** Implementar.
- [ ] **Passo 4:** `cargo test -- --ignored` → PASS (com Docker rodando).
- [ ] **Passo 5:** Commit `feat(core): implement DockerClient over bollard`.

### Tarefa 1.5: `FakeClient`

**Arquivos:**
- Criar: `src/docker/fake.rs` (compilado em `cfg(any(test, feature = "fake"))`)

**Interfaces:**
- Produz:
  - `pub struct FakeClient` com estado em `Mutex` e `pub fn new() -> Self` (vazio),
    `pub fn with_sample_data() -> Self` (3 containers: `web` Running com porta 8080→80,
    `db` Running, `worker` Exited 1; 2 volumes, 1 em uso; redes `bridge`, `host`, `none`, `app_net`).
  - Controles de teste: `pub fn set_ping_error(&self, e: Option<DockerError>)`,
    `pub fn fail_next(&self, e: DockerError)`, `pub fn calls(&self) -> Vec<String>`
    (ex.: `"stop:web-id"`), `pub fn push_event(&self, e: DockerEvent)`,
    `pub fn push_log(&self, id: &str, line: LogLine)`, `pub fn close_streams(&self, id: &str)`.
  - Ações mutam o estado (stop → `Exited { code: 0 }`, remove → some), respeitando as mesmas regras
    de erro do real (remover running sem force → `Conflict`; remover volume em uso → `Conflict`).
- `main.rs`: com `--features fake` ou env `DOKNYU_FAKE=1` (só se compilado com a feature), usa
  `FakeClient::with_sample_data()`.

- [ ] **Passo 1: Testes que falham:** `stop_changes_state`, `remove_running_without_force_conflicts`,
  `remove_in_use_volume_conflicts`, `fail_next_returns_error_once`, `logs_stream_ends_on_close`.
- [ ] **Passo 2:** `cargo test fake` → FAIL.
- [ ] **Passo 3:** Implementar.
- [ ] **Passo 4:** `cargo test` → PASS (todos).
- [ ] **Passo 5:** Commit `feat(core): add FakeClient for tests and offline UI`.
