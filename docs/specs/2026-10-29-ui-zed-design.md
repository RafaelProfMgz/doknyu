# doknyu — Redesign da interface inspirado no Zed

> Complementa [`2026-10-29-doknyu-design.md`](2026-10-29-doknyu-design.md). Onde as duas
> divergirem sobre interface, vale esta.

## 1. Objetivo

Deixar o doknyu com cara de ferramenta moderna, minimalista e funcional, no espírito do editor
Zed: visual sóbrio e denso, layout em painéis, command palette e uso completo pelo teclado.

**Critérios de sucesso**
- Nenhuma view usa cor, espaçamento ou tamanho de fonte literal: tudo vem de `ui::tokens`.
- O app segue o tema claro/escuro do sistema e reage à troca com ele aberto.
- Toda ação disponível na UI também está na command palette e tem atalho quando faz sentido.
- Dá para listar, filtrar, selecionar, iniciar/parar/reiniciar/remover containers e trocar de
  tela sem tocar no mouse.
- As Fases 3–5 constroem suas telas com os componentes de `ui::components` e registram suas
  ações no registro central, sem estilo próprio.

**Fora de escopo:** barra de título própria (fica a nativa), seletor de vários temas, temas do
usuário, atalhos configuráveis.

**Quando:** duas fases novas, **2.5** (visual e layout) e **2.6** (teclado e palette), antes da
Fase 3.

## 2. Sistema visual

### Módulos
- `src/ui/tokens.rs` — `Palette` (cores), `SPACE_*` (múltiplos de 4), `RADIUS` (4) e
  `RADIUS_LG` (6), `TEXT_SM` (12), `TEXT` (13), `TEXT_LG` (15), `ROW_HEIGHT` (28),
  `STATUS_BAR_HEIGHT` (24), `SIDEBAR_WIDTH` (200), `SIDEBAR_COLLAPSED` (40).
- `src/ui/theme.rs` — `ThemeMode { System, Dark, Light }`, `fn theme(resolved: iced::theme::Mode) -> Theme`
  via `Theme::custom`, e `fn palette(theme: &Theme) -> &'static Palette`.
- `src/ui/style.rs` — funções de estilo: `button::{ghost, primary, danger, chip(active)}`,
  `container::{bg, surface, elevated, row(selected, hovered)}`, `text_input::field`,
  `scrollable::thin`, `rule::border`.
- `src/ui/fonts.rs` — bytes embutidos (`include_bytes!`) e `const SANS`, `SANS_MEDIUM`,
  `SANS_SEMIBOLD`, `MONO`, `ICONS: Font`.
- `src/ui/icons.rs` — `enum Icon { Box, Database, Network, Play, Square, RotateCw, Trash2,
  Search, PanelLeft, PanelBottom, Maximize2, X, ExternalLink, Command, ... }` e
  `fn icon<'a, M>(Icon) -> Text<'a>` (glifo da fonte Lucide).

### Paleta (One Dark / One Light do Zed)

| Token | Escuro | Claro |
|---|---|---|
| `bg` | `#282c33` | `#fafafa` |
| `surface` | `#2f343e` | `#ebebec` |
| `elevated` | `#2f343e` | `#fafafa` |
| `border` | `#363c46` | `#dcdcdd` |
| `hover` | `#363c46` | `#dfdfe0` |
| `selected` | `#454a56` | `#d3d3d4` |
| `text` | `#dce0e5` | `#242529` |
| `muted` | `#a9afbc` | `#58585a` |
| `faint` | `#878a98` | `#7e7e83` |
| `accent` | `#74ade8` | `#5c78e2` |
| `success` | `#a1c181` | `#669f59` |
| `warning` | `#dec184` | `#a48819` |
| `danger` | `#d07277` | `#d36151` |

### Regras
- Separação por bordas de 1px (`border`), não por sombras. Sombra leve só em `elevated`
  (palette, modal, toast).
- Raio 4px; 6px em modal e palette.
- Cor forte só para estado (bolinha), foco/seleção (`accent`) e links.
- Fontes: IBM Plex Sans (Regular, Medium, SemiBold) para a UI e IBM Plex Mono para IDs,
  imagens, portas e logs. Ícones: fonte Lucide. Licenças (OFL e ISC) ficam em
  `assets/fonts/LICENSES.md`. Peso total embutido ≈ 700 KB.

### Componentes (`src/ui/components/`)
Cada um é uma função pura que devolve `Element`; nenhum guarda estado.

| Componente | Uso |
|---|---|
| `icon_button(icon, msg, tooltip)` | Ações por linha, barra de status, cabeçalho do dock |
| `list_row(cells, selected, hovered, on_select, on_open, on_hover)` | Linhas de containers, volumes, redes (`mouse_area` com clique, duplo clique e enter/exit) |
| `list_header(columns)` | Cabeçalho discreto em `faint`, `TEXT_SM` |
| `status_dot(&ContainerState)` | Bolinha 8px; cores: running `success`, paused/restarting `warning`, erro `danger`, demais `faint` |
| `search_field(id, value, on_input)` | Campo com ícone de lupa, sem moldura pesada; borda `accent` com foco |
| `chip(label, active, msg)` | Filtros que ligam/desligam |
| `kbd(&Shortcut)` | Dica de atalho (`Ctrl ⇧ P`) |
| `panel_header(title, actions)` | Topo do dock e de seções |
| `modal(content, on_dismiss)` | Substitui `confirm_dialog`; fundo escurecido, card `elevated` |
| `toast(&Toast)` | Compacto, borda esquerda colorida pelo tipo |

## 3. Layout

```
┌──────────────┬─────────────────────────────────────────────────────┐
│ ▣ Containers 8│ 🔍 Buscar…            [Rodando] [Parados]   8 itens │
│ ⛁ Volumes   3 ├─────────────────────────────────────────────────────┤
│ ⇄ Redes     4 │ ● web        nginx:1.27     8080 ↗   2h   ▶ ■ ↻ 🗑  │
│               │ ● db         postgres:16    5432     2h             │
│               ├─────────────────────────────────────────────────────┤
│               │ web   Logs │ Stats │ Terminal                  ⤢ ✕  │
│               │ …                                                   │
├───────────────┴─────────────────────────────────────────────────────┤
│ ● Docker conectado   3 rodando · 5 parados         ⊟ ⊞    Ctrl⇧P    │
└──────────────────────────────────────────────────────────────────────┘
```

- **Sidebar** (`views/sidebar.rs`): Containers, Volumes, Redes com ícone, nome e contagem;
  ativo com fundo `selected`. Recolhível (Ctrl+B) para 40px só com ícones.
- **Barra da tela** (`views/toolbar.rs`): `search_field`, chips de filtro, contagem à direita.
  Containers: chips *Rodando* e *Parados* (substituem "mostrar parados"; os dois ligados por padrão).
- **Lista:** `list_header` + `list_row`s. Nome em Medium; imagem e portas em Mono `muted`;
  portas como links `accent`. Ações (`icon_button`) só aparecem na linha com hover ou selecionada.
- **Dock inferior** (`views/dock.rs`): substitui a tela de detalhe. Enter ou duplo clique no
  container selecionado abre o dock com abas Logs | Stats | Terminal. Lista e dock dividem a área
  via `pane_grid` (divisor arrastável; altura inicial 40%). `⤢` maximiza, `✕`/Esc fecha,
  Ctrl+J alterna. Na Fase 2.5 as abas mostram um estado vazio; as Fases 3 e 5 as preenchem.
  Se o container sumir, o dock fecha.
- **Barra de status** (`views/status_bar.rs`): à esquerda a conexão (bolinha + texto: conectado,
  verificando, parado/sem permissão/…) e contagens; à direita botões de sidebar e dock e a dica
  `Ctrl⇧P` (clicável, abre a palette).
- **Onboarding:** cartão centralizado na área principal; sidebar e barra de status continuam.
- **Toasts:** canto inferior direito, acima da barra de status; máximo 5 (já implementado).

### Mudanças de estado no `App`
- `Screen` perde `Detail(String)`; passa a ser `Containers | Volumes | Networks`.
- Novos campos: `theme_mode: ThemeMode`, `system_mode: iced::theme::Mode`,
  `sidebar_collapsed: bool`, `selected: Option<String>` (id do container),
  `hovered: Option<String>`, `dock: Option<Dock>`, `panes: pane_grid::State<Pane>`.
- `pub struct Dock { pub container_id: String, pub tab: DockTab, pub maximized: bool }`,
  `pub enum DockTab { Logs, Stats, Terminal }`.
- Tema: `App::theme()` resolve `theme_mode` + `system_mode`; a `Subscription` inclui
  `iced::system::theme_changes()` e o `App::new` dispara `iced::system::theme()`. `Mode::None`
  (sistema sem preferência) resolve para escuro.

## 4. Teclado e command palette

### Registro de ações (`src/actions.rs`)
- `pub enum ActionId` — `GoContainers`, `GoVolumes`, `GoNetworks`, `ToggleSidebar`,
  `ToggleDock`, `FocusSearch`, `OpenPalette`, `SelectNext`, `SelectPrev`, `OpenSelected`,
  `StartStopSelected`, `RestartSelected`, `RemoveSelected`, `ThemeSystem`, `ThemeDark`,
  `ThemeLight`, `Retry`. As Fases 3–5 acrescentam as suas (ex.: `DockLogs`, `CopyLogs`).
- `pub struct ActionDef { pub id: ActionId, pub label_key: &'static str, pub shortcut: Option<Shortcut>, pub in_palette: bool }`
  e `pub fn all() -> &'static [ActionDef]`.
- `pub fn available(app: &App, id: ActionId) -> bool` — ex.: ações de container exigem
  conexão ok e seleção; `Retry` só no onboarding.
- `App::run_action(id) -> Task<Message>` é o único caminho: atalhos, palette e botões chamam
  `Message::Action(ActionId)`.

### Atalhos
| Atalho | Ação |
|---|---|
| Ctrl+Shift+P | Abrir palette |
| Ctrl+1 / 2 / 3 | Containers / Volumes / Redes |
| Ctrl+B / Ctrl+J | Sidebar / dock |
| Ctrl+F ou `/` | Focar busca |
| ↑ ↓ ou `k` `j` | Selecionar linha |
| Enter | Abrir dock do selecionado |
| `s` / `r` / Delete | Iniciar-parar / reiniciar / remover (com confirmação) |
| Esc | Fecha, nesta ordem: palette → modal → dock → limpa busca e volta o foco à lista |

- Captura via `iced::event::listen_with`. Uma função pura
  `keymap::resolve(key, modifiers, captured: bool) -> Option<ActionId>` decide; teclas sem
  modificador (`j`, `s`, `/`…) são ignoradas quando o evento foi capturado por um `text_input`
  (status `Captured`), para não disparar ao digitar.
- **Foco visível:** a linha selecionada tem fundo `selected` e uma barra de 2px `accent` à
  esquerda. A seleção é mantida por id; se o container sumir, passa para a linha seguinte.

### Command palette (`views/palette.rs`)
- Overlay `elevated`, 560px, no topo e centralizado; `text_input` com foco automático
  (`text_input::focus`).
- Lista as ações `in_palette && available`, filtradas por `fuzzy::score(query, label)` (subsequência
  com bônus para início de palavra e letras consecutivas; implementação própria, sem crate) e
  ordenadas pela pontuação. Cada item mostra rótulo traduzido e `kbd` do atalho.
- ↑ ↓ movem, Enter executa e fecha, Esc fecha. Clique também executa.

### Preferência de tema
- Ações `ThemeSystem/Dark/Light` na palette. A escolha é salva em
  `$XDG_CONFIG_HOME/doknyu/theme` (padrão `~/.config/doknyu/theme`) com o texto
  `system|dark|light`; falha ao ler/gravar só gera `tracing::warn`, nunca toast.

## 5. Testes

- `ui::theme`: os dois temas são construídos; `palette()` devolve a paleta certa para cada um.
- Componentes: cada função monta um `Element` sem pânico para todos os estados (como os testes de
  view atuais).
- `update` com `FakeClient`: seleção (setas, j/k, clique, sumiço do container), dock (abrir,
  fechar, alternar, fechar ao sumir), sidebar, tema do sistema mudando, `run_action` para cada
  `ActionId` disponível/indisponível.
- `keymap::resolve`: tabela de casos, incluindo teclas ignoradas com `captured = true`.
- `fuzzy::score`: ordem esperada para consultas típicas ("stp" → "Parar container").
- Palette: filtrar, navegar, executar, fechar.
- Preferência de tema: leitura/gravação em diretório temporário; arquivo inválido vira `System`.

## 6. Impacto nas fases seguintes

- **Fase 3 (Logs/Stats):** preenche as abas do dock em vez de uma tela de detalhe; usa Mono e
  tokens; registra `DockLogs`, `DockStats`, `ToggleTimestamps`, `CopyLogs`, `FocusLogSearch`.
- **Fase 4 (Volumes/Redes):** reaproveita toolbar, `list_row` e ações no hover; registra
  `RemoveSelected` específico e `PruneVolumes`.
- **Fase 5 (Terminal):** aba Terminal do dock; o `iced_term` usa a fonte Mono e as cores da paleta.
- **Fase 6:** novas chaves de i18n já entram em `en` e `pt-BR` nas Fases 2.5/2.6.
- Os planos dessas fases serão ajustados ao escrever o plano das Fases 2.5 e 2.6.
