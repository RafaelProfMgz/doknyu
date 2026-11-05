# doknyu — Desinstalar pelo app (fase 8)

> Status: design aprovado em conversa. Spec escrita para revisão.
> Data: 2026-11-05

## 1. Objetivo

Quem instalou o doknyu sem saber bem como (um AppImage baixado, um `.deb` de duplo clique) deve
conseguir removê-lo sem procurar comandos. O app descobre como foi instalado e, quando é seguro,
faz a remoção ele mesmo; quando não dá, mostra o comando certo para copiar.

A documentação por canal já existe (`README.md`, seção "Uninstalling", commit da fase 7) e é o
fallback do caso "desconhecido".

### Critérios de sucesso

1. Em AppImage e em pacote nativo (`.deb`, rpm, AUR), o usuário remove o app com um clique (mais a
   senha do sistema no caso de pacote) e o app fecha.
2. Em Flatpak, Nix, Homebrew e `cargo install`, o diálogo mostra o comando exato com botão Copiar.
3. Desinstalar nunca mexe no Docker (containers, imagens, volumes, redes). O diálogo diz isso.
4. Nenhum caminho executa shell com texto interpolado; nenhum teste apaga arquivo real do usuário.

### Fora do escopo

Desinstalar o Docker; remover outras versões/cópias do doknyu além da que está rodando; menu
"Sobre"; tela de Configurações.

## 2. Detecção (`src/install.rs`, sem iced)

```rust
pub enum InstallKind {
    AppImage { path: PathBuf },   // valor de $APPIMAGE
    Deb { package: String },
    Rpm { package: String },
    Pacman { package: String },   // doknyu, doknyu-bin ou doknyu-git
    Flatpak,
    Nix,
    Homebrew,
    Cargo,
    Unknown { exe: PathBuf },
}

pub fn detect(env: &dyn Env, run: &dyn Runner) -> InstallKind;
```

Ordem (a primeira que casa vence):

1. `sandbox::in_flatpak()` → `Flatpak`.
2. `$APPIMAGE` definido e apontando para arquivo existente → `AppImage`.
3. `std::env::current_exe()` (canonicalizado):
   - começa com `/nix/store/` → `Nix`;
   - está sob o prefixo do Homebrew (`/home/linuxbrew/.linuxbrew/` ou `$HOMEBREW_PREFIX`) → `Homebrew`;
   - está em `$CARGO_HOME/bin` (padrão `~/.cargo/bin`) → `Cargo`.
4. Perguntar ao gerenciador de pacotes quem é dono do executável, tentando só os que existem no
   `PATH`, nesta ordem: `dpkg-query -S <exe>` → `Deb`; `rpm -qf --qf %{NAME} <exe>` → `Rpm`;
   `pacman -Qqo <exe>` → `Pacman`. Saída com código ≠ 0 ou vazia: tenta o próximo.
5. Nada casou → `Unknown { exe }`.

- `Env` (variáveis, `current_exe`, existência de arquivo) e `Runner` (executa programa + args,
  devolve código e stdout) são traits para os testes injetarem valores falsos.
- O nome do pacote vindo do gerenciador é validado com `^[a-z0-9][a-z0-9._+-]*$`; se não casar,
  vira `Unknown`. Programas sempre via `Command::new(prog).args([...])`, nunca `sh -c`.

## 3. O que cada caso faz

```rust
pub enum UninstallPlan {
    /// O próprio app remove arquivos do usuário (sem root).
    RemoveFiles { files: Vec<PathBuf> },
    /// Roda via pkexec (janela de senha do sistema).
    Privileged { program: &'static str, args: Vec<String> },
    /// Só mostra o comando (texto exibido e copiado).
    ShowCommand { command: String },
}

pub fn plan(kind: &InstallKind, home: &Path, has_pkexec: bool) -> UninstallPlan;
```

| Caso | Plano |
|---|---|
| `AppImage { path }` | `RemoveFiles`: o arquivo `path` + cada `*.desktop` em `~/.local/share/applications/` cuja linha `Exec=` contém exatamente `path` (integração do AppImageLauncher / Gear Lever). Nenhum outro arquivo. |
| `Deb { package }` | `Privileged`: `pkexec apt-get remove -y <package>` |
| `Rpm { package }` | `Privileged`: `pkexec dnf remove -y <package>` |
| `Pacman { package }` | `Privileged`: `pkexec pacman -R --noconfirm <package>` |
| os três acima sem `pkexec` no `PATH` | `ShowCommand` com o mesmo comando usando `sudo` |
| `Flatpak` | `ShowCommand`: `flatpak uninstall --delete-data io.github.rafaelprofmgz.Doknyu` |
| `Nix` | `ShowCommand`: `nix profile remove doknyu`, com nota sobre NixOS/Home Manager |
| `Homebrew` | `ShowCommand`: `brew uninstall doknyu` |
| `Cargo` | `ShowCommand`: `cargo uninstall doknyu` |
| `Unknown { exe }` | `ShowCommand` vazio: mostra o caminho do executável e o link para a seção "Uninstalling" do README |

Remover o AppImage em execução é seguro no Linux: o squashfs já está montado; o app fecha logo
depois.

### Preferências

Checkbox "Apagar também minhas preferências" (marcado por padrão), mostrando o caminho de
`prefs::config_dir()`. Em `RemoveFiles` e `Privileged`, se marcado, o diretório é removido **depois**
que a remoção principal deu certo. Em `ShowCommand`, o app apaga as preferências ao clicar
"Apagar preferências e fechar" (o Flatpak não precisa: `--delete-data` já cobre; o checkbox some).

## 4. Interface

### Onde fica

- `ActionId::Uninstall` na command palette: "Desinstalar doknyu…".
- Rodapé da sidebar: entrada discreta (ícone Lucide `trash-2`, texto `muted`) presa ao fim da
  coluna, abaixo das telas. Na sidebar recolhida, só o ícone com tooltip. Sem atalho de teclado.

### Diálogo (`views/uninstall.rs`, usando `ui::components::modal`)

- Título "Desinstalar doknyu".
- Linha de detecção: "Instalado via AppImage em `~/Apps/doknyu.AppImage`" / "via pacote `.deb`
  (`doknyu`)" / "via Flatpak" / "Não foi possível detectar como o doknyu foi instalado".
- Aviso fixo: "Seus containers, imagens, volumes e redes do Docker não serão afetados."
- Checkbox de preferências (seção 3).
- Rodapé conforme o plano:
  - `RemoveFiles` / `Privileged`: Cancelar + botão destrutivo "Desinstalar".
  - `ShowCommand`: bloco `MONO` com o comando + botão Copiar (como o onboarding), e Fechar.
- Estados: `Idle` → `Running` (spinner, botões desabilitados, diálogo não fecha) → sucesso (o app
  sai com `iced::exit()`) ou erro (volta a `Idle` com a mensagem dentro do diálogo).

### Erros

- `pkexec` com código 126 (usuário cancelou a senha) ou 127 (não autorizado): toast informativo
  "Desinstalação cancelada"; nada muda.
- Outro código ≠ 0: mensagem no diálogo com as últimas linhas do stderr.
- Falha ao apagar arquivo (permissão, arquivo sumiu): mensagem no diálogo; arquivos já apagados
  não são restaurados, e o texto diz quais ficaram.

## 5. Arquitetura e fluxo

- `install.rs`: `detect` + `plan` (puros, testáveis).
- `uninstall` executor: função assíncrona que recebe o `UninstallPlan` e um `Runner`/fs injetáveis,
  chamada via `Task::perform`; devolve `Result<(), UninstallError>`.
- A detecção roda uma vez, ao abrir o diálogo (pode chamar `dpkg-query` etc.), não no startup.
- `app.rs`: `Message::Uninstall(uninstall::Msg)`, estado `Option<UninstallState>`; mesmo padrão
  dos diálogos de volumes.

## 6. i18n

Chaves `uninstall.*` (título, linhas de detecção por tipo, aviso do Docker, checkbox, botões,
erros, toast) e `actions.uninstall` em `locales/en.yml` e `locales/pt-BR.yml`.

## 7. Testes

- `detect`: um teste por caso, com `Env`/`Runner` falsos (Flatpak vence AppImage; `$APPIMAGE`
  para arquivo inexistente é ignorado; `/nix/store`; prefixo do brew; `~/.cargo/bin`; dpkg falha e
  rpm responde; nome de pacote inválido → `Unknown`; nada casa → `Unknown`).
- `plan`: tabela da seção 3, incluindo sem `pkexec` → `ShowCommand` com `sudo`, e AppImage
  pegando só `.desktop` cujo `Exec=` contém o caminho exato (um `.desktop` de outro app com nome
  parecido fica de fora), em diretório temporário.
- Executor: `RemoveFiles` em diretório temporário; `Privileged` com `Runner` falso devolvendo 0,
  126 e 1; preferências só apagadas depois do sucesso.
- `update`: abrir/fechar diálogo; não fecha durante `Running`; 126 vira toast; sucesso produz a
  task de saída.
- Nenhum teste chama `pkexec`, gerenciador de pacotes real ou apaga fora de diretório temporário.
