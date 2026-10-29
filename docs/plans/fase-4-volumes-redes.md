# Fase 4 — Volumes e Redes

> Ver [README.md](README.md). Consome `DockerClient::{list,remove,prune}_volumes`, `list_networks`,
> `remove_network`, `confirm_dialog`, `friendly_error` e a Subscription de eventos da Fase 2.

**Entrega:** telas Volumes e Redes completas, substituindo o "em breve".

---

### Tarefa 4.1: Tela Volumes

**Arquivos:**
- Criar: `src/views/volumes.rs`
- Modificar: `src/app.rs`, `locales/*.yml`

**Interfaces:**
- Produz:
  ```rust
  pub struct VolumesState { pub items: Vec<VolumeInfo>, pub query: String,
                            pub confirm: Option<VolumeConfirm>, pub pending: HashSet<String> }
  pub enum VolumeConfirm { Remove(String), Prune(Vec<String> /* nomes sem uso, mostrados no diálogo */) }
  pub enum Msg { Loaded(Result<Vec<VolumeInfo>, DockerError>), Query(String),
                 AskRemove(String), AskPrune, Confirm, Cancel,
                 Removed(String, Result<(), DockerError>), Pruned(Result<Vec<String>, DockerError>) }
  ```
  - Colunas: nome, driver, mountpoint (truncado com tooltip), "usado por" (nomes ou "—").
  - Botão remover **desabilitado** quando `used_by` não está vazio, com tooltip
    "Em uso por: web, db".
  - `AskPrune` lista no diálogo os volumes com `used_by` vazio; lista vazia → botão "Limpar" desabilitado.
  - `Pruned(Ok(nomes))` → toast "N volumes removidos".
  - Evento `kind == Volume` recarrega (mesmo debounce de 300 ms).

- [ ] **Passo 1: Testes que falham:**
  - `in_use_volume_cannot_be_removed` (`AskRemove` em volume usado não abre diálogo).
  - `remove_conflict_keeps_item_and_toasts` (Fake `fail_next(Conflict)` → item continua, 1 toast).
  - `prune_dialog_lists_only_unused`.
  - `prune_result_toast_counts`.
- [ ] **Passo 2–4:** FAIL → implementar → PASS.
- [ ] **Passo 5:** Commit `feat(ui): volumes screen with safe remove and prune`.

### Tarefa 4.2: Tela Redes

**Arquivos:**
- Criar: `src/views/networks.rs`
- Modificar: `src/app.rs`, `locales/*.yml`

**Interfaces:**
- Produz:
  ```rust
  pub const PROTECTED_NETWORKS: [&str; 3] = ["bridge", "host", "none"];
  pub fn is_protected(n: &NetworkInfo) -> bool;
  pub struct NetworksState { pub items: Vec<NetworkInfo>, pub query: String,
                             pub confirm_remove: Option<String>, pub pending: HashSet<String> }
  pub enum Msg { Loaded(Result<Vec<NetworkInfo>, DockerError>), Query(String),
                 AskRemove(String), Confirm, Cancel, Removed(String, Result<(), DockerError>) }
  ```
  - Colunas: nome, driver, subnet (ou "—"), containers conectados.
  - Redes protegidas: sem botão remover, com cadeado e tooltip "Rede padrão do Docker".
  - Rede com containers conectados: remover permitido, mas o diálogo avisa; se o Docker responder
    `Conflict`, toast amigável.

- [ ] **Passo 1: Testes que falham:** `default_networks_are_protected`;
  `ask_remove_on_protected_is_ignored`; `remove_conflict_keeps_item_and_toasts`;
  `network_event_triggers_reload`.
- [ ] **Passo 2–4:** FAIL → implementar → PASS.
- [ ] **Passo 5:** Manual: `docker network create teste` → aparece; remover pelo app → some.
- [ ] **Passo 6:** Commit `feat(ui): networks screen with protected defaults`.
