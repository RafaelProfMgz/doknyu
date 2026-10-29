# doknyu — Plano de implementação (roadmap)

> **Para agentes:** SUB-SKILL OBRIGATÓRIA: use superpowers:subagent-driven-development (recomendado)
> ou superpowers:executing-plans para implementar fase a fase, tarefa a tarefa. Os passos usam
> checkbox (`- [ ]`) para acompanhamento.

**Objetivo:** app desktop Linux em Rust (iced + bollard) para gerenciar containers, logs, stats,
volumes, redes e terminal de containers Docker de forma amigável.

**Arquitetura:** binário único. Core (`model`, `error`, `docker`, `stats`, `logbuf`) não depende
da UI; a UI iced segue a arquitetura Elm e conversa com o core via a trait `DockerClient`.
`FakeClient` permite testar `update` e rodar a UI sem Docker.

**Stack:** Rust 1.95 (edition 2024), iced, bollard, tokio, futures, async-trait, iced_term,
thiserror, anyhow, rust-i18n, sys-locale, tracing.

**Spec:** [`docs/specs/2026-10-29-doknyu-design.md`](../specs/2026-10-29-doknyu-design.md)

## Fases

| Fase | Arquivo | Entrega (testável sozinha) |
|------|---------|----------------------------|
| 0 | [fase-0-fundacao.md](fase-0-fundacao.md) | Repo, Cargo, CI, versões fixadas, spike do `iced_term` |
| 1 | [fase-1-core-docker.md](fase-1-core-docker.md) | `model`, `error`, trait `DockerClient`, `BollardClient`, `FakeClient` |
| 2 | [fase-2-shell-containers.md](fase-2-shell-containers.md) | Janela, sidebar, onboarding, lista de containers + ações + eventos |
| 3 | [fase-3-logs-stats.md](fase-3-logs-stats.md) | Detalhe do container: abas Logs e Stats |
| 4 | [fase-4-volumes-redes.md](fase-4-volumes-redes.md) | Telas de Volumes e Redes |
| 5 | [fase-5-terminal.md](fase-5-terminal.md) | Aba Terminal (`iced_term` ou terminal externo) |
| 6 | [fase-6-i18n-release.md](fase-6-i18n-release.md) | pt-BR/en completos, ícone, AppImage/.deb, release |

Cada fase termina com o app compilando, `cargo clippy -- -D warnings` limpo e testes passando.

## Restrições globais

- Rust edition 2024, MSRV 1.89 (iced 0.14 exige 1.88; serde-saphyr, via rust-i18n, exige 1.89); `cargo fmt` e `cargo clippy --all-targets -- -D warnings` limpos.
- Licença: `MIT OR Apache-2.0`.
- A UI nunca importa tipos do `bollard`; só tipos de `crate::model` e `crate::error`.
- Toda string visível ao usuário passa por `t!("chave")` (rust-i18n) a partir da Fase 2;
  locales: `en` (fallback) e `pt-BR`.
- Buffer de logs: no máximo **10.000** linhas por container.
- Gráficos de stats: janela de **60** amostras (1 por segundo).
- Redes protegidas contra remoção: `bridge`, `host`, `none`.
- Operações destrutivas (remover container/volume/rede, prune) sempre pedem confirmação.
- Socket padrão: `/var/run/docker.sock`; respeitar `DOCKER_HOST` se definido (`unix://`).

## Interfaces compartilhadas (definidas na Fase 1, usadas por todas)

```rust
// src/model.rs
pub enum ContainerState { Running, Paused, Restarting, Created, Exited { code: i64 }, Dead }
pub struct PortMapping { pub private: u16, pub public: Option<u16>, pub protocol: String }
pub struct ContainerInfo {
    pub id: String, pub name: String, pub image: String,
    pub state: ContainerState, pub status_text: String,
    pub ports: Vec<PortMapping>,
}
pub struct RawStats {
    pub cpu_total: u64, pub precpu_total: u64, pub system: u64, pub presystem: u64,
    pub online_cpus: u32, pub mem_usage: u64, pub mem_inactive_file: u64, pub mem_limit: u64,
    pub net_rx: u64, pub net_tx: u64, pub blk_read: u64, pub blk_write: u64,
}
pub struct StatsSample {
    pub cpu_percent: f64, pub mem_used: u64, pub mem_limit: u64,
    pub net_rx: u64, pub net_tx: u64, pub blk_read: u64, pub blk_write: u64,
}
pub enum LogStream { Stdout, Stderr }
pub struct LogLine { pub stream: LogStream, pub timestamp: Option<String>, pub text: String }
pub struct VolumeInfo { pub name: String, pub driver: String, pub mountpoint: String, pub used_by: Vec<String> }
pub struct NetworkInfo { pub id: String, pub name: String, pub driver: String, pub subnet: Option<String>, pub containers: Vec<String> }
pub enum ResourceKind { Container, Volume, Network }
pub struct DockerEvent { pub kind: ResourceKind, pub action: String, pub id: String }

// src/error.rs
pub enum DockerError {
    NotInstalled, NotRunning, PermissionDenied,
    NotFound(String), Conflict(String), Api { status: u16, message: String }, Other(String),
}

// src/docker/mod.rs
pub type DockerStream<T> = futures::stream::BoxStream<'static, Result<T, DockerError>>;
#[async_trait::async_trait]
pub trait DockerClient: Send + Sync + 'static {
    async fn ping(&self) -> Result<(), DockerError>;
    async fn list_containers(&self, all: bool) -> Result<Vec<ContainerInfo>, DockerError>;
    async fn start(&self, id: &str) -> Result<(), DockerError>;
    async fn stop(&self, id: &str) -> Result<(), DockerError>;
    async fn restart(&self, id: &str) -> Result<(), DockerError>;
    async fn remove(&self, id: &str, force: bool) -> Result<(), DockerError>;
    fn logs(&self, id: &str, tail: usize) -> DockerStream<LogLine>;
    fn stats(&self, id: &str) -> DockerStream<RawStats>;
    fn events(&self) -> DockerStream<DockerEvent>;
    async fn list_volumes(&self) -> Result<Vec<VolumeInfo>, DockerError>;
    async fn remove_volume(&self, name: &str) -> Result<(), DockerError>;
    async fn prune_volumes(&self) -> Result<Vec<String>, DockerError>;
    async fn list_networks(&self) -> Result<Vec<NetworkInfo>, DockerError>;
    async fn remove_network(&self, id: &str) -> Result<(), DockerError>;
    // exec_shell é adicionado na Fase 5
}
```

Todos os tipos de `model` derivam `Debug, Clone, PartialEq` (e `Eq` quando possível).
`DockerError` deriva `Debug, Clone, PartialEq, thiserror::Error`.

## Foco de revisão

Situações que a spec implica mas que nenhum caminho feliz exercita; cada uma tem teste na tarefa dona:

1. **Container removido por fora com o detalhe aberto** → streams de logs/stats terminam; UI mostra
   aviso "container não existe mais" e volta à lista, sem panic. (Fase 3, Tarefa 3.3)
2. **Daemon reinicia/para com o app aberto** → stream de eventos termina; app vai para a tela de
   onboarding `NotRunning` e reconecta sozinho a cada 3 s. (Fase 2, Tarefa 2.4)
3. **Logs com códigos ANSI, `\r`, bytes não UTF-8 ou linha gigante** → decodificação lossy, ANSI
   removido, linha truncada em 4.000 caracteres com "…". (Fase 3, Tarefa 3.1)
4. **Primeira amostra de stats / container parado** (`precpu_total = 0`, `system == presystem`) →
   CPU 0.0, nunca NaN/infinito. (Fase 1, Tarefa 1.3)
5. **Remover volume/rede em uso** → `DockerError::Conflict` mostrado como mensagem amigável,
   item continua na lista. (Fase 4, Tarefas 4.1 e 4.2)
