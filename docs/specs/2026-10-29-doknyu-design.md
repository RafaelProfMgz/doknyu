# doknyu — Design

> Status: aprovado em conversa (seções 1 e 2). Seção 3 adicionada para revisão.
> Data: 2026-10-29

## 1. Visão

**doknyu** é um app desktop para Linux, escrito em Rust, que oferece uma interface gráfica
amigável para gerenciar Docker — no espírito do Docker Desktop, mas leve e nativo.

- **Público:** quem usa Docker no Linux e não quer depender de CLI ou de TUIs como o lazydocker.
- **Distribuição:** open source, releases públicas (AppImage, `.deb`, Flatpak).
- **Relação com o lazydocker:** inspiração, não dependência. O doknyu fala diretamente com a
  API do Docker (`/var/run/docker.sock`), assim como o próprio lazydocker faz.

### Critérios de sucesso do MVP

1. Usuário sem experiência com CLI consegue iniciar, parar, reiniciar e remover containers.
2. Consegue ver logs ao vivo e consumo de CPU/memória de um container.
3. Consegue listar e remover volumes e redes com segurança (sem apagar algo em uso por engano).
4. Consegue abrir um shell dentro de um container sem sair do app.
5. Quando o Docker não está instalado, parado ou sem permissão, o app explica o que fazer.

### Fora do escopo do MVP

Imagens (pull/remove/prune), Docker Compose, botão "abrir no lazydocker", Docker remoto,
Podman. Podem entrar em versões futuras.

## 2. Arquitetura

```
┌─────────────────────────── doknyu (1 binário) ───────────────────────────┐
│  UI (iced)                         Core (sem dependência de UI)          │
│  ┌──────────────┐   Message        ┌──────────────────────────┐          │
│  │ app.rs       │◄────────────────►│ docker::DockerClient     │          │
│  │  state+update│   Task/Sub       │  impl: BollardClient     │──► docker.sock
│  ├──────────────┤                  │  impl: FakeClient (test) │          │
│  │ views/       │                  └──────────────────────────┘          │
│  │  containers  │                  ┌──────────────────────────┐          │
│  │  detail      │                  │ model.rs (structs puros) │          │
│  │  volumes     │                  └──────────────────────────┘          │
│  │  networks    │                                                        │
│  │  onboarding  │                                                        │
│  └──────────────┘                                                        │
└──────────────────────────────────────────────────────────────────────────┘
```

### Princípios

- **Core desacoplado da UI.** `docker::DockerClient` é uma trait async que retorna tipos de
  `model.rs` (nunca tipos do bollard). Isso permite testar sem Docker real via `FakeClient`.
- **Arquitetura Elm (iced):** ação do usuário → `Message` → `update` dispara `Task` async →
  resultado volta como `Message`. Streams (eventos, logs, stats) entram como `Subscription`.
- **Tempo real sem polling:** a UI assina `docker events` e recarrega a lista afetada quando
  algo muda.
- **Layout:** sidebar (Containers | Volumes | Redes) + área principal. Clique em um container
  abre o detalhe com abas **Logs | Stats | Terminal**.

### Stack

| Função              | Crate                                   |
|---------------------|-----------------------------------------|
| GUI                 | `iced` (features: `tokio`, `canvas`)    |
| API Docker          | `bollard`                               |
| Runtime async       | `tokio`                                 |
| Terminal embutido   | `iced_term` (plano B: terminal externo) |
| Erros               | `thiserror` (core), `anyhow` (bin)      |
| i18n                | `rust-i18n` + `sys-locale`              |
| Logs internos       | `tracing` + `tracing-subscriber`        |

> As versões exatas são fixadas na Fase 0, verificando a compatibilidade `iced` ↔ `iced_term`.

## 3. Telas e interações

### Containers (tela inicial)
- Tabela: nome, imagem, status (🟢 rodando, ⚪ parado, 🔴 erro/exited≠0), portas, uptime.
- Ações por linha: iniciar, parar, reiniciar, remover. Remover pede confirmação e, se o
  container estiver rodando, oferece "forçar".
- Busca por nome/imagem; filtro "mostrar parados".
- Portas publicadas são links clicáveis (`http://localhost:<porta>`).

### Detalhe do container
- **Logs:** streaming ao vivo; auto-scroll que pausa quando o usuário rola para cima; busca;
  copiar; mostrar/ocultar timestamps; buffer limitado a 10.000 linhas.
- **Stats:** CPU %, memória (uso/limite), rede RX/TX, I/O de disco; gráficos dos últimos 60 s
  em `canvas`.
- **Terminal:** shell interativo via `iced_term` ligado a `docker exec` (tenta `bash`, cai para
  `sh`). Botão "Abrir no terminal do sistema" executa `docker exec -it <id> sh` no emulador
  padrão (plano B).

### Volumes
- Lista: nome, driver, mountpoint, containers que usam.
- Remover (desabilitado com explicação se estiver em uso) e "Limpar volumes sem uso"
  (prune com confirmação listando o que será apagado).

### Redes
- Lista: nome, driver, subnet, containers conectados.
- Remover; redes padrão (`bridge`, `host`, `none`) protegidas.

### Onboarding e erros
| Situação                     | Tela/mensagem                                                        |
|------------------------------|----------------------------------------------------------------------|
| Docker não instalado         | Explicação + link para docs.docker.com/engine/install                |
| Daemon parado                | `sudo systemctl start docker` + botão "Tentar novamente"             |
| Sem permissão no socket      | Explica grupo `docker` + `sudo usermod -aG docker $USER` + relogar   |
| Erro em ação (ex.: porta em uso) | Toast em linguagem simples + "Ver detalhes" com erro técnico     |

### Idioma
Inglês e pt-BR desde o início, detectado pelo locale do sistema.

## 4. Estrutura de código

Um único crate binário com módulos bem separados (dividir em workspace só se crescer):

```
doknyu/
├── Cargo.toml
├── locales/               # en.yml, pt-BR.yml
├── assets/                # ícone, .desktop
├── src/
│   ├── main.rs            # setup tracing, i18n, roda iced::application
│   ├── app.rs             # State, Message, update, view raiz, subscriptions
│   ├── model.rs           # ContainerInfo, ContainerState, StatsSample, VolumeInfo, NetworkInfo
│   ├── error.rs           # DockerError (classifica: NotInstalled, NotRunning, PermissionDenied, Api)
│   ├── docker/
│   │   ├── mod.rs         # trait DockerClient
│   │   ├── bollard.rs     # BollardClient
│   │   ├── fake.rs        # FakeClient (cfg(test) + feature "fake" para rodar a UI sem Docker)
│   │   └── convert.rs     # bollard → model (funções puras, testáveis)
│   ├── stats.rs           # cálculo de CPU%/mem a partir das amostras brutas
│   ├── logbuf.rs          # ring buffer de logs + busca
│   └── views/
│       ├── sidebar.rs
│       ├── containers.rs
│       ├── detail/{mod,logs,stats,terminal}.rs
│       ├── volumes.rs
│       ├── networks.rs
│       ├── onboarding.rs
│       └── widgets.rs     # status badge, toast, diálogo de confirmação, sparkline
└── tests/
    └── docker_integration.rs  # #[ignore] por padrão; roda contra Docker real
```

## 5. Testes

- **Unitários (sem Docker):** `convert.rs`, `stats.rs`, `logbuf.rs`, classificação de erros,
  e `update` do app usando `FakeClient`.
- **Integração (`cargo test -- --ignored`):** `BollardClient` contra Docker real usando
  container `alpine` efêmero (criar → start → logs → stats → exec → stop → remove).
- **CI (GitHub Actions):** `fmt`, `clippy -D warnings`, testes unitários; testes de integração
  no runner Ubuntu (já tem Docker).

## 6. Empacotamento e release

- `cargo-dist` ou workflow próprio gerando **AppImage** e **.deb** em tags `v*`.
- **Flatpak** depois do MVP (acesso ao socket exige `--filesystem=/run/docker.sock`; documentar).
- Arquivo `.desktop` + ícone.
- Licença: MIT OR Apache-2.0.

## 7. Riscos

| Risco | Mitigação |
|-------|-----------|
| `iced_term` incompatível com a versão do `iced` ou sem suporte a backend custom (exec do Docker) | Spike na Fase 0; plano B: terminal externo |
| Stats do Docker vêm como amostras brutas (cálculo de CPU% é não trivial) | Isolar em `stats.rs` com testes baseados em amostras reais |
| Logs com volume alto travando a UI | Ring buffer + entrega em lotes (batch por frame) |
| Flatpak e socket do Docker | Adiar Flatpak; AppImage/.deb primeiro |
