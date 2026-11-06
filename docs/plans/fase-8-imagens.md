# Fase 8 — Tela de Imagens

> Ver [README.md](README.md). Spec: [`2026-11-05-imagens-design.md`](../specs/2026-11-05-imagens-design.md)
> (o implementador lê as seções citadas em cada tarefa). Consome a trait `DockerClient`, `FakeClient`,
> `friendly_error`, a Subscription de eventos (Fase 2), os componentes da Fase 2.5 e o registro de
> ações da Fase 2.6. A tela de **Volumes** (`src/views/volumes.rs`, Fase 4) é o modelo a copiar:
> estado, `Msg`, seleção/hover/scroll, `pending`, diálogo de confirmação e prune.

**Entrega:** tela Imagens na sidebar (depois de Redes), com lista por tag, remover seguro e limpeza
das imagens sem tag (dangling).

Branch: `feat/fase-8-imagens` (a partir da `develop`), PR para a `develop`.

---

### Tarefa 8.1: Core de imagens (`rust-implementer`)

Spec: seção 2 e os testes de `convert`/`--ignored` da seção 6.

**Arquivos:**
- Modificar: `src/model.rs`, `src/docker/mod.rs`, `src/docker/convert.rs`, `src/docker/bollard.rs`,
  `src/docker/fake.rs`, `tests/` (teste `#[ignore]` contra Docker real, junto dos existentes)

**Interfaces:**
- Produz:
  ```rust
  // src/model.rs (Debug, Clone, PartialEq, Eq)
  pub struct ImageInfo { pub id: String, pub repo_tags: Vec<String>, pub size: u64,
                         pub created: i64, pub used_by: Vec<String> }
  pub struct PruneReport { pub removed: Vec<String>, pub reclaimed: u64 }
  pub enum ResourceKind { Container, Volume, Network, Image }   // + Image

  // src/docker/mod.rs, trait DockerClient
  async fn list_images(&self) -> Result<Vec<ImageInfo>, DockerError>;
  async fn remove_image(&self, reference: &str) -> Result<(), DockerError>;
  async fn prune_images(&self) -> Result<PruneReport, DockerError>;

  // src/docker/convert.rs
  pub fn image_from(s: &ImageSummary, used_by: Vec<String>) -> ImageInfo;
  ```
  - `image_from`: `repo_tags` igual a `["<none>:<none>"]` vira vazio (dangling); tamanho negativo vira 0.
  - `list_images`: `all = false`; `used_by` cruzando com `list_containers(all = true)` pelo ID da
    imagem (`ContainerSummary::image_id`), com os nomes dos containers, como `used_by` de volumes.
  - `remove_image(reference)`: sem `force`, sem `noprune`; erros passam por `api_error_from`
    (409 → `Conflict`, 404 → `NotFound`).
  - `prune_images`: filtro `dangling=true`; `removed` = IDs `Deleted` do relatório, `reclaimed` =
    `space_reclaimed` (0 se ausente).
  - `event_from`: `EventMessageTypeEnum::IMAGE` → `ResourceKind::Image`.
  - `FakeClient`: `add_image(ImageInfo)`; `remove_image` aceita `repo:tag` (tira a tag; apaga a
    imagem quando não sobra tag) ou ID; recusa com `Conflict` se `used_by` não está vazio;
    `prune_images` apaga as de `repo_tags` vazio e soma os tamanhos; respeita `fail_next` e
    registra em `calls()`. `with_sample_data` ganha imagens coerentes com os containers de exemplo
    (incluindo uma dangling).
  - Nenhum tipo do bollard sai de `src/docker/`.

- [ ] **Passo 1: Testes que falham:** `image_from_tagged`, `image_from_dangling_none_tag`,
  `image_from_dangling_empty_tags`, `event_maps_image_type`; ajustar
  `event_ignores_unknown_types` para usar `"plugin"`; no Fake: `fake_remove_tag_keeps_image`,
  `fake_remove_last_tag_deletes_image`, `fake_remove_in_use_conflicts`,
  `fake_prune_removes_only_dangling_and_sums_size`.
- [ ] **Passo 2–4:** FAIL → implementar → PASS.
- [ ] **Passo 5:** Teste `#[ignore]` com Docker real: `list_images` vê uma imagem puxada no setup;
  `remove_image` numa tag criada pelo teste (`docker tag`); `prune_images` apaga uma dangling criada
  pelo teste. Rodar com `cargo test -- --ignored` se houver Docker; relatar se não houver.
- [ ] **Passo 6:** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- [ ] **Passo 7:** Commit `feat(docker): list, remove and prune images`.

### Tarefa 8.2: Tela Imagens (`ui-implementer`)

Spec: seções 3, 4, 5 e os testes de view/formatação da seção 6.

**Arquivos:**
- Criar: `src/views/images.rs`
- Modificar: `src/app.rs`, `src/actions.rs`, `src/keymap.rs`, `src/views/mod.rs`,
  `src/views/sidebar.rs`, `locales/en.yml`, `locales/pt-BR.yml`

**Interfaces:**
- Produz:
  ```rust
  pub struct ImageRow { pub reference: String /* repo:tag, ou o ID se dangling */,
                        pub label: String /* repo:tag ou "<none>:<none>" */,
                        pub id: String, pub size: u64, pub created: i64,
                        pub used_by: Vec<String>, pub other_tags: usize }
  pub fn rows(items: &[ImageInfo]) -> Vec<ImageRow>;      // uma linha por tag
  pub fn format_size(bytes: u64) -> String;               // base 1000: "512 B", "1.2 MB", "3.4 GB"
  pub fn format_age(created: i64, now: i64) -> String;    // via t!, "3 dias" / "3 days"
  pub enum ImageConfirm { Remove(String /* reference */), Prune(Vec<(String, u64)> /* id, size */) }
  pub struct ImagesState { items, query, confirm: Option<ImageConfirm>, pending: HashSet<String>,
                           selected, hovered, scroll_y, viewport_h }   // como VolumesState
  pub enum Msg { Loaded(u64, Result<Vec<ImageInfo>, DockerError>), Query(String), Select(String),
                 Hover(String), Leave(String), AskRemove(String), AskPrune, Confirm, Cancel,
                 Removed(String, Result<(), DockerError>), Pruned(Result<PruneReport, DockerError>),
                 Scrolled { offset: f32, height: f32 }, Resized(f32) }
  ```
  - `Screen::Images` depois de `Networks`; sidebar com ícone Lucide e contagem.
  - `ActionId::GoImages` (`Ctrl+4`; `4` no modo de lista) e `ActionId::PruneImages` (palette, sem
    atalho; de outra tela vai para Imagens e abre o diálogo, como `PruneVolumes`).
    `SelectNext/Prev`, `RemoveSelected` e `Escape` valem na tela.
  - Colunas e estilos da tabela da spec (seção 3); busca por repo:tag ou ID, case-insensitive.
  - Remover desabilitado com `used_by` não vazio, tooltip "Em uso por: …"; o diálogo avisa quando a
    imagem tem outras tags; linha `pending` durante a operação; o diálogo fecha se a linha some.
  - Prune: diálogo lista as dangling (ID curto + tamanho) e o total; "Confirmar" desabilitado com
    lista vazia; recalcula quando a lista muda; `Pruned(Ok(r))` → toast "N imagens removidas,
    X liberados".
  - Recarrega ao entrar na tela, em evento `ResourceKind::Image` e, com a tela aberta, em evento de
    container (muda `used_by`). Mesmo debounce das outras telas.
  - Erros: toast via `friendly_error` e recarga; Docker indisponível como nas outras listas.
  - Todas as strings em `locales/en.yml` e `locales/pt-BR.yml` (`images.*`, `actions.go_images`,
    `actions.prune_images`).

- [ ] **Passo 1: Testes que falham:** `rows_one_per_tag`, `dangling_row_uses_id_as_reference`,
  `in_use_image_cannot_be_removed`, `remove_dialog_closes_when_row_disappears`,
  `remove_conflict_keeps_item_and_toasts`, `prune_dialog_lists_only_dangling`,
  `prune_with_empty_list_cannot_confirm`, `prune_toast_counts_and_bytes`,
  `search_by_tag_and_by_id`, `image_event_triggers_reload`, `format_size_base_1000`,
  `format_age_relative`, `go_images_shortcut`.
- [ ] **Passo 2–4:** FAIL → implementar → PASS.
- [ ] **Passo 5:** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`;
  `cargo run --features fake` abre a tela com os dados de exemplo.
- [ ] **Passo 6:** Commit `feat(ui): images screen with safe remove and prune`.

### Fim de fase

`phase-reviewer` sobre os commits da fase; o humano testa com Docker real
(`docker pull alpine`, `docker tag alpine teste:1`, remover pelo app, `docker build` gerando
dangling e "Limpar sem tag") antes do PR para a `develop`.
