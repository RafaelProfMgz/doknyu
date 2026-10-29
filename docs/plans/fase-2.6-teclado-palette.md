# Fase 2.6 — Teclado e command palette

> **Para agentes:** SUB-SKILL OBRIGATÓRIA: use superpowers:subagent-driven-development (recomendado)
> ou superpowers:executing-plans. Ver [README.md](README.md) e
> [execucao-agentes.md](execucao-agentes.md). Os passos usam checkbox (`- [ ]`).

**Objetivo:** tudo que a UI faz pode ser feito pelo teclado: um registro central de ações,
atalhos no estilo Zed, navegação na lista e uma command palette com busca aproximada.

**Arquitetura:** `src/actions.rs` (registro e disponibilidade), `src/keymap.rs` (tecla → ação,
função pura), `src/fuzzy.rs` (pontuação), `src/views/palette.rs` (overlay) e `src/ui/prefs.rs`
(preferência de tema). Botões, atalhos e palette convergem em `Message::Action(ActionId)` e
`App::run_action`.

**Stack:** iced 0.14 (`event::listen_with`, foco de `text_input` por `Id`).

**Spec:** [`docs/specs/2026-10-29-ui-zed-design.md`](../specs/2026-10-29-ui-zed-design.md)
(seções 4 e 5). Consome a Fase 2.5.

## Restrições globais

- Valem as restrições de [README.md](README.md) e de [fase-2.5](fase-2.5-visual-layout.md).
- Atalhos exatamente os da tabela da spec, seção 4.
- Teclas sem Ctrl (`j`, `k`, `s`, `r`, `/`, Delete, Enter) nunca disparam quando o evento foi
  capturado por um `text_input`.
- Remover pelo teclado sempre passa pelo modal de confirmação.
- Sem crates novos (fuzzy e caminho de configuração são implementação própria).
- Falha ao ler ou gravar a preferência de tema: só `tracing::warn!`, nunca toast nem pânico.

## Foco de revisão

1. Digitar "s", "j" ou "/" na busca → só escreve no campo, nenhuma ação dispara (teste na 2.6.1).
2. Atalho de ação de container sem conexão ou sem seleção → nada acontece, sem toast; na palette
   essas ações não aparecem (teste na 2.6.1 e 2.6.2).
3. Esc com várias camadas abertas (palette sobre modal sobre dock) → fecha só a de cima (teste
   na 2.6.1).
4. Palette com consulta sem resultado e Enter → nada executa, palette continua aberta (teste na 2.6.2).
5. `~/.config/doknyu/theme` com lixo, vazio ou diretório sem permissão → app abre em `System` e
   segue funcionando (teste na 2.6.2).

---

### Tarefa 2.6.1: Registro de ações, atalhos e navegação na lista

**Implementa:** `ui-implementer`.

**Arquivos:**
- Criar: `src/actions.rs`, `src/keymap.rs`
- Modificar: `src/lib.rs`, `src/app.rs`, `src/views/*.rs` (botões passam a emitir
  `Message::Action`), `locales/*.yml`

**Interfaces:**
- Consome: Fase 2.5 (`Shortcut` de `ui::components`, `selected`, `dock`, `SEARCH_ID`,
  `ToggleSidebar`/`ToggleDock`/`CloseDock`).
- Produz:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  pub enum ActionId { GoContainers, GoVolumes, GoNetworks, ToggleSidebar, ToggleDock,
      FocusSearch, OpenPalette, SelectNext, SelectPrev, OpenSelected, StartStopSelected,
      RestartSelected, RemoveSelected, ThemeSystem, ThemeDark, ThemeLight, Retry, Escape }
  pub struct ActionDef { pub id: ActionId, pub label_key: &'static str,
                         pub shortcut: Option<Shortcut>, pub in_palette: bool }
  pub fn all() -> &'static [ActionDef];
  pub fn def(id: ActionId) -> &'static ActionDef;
  pub fn available(app: &App, id: ActionId) -> bool;
  // keymap.rs
  pub fn resolve(key: &iced::keyboard::Key, mods: iced::keyboard::Modifiers,
                 captured: bool) -> Option<ActionId>;
  // app.rs
  Message::Action(ActionId),
  Message::Key { key: iced::keyboard::Key, mods: iced::keyboard::Modifiers, captured: bool },
  impl App { pub fn run_action(&mut self, id: ActionId) -> Task<Message>; }
  ```
  - `subscription()` inclui `iced::event::listen_with` (função `fn`, sem captura) que converte
    `Event::Keyboard(KeyPressed{..})` em `Message::Key` com `captured = status == Captured`.
    `Key` → `keymap::resolve` → `run_action` se `available`.
  - Em `resolve`: Ctrl+Shift+P → `OpenPalette`; Ctrl+1/2/3 → `Go*`; Ctrl+B → `ToggleSidebar`;
    Ctrl+J → `ToggleDock`; Ctrl+F → `FocusSearch`; Esc → `Escape` (mesmo com `captured`).
    Só com `captured == false`: `/` → `FocusSearch`; ↓/`j` → `SelectNext`; ↑/`k` → `SelectPrev`;
    Enter → `OpenSelected`; `s` → `StartStopSelected`; `r` → `RestartSelected`;
    Delete → `RemoveSelected`.
  - `available`: ações de container exigem `Connection::Ok`, tela Containers e `selected`
    existente; `Retry` só com `Connection::Failed`; `OpenPalette` sempre; temas sempre.
  - `run_action(Escape)` fecha a camada de cima, nesta ordem: palette (Tarefa 2.6.2) → modal de
    remoção → dock → busca não vazia (limpa) → nada.
  - `SelectNext/Prev` andam em `visible()`, sem dar a volta; sem seleção, `Next` pega o primeiro
    e `Prev` o último. `StartStopSelected`: rodando → `Stop`, senão → `Start`.
  - `FocusSearch` devolve a `Task` de foco do iced 0.14 para `SEARCH_ID`.
  - `ThemeSystem/Dark/Light` mudam `theme_mode` (a gravação em disco vem na Tarefa 2.6.2).
  - Botões existentes (sidebar, status bar, ações da linha, dock) passam a emitir
    `Message::Action(...)` quando houver `ActionId` correspondente; tooltips mostram o atalho.
  - Rótulos em `actions.<snake_case>` nos dois locales.

- [ ] **Passo 1: Testes que falham:**
  - `keymap_table` (`src/keymap.rs`) — tabela de casos: Ctrl+Shift+P → `OpenPalette`; Ctrl+2 →
    `GoVolumes`; `j` → `SelectNext`; Delete → `RemoveSelected`; Esc → `Escape`; Ctrl+P (sem
    Shift) → `None`.
  - `letters_ignored_while_typing` — `resolve(j|s|/|Enter, none, captured=true)` → `None`;
    `resolve(Ctrl+B, captured=true)` → `ToggleSidebar`; `resolve(Esc, captured=true)` → `Escape`.
  - `every_action_has_label_in_both_locales` — para cada `all()`, `t!(label_key)` em `en` e
    `pt-BR` é diferente da própria chave.
  - `select_next_prev_bounds` — `[a,b,c]`: sem seleção `Next` → a; em c `Next` fica em c; sem
    seleção `Prev` → c.
  - `start_stop_toggles_by_state` — selecionado rodando → `calls()` tem `stop:<id>`; parado →
    `start:<id>`.
  - `container_actions_unavailable_without_connection_or_selection` — `available` falso e
    `run_action` não chama o client nem cria toast.
  - `remove_selected_opens_confirmation` — `RemoveSelected` → `confirm_remove == Some(id)`, sem
    chamar `remove`.
  - `escape_closes_top_layer_only` — modal + dock abertos: 1º `Escape` fecha o modal, 2º o dock,
    3º limpa a busca.
  - `go_actions_navigate` — `GoNetworks` → `screen == Networks`.
- [ ] **Passo 2:** `cargo test keymap actions app::` → FAIL.
- [ ] **Passo 3:** Implementar.
- [ ] **Passo 4:** Testes e clippy (com e sem `fake`) → PASS.
- [ ] **Passo 5:** `cargo run --features fake`: usar o app só com teclado — Ctrl+1/2/3, j/k,
  Enter, s, r, Delete + Enter no modal, Esc, Ctrl+B, Ctrl+J, `/` e digitar "s" na busca sem
  disparar nada.
- [ ] **Passo 6:** Commit `feat(ui): action registry, keyboard shortcuts and list navigation`.

### Tarefa 2.6.2: Command palette e preferência de tema

**Implementa:** `ui-implementer`.

**Arquivos:**
- Criar: `src/fuzzy.rs`, `src/views/palette.rs`, `src/ui/prefs.rs`
- Modificar: `src/lib.rs`, `src/app.rs`, `src/main.rs`, `src/views/status_bar.rs`,
  `locales/*.yml`

**Interfaces:**
- Consome: Tarefa 2.6.1 (`ActionId`, `all`, `available`, `run_action`, `Escape`).
- Produz:
  ```rust
  // fuzzy.rs
  pub fn score(query: &str, candidate: &str) -> Option<u32>; // None = não casa
  // views/palette.rs
  pub struct PaletteState { pub query: String, pub cursor: usize }
  pub const PALETTE_INPUT_ID: &str = "palette-input";
  pub fn matches(app: &App, query: &str) -> Vec<ActionId>; // available && in_palette, por score desc
  pub fn view<'a>(app: &'a App, state: &'a PaletteState) -> Element<'a, Message>;
  // app.rs
  pub palette: Option<PaletteState>,
  Message::PaletteQuery(String), Message::PaletteMove(i32), Message::PaletteRun(Option<ActionId>),
  pub fn new_with_config(client: Arc<dyn DockerClient>, config_dir: Option<PathBuf>) -> (Self, Task<Message>);
  // ui/prefs.rs
  pub fn config_dir() -> Option<PathBuf>; // $XDG_CONFIG_HOME/doknyu ou $HOME/.config/doknyu
  pub fn load_theme_mode(dir: &Path) -> ThemeMode;          // lixo/ausente → System
  pub fn save_theme_mode(dir: &Path, m: ThemeMode) -> std::io::Result<()>; // cria o diretório
  ```
  - `score`: casa se todas as letras da consulta aparecem em ordem no candidato
    (case-insensitive, sem acento: comparar após remover diacríticos de á à â ã é ê í ó ô õ ú ç).
    Pontos: +1 por letra, +8 se a letra inicia uma palavra, +4 se é consecutiva à anterior,
    +10 se o candidato começa com a consulta. Consulta vazia → `Some(0)` para todos.
  - Empate em `matches`: mantém a ordem de `all()`.
  - `OpenPalette` → `palette = Some(default)` + `Task` de foco em `PALETTE_INPUT_ID`.
    `Escape` fecha a palette antes de qualquer outra camada.
  - Com a palette aberta, o handler de `Message::Key` intercepta ↑/↓ e Enter **antes** do
    `keymap` (mesmo com `captured`): ↑/↓ viram `PaletteMove(-1/+1)` (cursor limitado aos
    resultados) e Enter vira `PaletteRun(matches.get(cursor))`. Outras teclas sem Ctrl são
    ignoradas enquanto a palette está aberta. `PaletteRun(Some(id))` fecha a palette e chama
    `run_action(id)`; `PaletteRun(None)` não faz nada.
  - Visual: `modal`-like `elevated`, largura 560, alinhada ao topo (margem 64), raio `RADIUS_LG`;
    item com rótulo traduzido à esquerda e `kbd` à direita; cursor com fundo `selected`; no
    máximo 12 itens visíveis com rolagem.
  - O botão `Ctrl ⇧ P` da status bar emite `Action(OpenPalette)`.
  - `run_action(Theme*)` muda `theme_mode` e, se `config_dir` existir, grava com
    `save_theme_mode` dentro de `Task::perform` (erro → `tracing::warn!`).
  - `main.rs` usa `App::new_with_config(client, prefs::config_dir())`, que carrega o tema salvo;
    `App::new` continua existindo (chama com `None`) para os testes.

- [ ] **Passo 1: Testes que falham:**
  - `fuzzy_basics` (`src/fuzzy.rs`) — `score("stp","Parar container")` é `None`;
    `score("par","Parar container") > score("par","Abrir paleta")`;
    `score("rei","Reiniciar container")` é `Some`; `score("acao","Ação")` é `Some`;
    `score("","x") == Some(0)`.
  - `fuzzy_prefers_word_starts` — `score("rc","Reiniciar container") > score("rc","Abrir console")`
    (definir os dois rótulos só no teste).
  - `palette_hides_unavailable_actions` — sem seleção, `matches(app,"")` não contém
    `StartStopSelected`; com `Connection::Failed`, contém `Retry`.
  - `palette_run_executes_and_closes` — abrir, `PaletteQuery("volumes")`, `PaletteRun(Some(GoVolumes))`
    → `palette == None` e `screen == Volumes`.
  - `palette_enter_without_results_keeps_open` — `PaletteRun(None)` → palette continua aberta.
  - `palette_cursor_is_clamped` — `PaletteMove(+50)` → cursor no último resultado.
  - `escape_closes_palette_first` — palette + modal abertos: `Escape` fecha só a palette.
  - `theme_pref_roundtrip` (`src/ui/prefs.rs`, diretório em `std::env::temp_dir()` com nome único) —
    salvar `Light` e ler → `Light`; arquivo com `"azul"` → `System`; diretório inexistente → `System`.
  - `theme_action_saves_preference` — `new_with_config(fake, Some(tmp))`, `Action(ThemeDark)` e
    executar a `Task` → arquivo contém `dark`.
- [ ] **Passo 2:** `cargo test fuzzy palette prefs` → FAIL.
- [ ] **Passo 3:** Implementar; rótulos e textos da palette nos dois locales.
- [ ] **Passo 4:** Testes e clippy (com e sem `fake`) → PASS.
- [ ] **Passo 5:** `LANG=pt_BR.UTF-8 cargo run --features fake`: Ctrl+Shift+P, digitar "vol" →
  Enter vai para Volumes; "esc" → Tema escuro, fechar e reabrir o app → continua escuro;
  `echo azul > ~/.config/doknyu/theme` → app abre seguindo o sistema.
- [ ] **Passo 6:** Commit `feat(ui): command palette with fuzzy search and saved theme`.

---

**Fim de fase:** `phase-reviewer` e parada para verificação manual do humano (ver
[execucao-agentes.md](execucao-agentes.md)).
