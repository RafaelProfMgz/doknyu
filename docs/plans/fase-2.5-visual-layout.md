# Fase 2.5 — Sistema visual e layout no estilo Zed

> **Para agentes:** SUB-SKILL OBRIGATÓRIA: use superpowers:subagent-driven-development (recomendado)
> ou superpowers:executing-plans. Ver [README.md](README.md) e
> [execucao-agentes.md](execucao-agentes.md). Os passos usam checkbox (`- [ ]`).

**Objetivo:** trocar o visual padrão do iced por um sistema próprio no estilo do Zed (tokens,
tema claro/escuro seguindo o sistema, fontes e ícones embutidos, componentes) e reorganizar a
janela em sidebar, área principal com dock inferior e barra de status.

**Arquitetura:** módulo novo `src/ui/` (tokens, tema, estilos, fontes, ícones, componentes) que
não conhece `App` nem `Message` (componentes são genéricos em `M`). As views em `src/views/`
passam a montar telas só com `ui::components` e `ui::style`.

**Stack:** iced 0.14 (`Theme::custom`, `system::theme`/`theme_changes`, `pane_grid`,
`mouse_area`), IBM Plex Sans/Mono, fonte de ícones Lucide.

**Spec:** [`docs/specs/2026-10-29-ui-zed-design.md`](../specs/2026-10-29-ui-zed-design.md)
(seções 2, 3 e 5). Consome as Fases 1 e 2.

## Restrições globais

- Valem as restrições de [README.md](README.md) (MSRV 1.89, `t!` em todo texto, sem `bollard`
  na UI, clippy `-D warnings`, `cargo fmt`).
- Nenhuma view usa cor, espaçamento, raio ou tamanho de fonte literal: só `ui::tokens`.
- Cores exatamente as da tabela da spec, seção 2 (One Dark / One Light).
- Raio 4px (`RADIUS`), 6px (`RADIUS_LG`) em modal/palette; bordas 1px; sombra só em `elevated`.
- Tamanhos: `TEXT_SM` 12, `TEXT` 13, `TEXT_LG` 15, `ROW_HEIGHT` 28, `STATUS_BAR_HEIGHT` 24,
  `SIDEBAR_WIDTH` 200, `SIDEBAR_COLLAPSED` 40; espaçamentos múltiplos de 4.
- Barra de título nativa (sem `decorations(false)`).
- Fontes e ícones embutidos com `include_bytes!`; licenças em `assets/fonts/LICENSES.md`.
- Toda chave nova de i18n entra em `en.yml` e `pt-BR.yml` no mesmo commit.

## Foco de revisão

1. Sistema sem preferência de tema (`Mode::None`) ou troca de tema com o app aberto → resolve
   para escuro e redesenha sem reiniciar (teste na Tarefa 2.5.1).
2. Container aberto no dock é removido (por fora ou pelo app) → dock fecha sem pânico e sem
   toast (teste na Tarefa 2.5.3).
3. Conexão cai com o dock aberto ou com uma linha selecionada → onboarding no centro; ao voltar,
   dock fechado e seleção só mantida se o container ainda existir (teste na Tarefa 2.5.3).
4. Lista com nome/imagem muito longos ou dezenas de portas → a linha mantém 28px, texto corta
   com reticências sem empurrar as ações para fora (teste de view na Tarefa 2.5.4).
5. Os dois chips de filtro desligados → estado vazio explicando o filtro, não lista "sumida"
   (teste na Tarefa 2.5.4).

---

### Tarefa 2.5.1: Tokens, tema, fontes e ícones

**Implementa:** `research-implementer` (precisa baixar fontes e fixar a versão do Lucide).

**Arquivos:**
- Criar: `src/ui/mod.rs`, `src/ui/tokens.rs`, `src/ui/theme.rs`, `src/ui/fonts.rs`,
  `src/ui/icons.rs`, `assets/fonts/` (`IBMPlexSans-Regular.ttf`, `IBMPlexSans-Medium.ttf`,
  `IBMPlexSans-SemiBold.ttf`, `IBMPlexMono-Regular.ttf`, `lucide.ttf`, `LICENSES.md`)
- Modificar: `src/lib.rs`, `src/main.rs`, `src/app.rs`

**Interfaces:**
- Produz:
  ```rust
  // tokens.rs
  pub struct Palette { pub bg: Color, pub surface: Color, pub elevated: Color, pub border: Color,
      pub hover: Color, pub selected: Color, pub text: Color, pub muted: Color, pub faint: Color,
      pub accent: Color, pub success: Color, pub warning: Color, pub danger: Color }
  pub const DARK: Palette; pub const LIGHT: Palette;
  pub const SPACE_1: f32 = 4.0; /* SPACE_2 = 8, SPACE_3 = 12, SPACE_4 = 16, SPACE_6 = 24 */
  pub const RADIUS: f32 = 4.0; pub const RADIUS_LG: f32 = 6.0;
  pub const TEXT_SM: f32 = 12.0; pub const TEXT: f32 = 13.0; pub const TEXT_LG: f32 = 15.0;
  pub const ROW_HEIGHT: f32 = 28.0; pub const STATUS_BAR_HEIGHT: f32 = 24.0;
  pub const SIDEBAR_WIDTH: f32 = 200.0; pub const SIDEBAR_COLLAPSED: f32 = 40.0;
  // theme.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  pub enum ThemeMode { #[default] System, Dark, Light }
  pub fn resolve(mode: ThemeMode, system: iced::theme::Mode) -> &'static Palette; // None → DARK
  pub fn theme(p: &'static Palette) -> iced::Theme;   // Theme::custom("doknyu dark"/"doknyu light", …)
  pub fn palette(theme: &iced::Theme) -> &'static Palette; // pelo nome do tema; fallback DARK
  // fonts.rs
  pub const SANS: Font; pub const SANS_MEDIUM: Font; pub const SANS_SEMIBOLD: Font;
  pub const MONO: Font; pub const ICONS: Font;
  pub fn all_bytes() -> [&'static [u8]; 5];
  // icons.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum Icon { Box, Database, Network, Play, Square, RotateCw, Trash2, Search, PanelLeft,
      PanelBottom, Maximize2, Minimize2, X, ExternalLink, Command, Circle, TriangleAlert, Copy }
  impl Icon { pub const ALL: &'static [Icon]; pub fn codepoint(self) -> char; }
  pub fn icon<'a>(i: Icon) -> iced::widget::Text<'a>; // text(char).font(ICONS).size(TEXT + 1)
  ```
  - `App` ganha `theme_mode: ThemeMode` (padrão `System`), `system_mode: iced::theme::Mode`
    (padrão `None`) e `pub fn theme(&self) -> Theme`.
  - `Message::SystemTheme(iced::theme::Mode)`; `App::new` encadeia `iced::system::theme()` →
    `SystemTheme`; `subscription()` sempre inclui `iced::system::theme_changes()` →
    `SystemTheme` (inclusive no onboarding).
  - `main.rs`: `.theme(App::theme)`, `.font(...)` para cada item de `all_bytes()` e
    `.default_font(fonts::SANS)`.
- Notas:
  - Fontes IBM Plex: `google/fonts` (`ofl/ibmplexsans`, `ofl/ibmplexmono`), estáticas TTF.
  - Lucide: pacote `lucide-static`, versão fixada e anotada em `LICENSES.md`; os `codepoint`
    vêm do mapeamento desse pacote (não inventar).
  - `Theme::custom` recebe um `iced::theme::Palette` derivado (background = `bg`, text = `text`,
    primary = `accent`, success, warning, danger) para os widgets que ainda usam o estilo padrão.

- [ ] **Passo 1: Testes que falham** (`src/ui/theme.rs`, `src/ui/icons.rs`, `src/app.rs`):
  - `resolve_follows_system` — `resolve(System, Light)` é `&LIGHT`; `(System, Dark)` e
    `(System, None)` são `&DARK`.
  - `forced_mode_ignores_system` — `resolve(Light, Dark)` é `&LIGHT`; `resolve(Dark, Light)` é `&DARK`.
  - `palette_roundtrips_through_theme` — `palette(&theme(&LIGHT))` é `&LIGHT`, idem `DARK`.
  - `dark_palette_matches_spec` — `DARK.bg == Color::from_rgb8(0x28,0x2c,0x33)` e
    `DARK.accent == Color::from_rgb8(0x74,0xad,0xe8)`; `LIGHT.accent == from_rgb8(0x5c,0x78,0xe2)`.
  - `icon_codepoints_are_unique_and_private_use` — todos os `Icon::ALL` distintos e em
    `U+E000..=U+F8FF`.
  - `system_theme_change_updates_app_theme` — `update(SystemTheme(Light))` →
    `palette(&app.theme())` é `&LIGHT`.
- [ ] **Passo 2:** `cargo test ui:: system_theme` → FAIL (módulos não existem).
- [ ] **Passo 3:** Baixar fontes, escrever `LICENSES.md`, implementar os módulos e a ligação em
  `main.rs`/`app.rs`.
- [ ] **Passo 4:** `cargo test --workspace` e `cargo clippy --workspace --all-targets -- -D warnings`
  (com e sem `--features fake`) → PASS.
- [ ] **Passo 5:** `cargo run --features fake`: a UI aparece em IBM Plex; trocar o tema do sistema
  (GNOME: Configurações → Aparência) troca as cores sem reiniciar.
- [ ] **Passo 6:** Commit `feat(ui): design tokens, zed-like themes, fonts and icons`.

### Tarefa 2.5.2: Estilos e componentes base

**Implementa:** `ui-implementer`.

**Arquivos:**
- Criar: `src/ui/style.rs`, `src/ui/components/mod.rs` e um arquivo por componente
  (`icon_button.rs`, `list.rs`, `status_dot.rs`, `search_field.rs`, `chip.rs`, `kbd.rs`,
  `panel_header.rs`, `modal.rs`, `toast.rs`)
- Modificar: `src/views/widgets.rs` (remove `status_badge` e `confirm_dialog`; mantém
  `friendly_error` e `state_label`), chamadas atuais desses dois em `src/views/`

**Interfaces:**
- Consome: Tarefa 2.5.1 (`tokens`, `palette(&Theme)`, `fonts`, `Icon`, `icon`).
- Produz (todas genéricas em `M: Clone + 'a`, sem estado):
  ```rust
  // style.rs
  pub mod button { pub fn ghost(t:&Theme,s:Status)->Style; pub fn primary(..); pub fn danger(..);
                   pub fn chip(active: bool) -> impl Fn(&Theme, Status) -> Style; }
  pub mod container { pub fn bg(t:&Theme)->Style; pub fn surface(..); pub fn elevated(..);
                      pub fn row(selected: bool, hovered: bool) -> impl Fn(&Theme) -> Style; }
  pub fn text_input_field(t:&Theme, s:text_input::Status) -> text_input::Style;
  pub fn scrollable_thin(t:&Theme, s:scrollable::Status) -> scrollable::Style;
  pub fn rule_border(t:&Theme) -> rule::Style;
  // components
  pub fn icon_button<'a,M>(icon: Icon, on_press: Option<M>, tooltip: String) -> Element<'a,M>;
  pub struct RowEvents<M> { pub on_select: M, pub on_open: M, pub on_enter: M, pub on_exit: M }
  pub fn list_row<'a,M>(cells: Element<'a,M>, actions: Option<Element<'a,M>>,
                        selected: bool, hovered: bool, events: RowEvents<M>) -> Element<'a,M>;
  pub fn list_header<'a,M>(columns: Vec<(String, Length)>) -> Element<'a,M>;
  pub fn status_dot<'a,M>(state: &ContainerState) -> Element<'a,M>;
  pub fn search_field<'a,M>(id: iced::widget::Id, placeholder: String, value: &str,
                            on_input: impl Fn(String) -> M + 'a) -> Element<'a,M>;
  pub fn chip<'a,M>(label: String, active: bool, on_press: M) -> Element<'a,M>;
  pub struct Shortcut { pub ctrl: bool, pub shift: bool, pub key: &'static str }
  pub fn kbd<'a,M>(s: &Shortcut) -> Element<'a,M>;   // "Ctrl ⇧ P"
  pub fn panel_header<'a,M>(title: Element<'a,M>, actions: Element<'a,M>) -> Element<'a,M>;
  pub fn modal<'a,M>(base: Element<'a,M>, card: Element<'a,M>, on_dismiss: M) -> Element<'a,M>;
  pub enum ToastKind { Error, Warning, Info }
  pub fn toast<'a,M>(kind: ToastKind, text: String, details: Option<(String, bool /*open*/)>,
                     on_toggle: M, on_dismiss: M) -> Element<'a,M>;
  ```
  - `list_row`: `mouse_area` (clique → `on_select`, duplo clique → `on_open`, enter/exit) em
    volta de um `container` com altura `ROW_HEIGHT`, fundo `hover`/`selected`; selecionada ganha
    barra esquerda de 2px `accent`. `actions` só é desenhado se `selected || hovered`.
  - `status_dot`: 8px; running `success`, paused/restarting `warning`, `is_error()` `danger`,
    demais `faint`.
  - `kbd` usa `Shortcut` (definido aqui; a Fase 2.6 reaproveita).
  - `Toast` em `app.rs` ganha `kind: ToastKind` (erros de ação = `Error`).

- [ ] **Passo 1: Testes que falham** (`src/ui/components/mod.rs`):
  - `components_build_in_both_themes` — monta cada componente com `M = ()` dentro de
    `theme(&DARK)` e `theme(&LIGHT)` sem pânico (estilos chamados com `Status::Active`/`Hovered`).
  - `status_dot_colors` — função auxiliar `dot_color(&Palette, &ContainerState) -> Color`:
    Running → `success`, Paused → `warning`, `Exited{code:1}` → `danger`, `Created` e
    `Exited{code:0}` → `faint`.
  - `row_style_selected_beats_hovered` — `style::container::row(true, true)` tem fundo `selected`;
    `row(false, true)` tem `hover`; `row(false,false)` sem fundo.
  - `chip_active_uses_accent_border` — borda do chip ativo é `accent`, inativo é `border`.
  - `kbd_label` — `Shortcut{ctrl:true,shift:true,key:"P"}` vira `"Ctrl ⇧ P"`.
- [ ] **Passo 2:** `cargo test ui::components` → FAIL.
- [ ] **Passo 3:** Implementar; trocar `status_badge`/`confirm_dialog`/toast atuais pelos novos.
- [ ] **Passo 4:** Testes e clippy (com e sem `fake`) → PASS.
- [ ] **Passo 5:** Commit `feat(ui): zed-like styles and base components`.

### Tarefa 2.5.3: Shell em painéis — sidebar, dock e barra de status

**Implementa:** `ui-implementer`.

**Arquivos:**
- Criar: `src/views/status_bar.rs`, `src/views/dock.rs`
- Modificar: `src/app.rs`, `src/views/sidebar.rs`, `src/views/onboarding.rs`,
  `src/views/mod.rs`, `locales/*.yml`

**Interfaces:**
- Consome: Tarefas 2.5.1 e 2.5.2.
- Produz:
  ```rust
  pub enum Screen { Containers, Volumes, Networks }          // remove Detail(String)
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum DockTab { Logs, Stats, Terminal }
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct Dock { pub container_id: String, pub tab: DockTab, pub maximized: bool }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Pane { List, Dock }
  // novos campos em App
  pub sidebar_collapsed: bool, pub dock: Option<Dock>, panes: pane_grid::State<Pane>,
  // novas Message
  ToggleSidebar, OpenDock(String), CloseDock, ToggleDock, DockTab(DockTab), ToggleMaximize,
  PaneResized(pane_grid::ResizeEvent),
  ```
  - `containers::Msg::Open(id)` passa a gerar `OpenDock(id)`.
  - `ToggleDock` sem dock aberto abre o do container selecionado, se houver (seleção vem na
    Tarefa 2.5.4; aqui `ToggleDock` sem dock é no-op).
  - `panes`: dividido na horizontal, lista em cima, dock embaixo com razão inicial `0.6`;
    `PaneResized` atualiza a razão. Sem dock, só a lista ocupa a área. Maximizado: só o dock.
  - Dock: `panel_header` com nome do container, abas (chips), `Maximize2`/`Minimize2` e `X`;
    corpo: estado vazio `t!("dock.empty_tab")` (as Fases 3 e 5 preenchem).
  - Ao recarregar containers, se `dock.container_id` não estiver mais em `items` → `dock = None`
    (sem toast). Em `set_failed()` → `dock = None`.
  - Sidebar: itens com `icon` (Box/Database/Network), nome e contagem (`items.len()` em
    Containers; Volumes/Redes sem contagem até a Fase 4); recolhida mostra só ícones com tooltip.
  - Status bar: altura `STATUS_BAR_HEIGHT`, fundo `surface`, borda superior; esquerda:
    `status_dot`-like de conexão (`success` Ok, `warning` Checking, `danger` Failed) + texto
    (`status.connected`, `status.checking`, ou `friendly_error`) + `status.counts` (`{running}`
    rodando · `{stopped}` parados); direita: `icon_button(PanelLeft → ToggleSidebar)`,
    `icon_button(PanelBottom → ToggleDock)`, botão com `kbd(Ctrl ⇧ P)` sem ação por enquanto
    (a Fase 2.6 liga em `OpenPalette`).
  - Onboarding vira cartão `elevated`, centralizado, largura máx. 520, dentro da área principal;
    sidebar e status bar continuam visíveis.
  - Toasts: coluna no canto inferior direito, acima da status bar, largura 360.

- [ ] **Passo 1: Testes que falham** (`src/app.rs`):
  - `open_dock_from_containers_msg` — `Containers(Open(id))` → `dock == Some(Dock{container_id:id, tab:Logs, maximized:false})`.
  - `close_and_toggle_dock` — `CloseDock` → `None`; `ToggleMaximize` alterna `maximized`;
    `DockTab(Stats)` muda a aba.
  - `dock_closes_when_container_disappears` — dock aberto em `web`, `FakeClient` remove `web`,
    `ReloadContainers` + `Loaded` → `dock == None` e nenhum toast.
  - `connection_loss_closes_dock` — dock aberto, `Pinged(Err(NotRunning))` → `dock == None`.
  - `toggle_sidebar` — duas vezes volta ao original.
  - `view_builds_for_every_layout` — view sem pânico para: sidebar aberta/recolhida × dock
    fechado/aberto/maximizado × `Connection::{Checking, Ok, Failed(NotRunning)}`.
  - Atualizar `view_builds_for_every_screen_and_connection_state` (sem `Detail`).
- [ ] **Passo 2:** `cargo test app::` → FAIL.
- [ ] **Passo 3:** Implementar; novas chaves em `en.yml` e `pt-BR.yml`.
- [ ] **Passo 4:** Testes e clippy (com e sem `fake`) → PASS.
- [ ] **Passo 5:** `cargo run --features fake` (atalhos só chegam na Fase 2.6): pelos botões, recolher a
  sidebar, abrir o dock por duplo clique em um container, arrastar o divisor, maximizar e fechar.
- [ ] **Passo 6:** Commit `feat(ui): paneled shell with sidebar, dock and status bar`.

### Tarefa 2.5.4: Tela de Containers no novo estilo + seleção

**Implementa:** `ui-implementer`.

**Arquivos:**
- Criar: `src/views/toolbar.rs`
- Modificar: `src/views/containers.rs`, `src/app.rs`, `locales/*.yml`

**Interfaces:**
- Consome: Tarefas 2.5.2 e 2.5.3.
- Produz:
  ```rust
  // ContainersState: troca `show_stopped` por
  pub show_running: bool, pub show_stopped: bool,      // ambos true por padrão
  pub selected: Option<String>, pub hovered: Option<String>,
  // containers::Msg ganha
  ToggleRunning, ToggleStopped, Select(String), Hover(Option<String>),
  // remove ToggleStopped(bool) antigo
  pub const SEARCH_ID: &str = "containers-search";
  pub fn toolbar<'a, M>(search: Element<'a,M>, chips: Vec<Element<'a,M>>, count: usize) -> Element<'a,M>;
  ```
  - `visible()`: "Rodando" cobre `Running | Paused | Restarting`; "Parados" cobre o resto.
    Ordenação inalterada (rodando primeiro, depois nome).
  - Colunas: dot + nome (`SANS_MEDIUM`, `Fill`), imagem (`MONO`, `muted`, `FillPortion`),
    portas (`MONO`, `accent`, clicáveis → `OpenPort`), status_text (`faint`, largura fixa),
    ações (`icon_button` Play/Square, RotateCw, Trash2; desabilitadas se `pending`).
    Texto longo corta em uma linha (`wrapping(Wrapping::None)` + largura limitada).
  - `Select(id)` define `selected`; duplo clique → `Open(id)`; `Hover(Some/None)` vindo de
    `RowEvents::on_enter/on_exit`.
  - Após recarga, se `selected` sumiu: seleciona o item que ficou na mesma posição de
    `visible()` (ou o último; `None` se a lista ficou vazia).
  - `ToggleDock` (Tarefa 2.5.3) agora abre o dock do `selected` quando não há dock.
  - Estados vazios: sem containers → `containers.empty`; com containers mas nenhum visível →
    `containers.empty_filtered` (cobre os dois chips desligados e busca sem resultado).
  - Diálogo de remoção usa `modal`.

- [ ] **Passo 1: Testes que falham** (`src/views/containers.rs`, `src/app.rs`):
  - `chips_filter_running_and_stopped` — com `show_running=false` só aparecem parados; com os dois
    `false`, `visible()` vazio.
  - `paused_counts_as_running_chip`.
  - `empty_filtered_state_when_nothing_matches` — view mostra `containers.empty_filtered`.
  - `select_and_double_click_open` — `Select("web")` → `selected`; `Open("web")` → dock aberto.
  - `selection_moves_to_neighbor_when_removed` — lista `[a,b,c]`, selecionado `b`, `b` some →
    seleciona `c`; se era o último, seleciona o novo último.
  - `toggle_dock_opens_selected`.
  - `long_names_do_not_panic` — view com nome de 200 caracteres e 40 portas sem pânico.
  - Atualizar testes que usavam `ToggleStopped(bool)`.
- [ ] **Passo 2:** `cargo test containers app::` → FAIL.
- [ ] **Passo 3:** Implementar; chaves novas (`containers.filter_running`,
  `containers.filter_stopped`, `containers.empty_filtered`, `containers.count`) nos dois locales.
- [ ] **Passo 4:** Testes e clippy (com e sem `fake`) → PASS.
- [ ] **Passo 5:** `LANG=pt_BR.UTF-8 cargo run --features fake`, nos temas claro e escuro: busca,
  chips, hover mostra ações, clique seleciona, duplo clique abre o dock, remover abre o modal.
- [ ] **Passo 6:** Commit `feat(ui): containers screen in zed style with selection`.

---

**Fim de fase:** `phase-reviewer` e parada para verificação manual do humano (ver
[execucao-agentes.md](execucao-agentes.md)).
