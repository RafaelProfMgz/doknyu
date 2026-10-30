# 0002 — Flatpak (Flathub): spike da Tarefa 7.1

Data: 2026-10-30. Status: **parcial** (spike interrompido por limite de turnos; ver "Pendente").
Origem: Tarefa 7.1 (spike descartável).

## Decisão (provisória)

**Seguir para a 7.2, mas ajustando o app antes (ou junto):** o doknyu roda no sandbox com as
permissões do Flathub (`--filesystem=/run/docker.sock` + `--filesystem=xdg-run/docker.sock`, sem
`--share=network`, sem `host`/`home`, sem `flatpak-spawn`). Três ajustes de código são necessários:

1. **Procurar o socket rootless sozinho.** Sem `DOCKER_HOST`, o app só tenta `/var/run/docker.sock`;
   o `$XDG_RUNTIME_DIR/docker.sock` (liberado por `xdg-run/docker.sock`) nunca é tentado.
2. **Mensagem certa quando não há socket.** Dentro do sandbox o `docker` nunca está no `PATH`, então
   `classify_connect_error` devolve sempre `NotInstalled` quando o socket falta (onboarding "Docker
   não instalado", errado para quem tem Docker rootless sem `DOCKER_HOST`).
3. **Terminal embutido não funciona no Flatpak:** ele roda `docker exec -it` (ADR 0001) e a CLI
   `docker` não existe no runtime. Opções para a 7.2: (a) modo `--exec-bridge` no próprio binário
   (bollard, sem permissão nova; era a "opção futura" do ADR 0001); (b) empacotar a CLI (Go, build
   from source; pesado); (c) esconder a aba Terminal no Flatpak, como o terminal externo. A (c) é o
   mínimo para publicar a 0.1; (a) é o caminho bom.

## Evidência (verificada de fato)

Ambiente: Flatpak 1.16.1, Platform/Sdk 25.08, host Wayland (RADV), usuário no grupo `docker`.
Instalação `--user`; build offline (sem rede no build) com `cargo-sources.json` gerado por
`flatpak-cargo-generator.py` (flatpak-builder-tools commit `41c20aa1…`, sha256 do script
`0a2db6be…`, rodado com `python -I` num venv só com `aiohttp` e `tomlkit`; 1029 entradas, todas
do crates.io, nenhuma fonte git). Foi preciso `--disable-rofiles-fuse` (flatpak-builder dentro do
Flatpak não consegue `fusermount`); diretórios de build/estado têm de ficar sob `$HOME`, não `/tmp`.

| Pergunta | Resultado |
|---|---|
| Build e instalação | `flatpak run org.flatpak.Builder --user --install ...` exit 0 com o manifesto em `packaging/flatpak/`. |
| Grupo `docker`, rootful | Binário de sondagem no sandbox: `ping=Ok(())`, `list_containers=Ok(10 items)` (inclui o container `doknyu-spike-a`). O app GUI (`RUST_LOG=info,bollard=debug`, 10 s) criou a janela em Wayland (adaptador RADV Vulkan), e o log do bollard mostra `_ping`, `events`, `containers/json`, `volumes`, `networks` e a resposta com os containers. |
| `/var/run/docker.sock` é symlink? | No host, `/var/run -> /run`. No sandbox, `/var/run -> ../run`. Liberar `/run/docker.sock` basta, o app usa `/var/run/docker.sock` e resolve. Dentro do sandbox o socket aparece como `nfsnobody:nfsnobody` (gids sem mapa) e o grupo `docker` do usuário aparece como `65534`; mesmo assim o `connect` funciona (o kernel usa o gid real). |
| Fora do grupo `docker` | Simulado (não dá para sair do grupo sem root): `socat` criou `/run/user/1000/docker.sock` com **modo 000**, exposto via `xdg-run/docker.sock` + `DOCKER_HOST`. Resultado: `ping=Err(PermissionDenied)` e `list_containers=Err(PermissionDenied)`, ou seja, o EACCES do kernel passa pelo sandbox e o app mostra a tela de permissão negada. |
| Socket ausente (`--nofilesystem=/run/docker.sock`, sem `DOCKER_HOST`) | `Err(NotInstalled)` (ver ajuste 2). |
| Docker rootless sem `DOCKER_HOST` | Socket `socat` (modo 600) em `$XDG_RUNTIME_DIR/docker.sock`, sem `/run/docker.sock`, sem `DOCKER_HOST`: `Err(NotInstalled)`. O app não tenta esse caminho (ajuste 1). |
| Rootless com `DOCKER_HOST=unix:///run/user/1000/docker.sock` | `ping=Ok`, `list_containers=Ok(10)`. |
| `DOCKER_HOST` do host | Não entra no sandbox. Configurar com `flatpak override --user --env=DOCKER_HOST=unix:///run/user/1000/docker.sock io.github.rafaelprofmgz.Doknyu` (conferido: o `sh` do sandbox vê a variável; override removido depois) ou pelo Flatseal. Documentar em `packaging/README.md`. |
| `DOCKER_HOST=tcp://...` | Ignorado pelo app (cai no socket padrão e conecta: `ping=Ok`). Comportamento anterior ao Flatpak, vale registrar. |
| Onde fica `theme` | `XDG_CONFIG_HOME=/home/angel/.var/app/io.github.rafaelprofmgz.Doknyu/config`; a sondagem imprimiu `config_dir=.../.var/app/<id>/config/doknyu` e a escrita de `theme` ali funcionou (apagado depois). Nada escrito em `~/.config/doknyu`. Nenhuma mudança de código necessária. |
| Detectar Flatpak | `/.flatpak-info` existe no sandbox (`in_flatpak=true` na sondagem). |
| Terminal externo | Nenhum emulador no `PATH` do sandbox (`xterm`, `gnome-terminal`, `konsole`, `kitty`, `alacritty`, `foot`, `x-terminal-emulator` ausentes); `flatpak-spawn` existe no runtime, mas sem `--talk-name=org.freedesktop.Flatpak` não sai do sandbox (e o Flathub emite aviso `finish-args-flatpak-spawn-access`). Esconder a ação (7.2). |
| Terminal embutido | `docker` ausente do `PATH` (`which: no docker in (/app/bin:/usr/bin)`; sondagem `docker_cli_in_path=false`), então a aba cai no aviso de CLI ausente (ajuste 3). O PTY em si não foi testado. |
| Fallback X11 | `--nosocket=wayland`: log `Using X11 platform`, adaptador selecionado. |
| Portal Settings | De dentro do sandbox, `gdbus call ... Settings.Read org.freedesktop.appearance color-scheme` devolve `uint32 1`, igual ao host; o iced (mundy) usa essa interface. Log do iced: `System theme: Dark`. |
| Portal OpenURI | Interface `org.freedesktop.portal.OpenURI` presente no bus visto do sandbox; `/usr/bin/xdg-open` do runtime tem o ramo do portal; o app chama `open::that_detached` (xdg-open). Não cliquei num link. |
| Linter (`flatpak-builder-lint`) | `manifest`: só o aviso `runtime-update-available-to-org.freedesktop.Platform-26.08` (a 26.08 já existe hoje; considerar na 7.2). Nenhum erro de `finish-args`. `repo`: erros `appstream-missing-screenshots` e `appstream-screenshots-not-mirrored-in-ostree`, porque as URLs `v0.1.0/...png` dão 404 (tag ainda não existe); somem com a release. |

## Precedentes no Flathub (Passo 4)

Manifestos lidos em `raw.githubusercontent.com/flathub/<app>/master/`:

- `com.github.sdv43.whaler` (GUI de Docker): `--filesystem=/run/docker.sock`,
  `--filesystem=xdg-run/docker.sock`, `--filesystem=/run/podman/podman.sock`,
  `--filesystem=xdg-run/podman/podman.sock`, mais só `ipc`, wayland e x11. É o caso mais próximo do nosso.
- `com.github.marhkb.Pods` (Rust/GTK, gerenciador de containers): `--filesystem=/run/docker.sock`,
  `--filesystem=xdg-run/podman:ro`.
- `io.github.buonhobo.Katharsis`: `--filesystem=/run/docker.sock`.
- `io.podman_desktop.PodmanDesktop`: `--filesystem=/run/docker.sock`, `xdg-run/podman:create`, mas
  com `home`, `host-os:ro` e `org.freedesktop.Flatpak` (permissões bem mais largas que as nossas).
- `https://flathub.org/api/v2/exceptions/<id>`: Whaler e Pods só têm exceções antigas do linter
  (`appid-uses-code-hosting-domain`), nenhuma ligada ao socket; Podman Desktop não tem exceção.
- Contra-exemplo: PR `flathub/dev.zed.Zed#340` ("Enable access to Docker & Podman socket"), fechado
  sem merge; o revisor comentou que bastam `xdg-run/docker` e `xdg-run/podman`; o autor do teste
  ainda via "docker CLI not found on $PATH" (o mesmo problema do nosso terminal embutido).

Regras do Flathub (docs.flathub.org, `requirements` e `linter`): permissões estáticas "no mínimo
absoluto"; permissões são revisadas à parte do PR (moderadores decidem no site); `host`/`home`
juntos e caminhos `/home/...` são erros; `org.freedesktop.Flatpak` gera aviso; `--share=network` em
`build-args` não funciona (build offline). A justificativa para o PR: o app é um cliente da API do
Docker, o socket é o único canal, o acesso é restrito a dois caminhos de socket, sem `home`.

## Riscos

- **Docker é dependência de host.** O Flathub aceita "caso a caso" apps que dependem de componentes
  do host; os precedentes acima (Whaler, Pods) reduzem o risco, mas a decisão é dos revisores.
- **Acesso ao socket do Docker equivale a root no host** (qualquer container montando `/`). É
  inerente ao tipo de app; declarar isso na justificativa e no README.
- **Terminal embutido** indisponível ou a reescrever (ajuste 3); sem ele o app perde a aba mais
  anunciada no metainfo/descrição ("with an embedded terminal"); ajustar o texto se for (c).
- Podman (`xdg-run/podman/podman.sock`) não está no manifesto: o app não o procura hoje.

## Pendente (não verificado na prática)

- Ver a janela, tema claro/escuro efetivo na UI, clique em link de porta (OpenURI) e troca de tema em
  tempo de execução: só inferido por documentação, logs e consulta ao portal.
- PTY e shell dentro do sandbox; `docker exec` (não há CLI).
- Docker rootless **real** (foi simulado com `socat`); usuário **realmente** fora do grupo (simulado com modo 000).
- Passo 5 só em rascunho: este ADR é a recomendação provisória; `flatpak-builder-lint repo` limpo
  depende de release com screenshots acessíveis.
- Manifesto: runtime 25.08 (26.08 disponível); fonte ainda `type: dir` local (na 7.2 vira `archive`).

## Recomendação

Seguir para a 7.2 incluindo no escopo: (1) fallback para `$XDG_RUNTIME_DIR/docker.sock` quando
`DOCKER_HOST` não existe e `/var/run/docker.sock` falha; (2) em Flatpak, tratar ausência de socket
como "sem socket/Docker" em vez de "não instalado", com o texto do `flatpak override` para
`DOCKER_HOST`; (3) decidir o terminal embutido (esconder no Flatpak ou `--exec-bridge`); (4) esconder
o terminal externo. Não desistir do Flathub: há quatro precedentes com o mesmo `finish-args`.

## Fontes

- https://docs.flathub.org/docs/for-app-authors/requirements e `.../linter`
- https://github.com/flathub/com.github.sdv43.whaler, `.../com.github.marhkb.Pods`,
  `.../io.github.buonhobo.Katharsis`, `.../io.podman_desktop.PodmanDesktop`
- https://github.com/flathub/dev.zed.Zed/pull/340
- `https://flathub.org/api/v2/exceptions/<app-id>`
- https://github.com/flatpak/flatpak-builder-tools (`cargo/flatpak-cargo-generator.py`)
- Código do repositório: `src/docker/convert.rs`, `src/error.rs`, `src/ui/prefs.rs`,
  `src/views/detail/terminal.rs`, `src/terminal_launcher.rs`.
