# Fase 3 — Detalhe do container: Logs e Stats

> Ver [README.md](README.md). Consome Fases 1, 2, 2.5 e 2.6 (dock, `StatsHistory`, `DockerClient::logs/stats`).

**Entrega:** clicar num container abre o detalhe com abas Logs e Stats funcionando ao vivo.

## Adaptação às Fases 2.5 e 2.6 (vale sobre o texto abaixo)

- Não existe mais `Screen::Detail`: o detalhe é o **dock inferior** (`App::dock: Option<Dock>`,
  Fase 2.5). Use `DockTab` da Fase 2.5 no lugar de `Tab`; `DetailState` fica guardado junto do
  dock (ex.: `App::detail: Option<DetailState>`, criado em `OpenDock` e descartado ao fechar).
  `Msg::Back` vira `CloseDock`. "Volta para `Screen::Containers`" significa "fecha o dock".
- O dock já fecha sozinho quando o container some da lista (Fase 2.5, sem toast). A Fase 3
  acrescenta o toast "O container não existe mais" só quando o fechamento veio de `destroy`
  ou `NotFound` com o dock aberto; `stopped_container_keeps_detail_open` verifica `dock.is_some()`.
- Visual: logs em `fonts::MONO` tamanho `TEXT_SM`, `Stderr` em `warning`, destaque da busca em
  `accent`; barra da aba com `search_field`, `chip` (timestamps) e `icon_button(Copy)`; cartões de
  Stats com `container::surface` e bordas de 1px; sparkline em `accent`. Nada de cor literal.
- Cabeçalho do dock é o `panel_header` da Fase 2.5 (nome + abas + ações); não criar outro.
- Ações registradas em `actions.rs` (com rótulos nos dois locales): `DockLogs`, `DockStats`
  (Ctrl+Shift+L / Ctrl+Shift+S, abrem o dock do selecionado na aba), `ToggleTimestamps`,
  `CopyLogs`, `FocusLogSearch` (Ctrl+F quando o foco lógico está no dock: o dock está aberto e
  a aba é Logs).
- `sparkline` e `human_bytes` vão para `src/ui/components/` (não para `views/widgets.rs`).
- `keymap::resolve` não conhece o estado do app: Ctrl+F sempre resolve para `FocusSearch`. Em
  `App::on_key`, troque para `FocusLogSearch` quando o dock estiver aberto na aba Logs.

---

### Tarefa 3.1: Buffer de logs (`logbuf.rs`)

**Arquivos:**
- Criar: `src/logbuf.rs`

**Interfaces:**
- Produz:
  ```rust
  pub const MAX_LINES: usize = 10_000;
  pub const MAX_LINE_CHARS: usize = 4_000;
  pub fn sanitize(raw: &[u8]) -> String; // lossy UTF-8, remove ANSI (CSI/OSC), remove '\r' final, trunca com '…'
  pub struct LogBuffer { /* VecDeque<LogLine> */ }
  impl LogBuffer {
      pub fn new() -> Self;
      pub fn push(&mut self, line: LogLine);            // descarta a mais antiga acima de MAX_LINES
      pub fn len(&self) -> usize;
      pub fn iter(&self) -> impl Iterator<Item = &LogLine>;
      pub fn search(&self, q: &str) -> Vec<usize>;      // índices, case-insensitive
      pub fn to_text(&self, timestamps: bool) -> String; // para "copiar"
  }
  ```
- `convert::log_line_from` (Fase 1) passa a usar `sanitize` e separa o timestamp RFC3339 do início.

- [ ] **Passo 1: Testes que falham:**
  - `sanitize_strips_ansi`: `b"\x1b[31mERRO\x1b[0m"` → `"ERRO"`.
  - `sanitize_invalid_utf8`: `b"ok \xff fim"` → contém `"ok "` e `" fim"`, sem panic.
  - `sanitize_truncates_long_line`: 10.000 `'a'` → `chars().count() == 4_001` e termina com `'…'`.
  - `sanitize_strips_carriage_return`: `b"linha\r"` → `"linha"`.
  - `buffer_caps_at_max`: 10.001 pushes → `len() == 10_000` e a primeira linha é a de índice 1.
  - `search_is_case_insensitive`.
  - `to_text_with_and_without_timestamps`.
- [ ] **Passo 2–4:** FAIL → implementar → PASS.
- [ ] **Passo 5:** Commit `feat(core): bounded log buffer with ansi sanitizing and search`.

### Tarefa 3.2: Aba Logs

**Arquivos:**
- Criar: `src/views/detail/mod.rs`, `src/views/detail/logs.rs`
- Modificar: `src/app.rs`

**Interfaces:**
- Produz:
  ```rust
  pub enum Tab { Logs, Stats, Terminal }
  pub struct DetailState { pub id: String, pub info: Option<ContainerInfo>, pub tab: Tab,
                           pub logs: LogBuffer, pub follow: bool, pub timestamps: bool, pub query: String,
                           pub stats: StatsHistory, pub gone: bool }
  pub enum Msg { Tab(Tab), LogBatch(Vec<LogLine>), LogsEnded, Scrolled { at_bottom: bool },
                 ToggleTimestamps(bool), Query(String), Copy, StatsSample(RawStats), StatsEnded, Back }
  ```
  - Subscription de logs (só com detalhe aberto) usa `logs(id, 500)` e entrega em **lotes**
    (agrupa linhas a cada 100 ms ou 200 linhas) → `LogBatch`.
  - **Reassinatura:** a identidade da subscription de logs/stats inclui `(id, generation)`;
    `generation` incrementa quando o container volta a rodar (evento `start`/`restart` do id).
    Ao reassinar, o `LogBuffer` é limpo antes (o `logs(id, 500)` traz o histórico de novo, sem duplicar)
    e o `StatsHistory` recomeça.
  - `follow` = auto-scroll; vira `false` quando `Scrolled { at_bottom: false }`, volta a `true` no fim
    ou no botão "Ir para o fim".
  - Renderização: só as linhas visíveis (lista virtualizada por altura de linha fixa, fonte mono);
    linhas `Stderr` em cor de aviso; ocorrências da busca destacadas, com "n de m".
  - `Copy` → `iced::clipboard::write(logs.to_text(timestamps))`.
  - Cabeçalho: nome, imagem, badge de status e botões de ação reaproveitados da Tarefa 2.3.

- [ ] **Passo 1: Testes que falham:** `log_batch_appends`; `scroll_up_disables_follow`;
  `switching_container_resets_buffer`; `restart_resubscribes_logs` (Fake: com detalhe aberto,
  `stop` → `LogsEnded`; `start` → `generation` incrementa, buffer limpo e novas linhas chegam).
- [ ] **Passo 2–4:** FAIL → implementar → PASS.
- [ ] **Passo 5:** Manual: `docker run -d --name spam alpine sh -c 'while true; do date; done'` →
  aba Logs rola sem travar a UI (CPU do doknyu < 1 núcleo); rolar para cima pausa.
- [ ] **Passo 6:** Commit `feat(ui): live container logs tab`.

### Tarefa 3.3: Aba Stats + container que some

**Arquivos:**
- Criar: `src/views/detail/stats.rs`, `src/views/widgets.rs` (adicionar `sparkline`)
- Modificar: `src/views/detail/mod.rs`, `src/app.rs`

**Interfaces:**
- Consome: `stats::sample_from_raw`, `StatsHistory`.
- Produz:
  - `pub fn sparkline(values: &[f64], max: Option<f64>) -> Element<'_, Message>` (canvas; `max = None`
    → escala pelo maior valor; nunca divide por zero).
  - `pub fn human_bytes(n: u64) -> String` (`1536` → `"1.5 KiB"`, `0` → `"0 B"`).
  - Cartões: CPU % (sparkline 0–100 × nº de CPUs), Memória "usado / limite" + sparkline, Rede RX/TX,
    Disco leitura/escrita.
  - Stats só são assinados com o container `Running`; parado → mensagem "Inicie o container para ver
    o consumo".
  - **Fim de stream ≠ container sumiu:** o daemon encerra os streams de logs/stats quando o
    container **para**. `LogsEnded`/`StatsEnded` sozinhos só marcam "container parado" (mantém o
    detalhe aberto e os logs já recebidos); voltar para a lista só com evento `destroy` ou `NotFound`.
  - `FakeClient` ganha `pub fn push_stats(&self, id: &str, raw: RawStats)` (resolve nome/prefixo,
    como `push_log`) e, com `with_sample_data()`, os containers Running geram uma amostra por segundo
    para a UI fake mostrar gráficos.
  - **Container sumiu:** `LogsEnded`/`StatsEnded` + evento `destroy` do id (ou `NotFound` ao recarregar)
    → `gone = true`, toast "O container não existe mais" e volta para `Screen::Containers`.

- [ ] **Passo 1: Testes que falham:** `human_bytes_formats`; `stats_sample_pushed_to_history`;
  `stopped_container_has_no_stats_subscription`;
  `destroyed_container_returns_to_list` (Fake: `close_streams(id)` + `push_event(destroy)` →
  `screen == Containers` e 1 toast);
  `sparkline_handles_all_zero_values` (sem panic).
  `stopped_container_keeps_detail_open` (Fake: `stop(id)` → streams terminam, `screen` continua
  `Detail(id)`, logs preservados, nenhum toast de "não existe mais").
- [ ] **Passo 2–4:** FAIL → implementar → PASS.
- [ ] **Passo 5:** Manual: `docker run -d --name burn alpine sh -c 'while :; do :; done'` → CPU ~100%;
  `docker rm -f burn` com o detalhe aberto → volta para a lista com aviso.
- [ ] **Passo 6:** Commit `feat(ui): stats tab with sparklines and gone-container handling`.

### Tarefa 3.4: Seleção e cópia de texto nos logs (como num terminal)

**Implementa:** `ui-implementer`. Pedido do humano depois da Tarefa 3.2.

**Arquivos:**
- Modificar: `src/views/detail/logs.rs`, `src/views/detail/mod.rs`, `src/app.rs`
- Criar: `src/views/detail/log_highlight.rs` (highlighter das cores)

**Comportamento:**
- O corpo da aba Logs passa a ser um `text_editor` **somente leitura** com as linhas visíveis (a
  mesma janela virtualizada de hoje, com overscan). Arrastar seleciona de qualquer caractere a
  qualquer outro; duplo clique seleciona a palavra; triplo clique a linha; Ctrl+A seleciona o
  visível; Ctrl+C copia a seleção (cópia do próprio widget).
- **Sem edição:** o `on_action` aplica só ações que não são de edição (`!action.is_edit()`);
  digitar, colar, apagar e Enter não mudam nada.
- Ao começar uma seleção (`Click`/`Drag`/`Select*`), `follow` vira `false` e a janela visível
  congela: lotes novos entram no `LogBuffer` mas o conteúdo do editor só é reconstruído quando a
  janela muda (rolagem, "Ir para o fim", troca de busca). Isso mantém a seleção estável.
- `Esc` com seleção ativa limpa a seleção antes das outras camadas (`close_top_layer`: palette >
  modal > **seleção nos logs** > busca de logs > dock > busca da lista).
- Cores mantidas por um `Highlighter` próprio: linhas `Stderr` em `warning`, ocorrências da busca
  em `accent` (fundo da linha atual da busca continua `selected`), timestamps em `faint`.
- Teclas de atalho de uma letra (`j`, `s`, `/`…) não disparam com o editor focado (o evento chega
  como capturado); Ctrl+C dentro do editor não é interceptado pelo app.
- Limitação aceita: a seleção cobre só a área visível (não rola sozinha ao arrastar além da borda).

- [ ] **Passo 1: Testes que falham:**
  - `log_editor_ignores_edits` — aplicar `Action::Edit(Insert('x'))` não muda o texto.
  - `log_selection_pauses_follow` — `Action::Drag` com follow ativo → `follow == false`.
  - `selection_survives_new_batches` — com seleção ativa, `LogBatch` não reconstrói o conteúdo
    (texto do editor igual ao anterior) e o buffer cresce.
  - `escape_clears_log_selection_first` — seleção ativa + dock aberto: 1º Esc limpa a seleção,
    dock continua aberto.
  - `highlight_marks_stderr_and_matches` — o highlighter devolve `warning` para linhas stderr e
    `accent` para a faixa da busca.
- [ ] **Passo 2–4:** FAIL → implementar → PASS (testes e clippy com e sem `fake`).
- [ ] **Passo 5:** Manual: selecionar parte de uma linha e várias linhas, Ctrl+C e colar num editor;
  digitar não altera nada; com logs chegando, a seleção não some.
- [ ] **Passo 6:** Commit `feat(ui): select and copy log text like a terminal`.
