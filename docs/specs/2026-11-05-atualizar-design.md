# doknyu — Atualizar pelo app (fase 8)

> Status: design aprovado em conversa. Spec escrita para revisão.
> Data: 2026-11-05
> Depende de: `2026-11-05-desinstalar-design.md` (reaproveita `install::detect`, `Runner` e o
> tratamento do `pkexec`).

## 1. Objetivo

Um botão "Procurar atualização…" que descobre como o doknyu foi instalado e faz a atualização
pelo caminho correspondente: o próprio app baixa e instala quando o canal é a release do GitHub
(AppImage, `.deb`); delega ao gerenciador quando dá para rodar como root (`dnf`); e mostra o
comando certo nos demais.

### Critérios de sucesso

1. AppImage e `.deb`: com versão nova no GitHub, um clique baixa, confere o hash, instala e
   reinicia o app na versão nova.
2. COPR: um clique (mais a senha) roda o `dnf upgrade` do pacote.
3. AUR, Flatpak, Nix, Homebrew, Cargo: o diálogo mostra o comando exato com botão Copiar.
4. O app só acessa a rede quando o usuário clica, e só nos casos que precisam (seção 3).
5. Download corrompido nunca substitui o app instalado.

### Fora do escopo

Verificação automática no startup; selo "atualização disponível"; canal beta/pre-release;
assinatura das releases (ver seção 7); downgrade; outras arquiteturas além de x86_64 (as releases
só têm x86_64).

## 2. Versão mais nova (`src/update.rs`, sem iced)

- `GET https://api.github.com/repos/RafaelProfMgz/doknyu/releases/latest`, cabeçalhos
  `User-Agent: doknyu/<CARGO_PKG_VERSION>` e `Accept: application/vnd.github+json`. Nenhum dado
  do usuário. O endpoint `latest` já exclui drafts e pre-releases.
- Da resposta: `tag_name`, `html_url`, `body` (notas) e `assets[]` (`name`,
  `browser_download_url`, `size`).
- Versão: `tag_name` sem o `v` inicial, parse de `X.Y.Z` em três `u64`, comparação lexicográfica
  com `CARGO_PKG_VERSION`. Tag que não casar com `X.Y.Z` → erro "release com versão inválida".
- Cliente HTTP: `ureq` (3.x, TLS com rustls, sem OpenSSL), chamado via `tokio::task::spawn_blocking`
  dentro de `Task::perform`. Hash: crate `sha2`. São as únicas dependências novas. Versões exatas
  fixadas no plano.
- Trait `Http` (`get_json`, `download(url, dest, on_progress)`) para os testes usarem um fake.

## 3. Plano por tipo de instalação

```rust
pub enum UpdatePlan {
    /// Consulta o GitHub; se houver versão nova, baixa o asset e instala.
    SelfUpdate { asset: &'static str, install: SelfInstall },
    /// Roda via pkexec; o gerenciador decide se há versão nova.
    Privileged { program: &'static str, args: Vec<String> },
    /// Só mostra o comando.
    ShowCommand { command: String },
    /// Consulta o GitHub e, se houver versão nova, oferece abrir a página da release.
    OpenRelease,
}
pub enum SelfInstall { ReplaceAppImage { path: PathBuf }, AptInstall }
```

| Instalação (`InstallKind`) | Plano | Rede |
|---|---|---|
| `AppImage { path }` | `SelfUpdate { asset: "doknyu-x86_64.AppImage", ReplaceAppImage }` | sim |
| `Deb { .. }` | `SelfUpdate { asset: "doknyu_<versão>-1_amd64.deb", AptInstall }` | sim |
| `Rpm { package }` | `Privileged`: `pkexec dnf upgrade -y <package>` | não (o dnf usa a dele) |
| `Pacman { .. }` | `ShowCommand`: `paru -Syu` se `paru` existe, senão `yay -Syu`, senão `sudo pacman -Syu` (cobre o AUR e o repositório pacman `[doknyu]` do `RafaelProfMgz/pacman-tap`) | não |
| `Flatpak` | `ShowCommand`: `flatpak update io.github.rafaelprofmgz.Doknyu` (o sandbox não tem permissão de rede, e nem precisa) | não |
| `Nix` | `ShowCommand`: `nix profile upgrade doknyu`, com nota sobre NixOS/Home Manager | não |
| `Homebrew` | `ShowCommand`: `brew upgrade doknyu` | não |
| `Cargo` | `ShowCommand`: `cargo install --git https://github.com/RafaelProfMgz/doknyu --locked --force` | não |
| `Unknown { .. }` | `OpenRelease` | sim |

`Rpm` sem `pkexec` → `ShowCommand` com `sudo`, como no Desinstalar. O nome do asset `.deb` é
montado com a versão da release (sem o `v`) mais a revisão Debian `-1` (o `cargo deb` gera
`doknyu_0.1.0-1_amd64.deb`) e procurado em `assets[]`; asset ausente → erro
"a release não tem o arquivo para esta instalação".

## 4. Download e instalação (`SelfUpdate`)

1. Baixar `SHA256SUMS` da mesma release e achar a linha do asset (`<hash>  <nome>`). Sem linha →
   erro, nada é baixado.
2. Baixar o asset para o destino temporário, calculando o SHA-256 em streaming e reportando o
   progresso (`bytes / size`).
3. Hash diferente → apagar o temporário e parar com erro. O app instalado não é tocado.
4. Instalar:
   - `ReplaceAppImage { path }`: o temporário é criado **no mesmo diretório** de `path`
     (`.doknyu-update-<pid>.AppImage`, para o `rename` ser atômico no mesmo sistema de arquivos);
     `chmod 0755`; `rename(temp, path)`. Diretório sem permissão de escrita → erro antes de baixar
     ("O doknyu não pode escrever em <dir>. Mova o AppImage para uma pasta sua ou baixe a nova
     versão manualmente"), com botão para abrir a release.
   - `AptInstall`: temporário em `$XDG_CACHE_HOME/doknyu/` (padrão `~/.cache/doknyu/`);
     `pkexec apt-get install -y <caminho absoluto>`; o arquivo é apagado depois, com sucesso ou não.
5. Reiniciar: `Command::new(<novo executável>)` desacoplado (AppImage: `path`; `.deb`: o
   `current_exe()` original), depois `iced::exit()`. Se não der para subir o novo processo, o app
   avisa "Atualizado. Abra o doknyu de novo." e fecha mesmo assim.

## 5. Interface

### Onde fica

- `ActionId::CheckUpdate` na command palette: "Procurar atualização…".
- Rodapé da sidebar, logo acima de "Desinstalar doknyu…", mesmo estilo (ícone Lucide
  `refresh-cw`). Sem atalho de teclado.

### Diálogo (`views/update.rs`, `ui::components::modal`)

Estados:

- `Checking`: "Procurando atualização…" (só nos planos com rede). Para `Privileged` e
  `ShowCommand`, o diálogo abre direto no estado final.
- `UpToDate`: "Você já está na versão mais recente (0.1.0)." + Fechar.
- `Available { current, latest, notes }`: "0.1.0 → 0.2.0", notas da release em texto puro com
  rolagem (até 12 linhas visíveis), botão primário "Atualizar e reiniciar" (ou "Abrir página da
  release" em `OpenRelease`) + Agora não.
- `Downloading { done, total }`: barra de progresso; botões desabilitados; o diálogo não fecha.
- `Installing`: spinner ("Instalando…"); para `.deb`/`dnf`, a janela de senha do sistema aparece.
- `Command { text }`: bloco `MONO` + Copiar + Fechar (planos `ShowCommand`).
- `Error { message }`: mensagem + Tentar de novo + Fechar.

`Privileged` (dnf) mostra Cancelar + "Atualizar" antes de chamar o `pkexec`; ao final, se o dnf
não mudou nada, mostra `UpToDate`; se mudou, reinicia como na seção 4.5. Para distinguir, o app
compara a versão do pacote antes e depois (`rpm -q --qf %{VERSION} <package>`).

## 6. Erros

| Situação | Comportamento |
|---|---|
| Sem rede / DNS / TLS | `Error`: "Não foi possível falar com o GitHub." |
| HTTP 403/429 (rate limit) | `Error`: "O GitHub limitou as consultas; tente de novo mais tarde." |
| Release sem o asset ou sem linha no `SHA256SUMS` | `Error`, nada é baixado |
| Hash diferente | `Error`: "O arquivo baixado não confere; nada foi alterado." Temporário apagado |
| Sem escrita no diretório do AppImage | `Error` antes do download, com botão para a release |
| `pkexec` 126/127 | Toast "Atualização cancelada"; o diálogo volta ao estado anterior |
| `apt-get`/`dnf` com outro código | `Error` com as últimas linhas do stderr |

## 7. Segurança e limite conhecido

- Só HTTPS; o `ureq` segue redirecionamentos do GitHub para `objects.githubusercontent.com`.
- Programas sempre via `Command::new(prog).args([...])`; nenhum `sh -c`. Nomes de pacote já
  validados pelo `install::detect`.
- **Limite:** o `SHA256SUMS` vem da mesma release, então protege contra download corrompido ou
  truncado, mas **não** contra quem consiga publicar uma release na conta do GitHub. A defesa é
  assinar as releases (ex.: minisign, chave pública embutida no binário) e verificar a assinatura
  do `SHA256SUMS` antes de usá-lo. Fica para uma fase futura; até lá este risco é aceito.

## 8. i18n

Chaves `update.*` (estados, erros, botões, toast) e `actions.check_update` em `locales/en.yml` e
`locales/pt-BR.yml`.

## 9. Testes

- Versão: `0.1.0 < 0.2.0`, `0.10.0 > 0.9.0`, igual → sem atualização, tag `v` opcional, tag
  inválida → erro.
- Parse do JSON da release (fixture) e do `SHA256SUMS` (linha certa, ausente, formato com `*`).
- `plan`: tabela da seção 3, incluindo `Rpm` sem `pkexec` e escolha `paru`/`yay`/`makepkg`.
- `ReplaceAppImage` em diretório temporário: sucesso troca o arquivo e mantém `0755`; hash errado
  deixa o arquivo original byte a byte igual; diretório sem escrita falha antes de chamar `Http`.
- `AptInstall` e `dnf` com `Runner` falso (0, 126, 1); `.deb` temporário apagado em todos os casos.
- `update` do app com `Http` e `Runner` falsos: transições de estado, diálogo não fecha durante
  `Downloading`/`Installing`, 126 vira toast, sucesso produz a task de saída.
- Nenhum teste acessa a rede, roda `pkexec` ou escreve fora de diretório temporário.
