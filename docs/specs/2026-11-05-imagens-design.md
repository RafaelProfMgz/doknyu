# doknyu — Tela de Imagens (fase 8)

> Status: design aprovado em conversa. Spec escrita para revisão.
> Data: 2026-11-05

## 1. Objetivo

Hoje o doknyu gerencia containers, volumes e redes. Imagens ficaram fora do MVP
(`2026-10-29-doknyu-design.md`, "Fora do escopo"). Esta fase traz uma tela de **Imagens** para o
usuário ver o que ocupa espaço em disco e limpar com segurança, sem abrir o terminal.

### Critérios de sucesso

1. O usuário vê todas as imagens locais com repositório:tag, ID curto, tamanho, idade e quais
   containers as usam.
2. Consegue remover uma tag/imagem que nenhum container usa; o app impede remover imagem em uso.
3. Consegue limpar as imagens dangling (`<none>:<none>`) com confirmação e vê quanto espaço foi
   liberado.
4. A lista se atualiza sozinha quando imagens mudam fora do app (ex.: `docker pull` no terminal).

### Fora do escopo desta fase

Pull de imagem, rodar container a partir de imagem, prune de imagens com tag não usadas
(`prune -a`), remoção forçada, build, push, histórico/camadas, inspeção detalhada.

## 2. Core (`src/docker/`, `src/model.rs`)

### Modelo

```rust
pub struct ImageInfo {
    pub id: String,             // "sha256:..." completo
    pub repo_tags: Vec<String>, // vazio para dangling
    pub size: u64,              // bytes
    pub created: i64,           // unix seconds
    pub used_by: Vec<String>,   // nomes dos containers (rodando ou parados) que usam a imagem
}

pub struct PruneReport {
    pub removed: Vec<String>,   // IDs apagados
    pub reclaimed: u64,         // bytes liberados
}
```

- `ResourceKind` ganha `Image`; `convert::event_from` passa a mapear `EventMessageTypeEnum::IMAGE`.
- `used_by` é calculado em `bollard.rs` cruzando com `list_containers(all = true)` pelo ID da
  imagem, igual ao `used_by` de volumes. Containers parados contam: o daemon também recusa remover
  imagem usada por container parado.

### Trait `DockerClient`

```rust
async fn list_images(&self) -> Result<Vec<ImageInfo>, DockerError>;
async fn remove_image(&self, reference: &str) -> Result<(), DockerError>;
async fn prune_images(&self) -> Result<PruneReport, DockerError>;
```

- `list_images`: lista sem intermediárias (`all = false`), como `docker images`.
- `remove_image(reference)`: `reference` é `repo:tag` (remove aquela tag; a imagem só é apagada
  quando a última tag sai, como `docker rmi repo:tag`) ou o ID (para dangling). Sem `force`,
  sem `noprune`.
- `prune_images`: filtro `dangling=true` (equivale a `docker image prune`, sem `-a`).
- Tipos do bollard não saem de `docker/`; conversão em `convert::image_from`.
- `FakeClient` implementa as três operações, com imagens configuráveis no teste.

## 3. Interface (`src/views/images.rs`)

### Linhas: uma por tag

`ImageInfo` vira linhas na view: uma linha por item de `repo_tags`. Imagem dangling vira uma única
linha `<none>:<none>` cuja referência de remoção é o ID. Uma imagem com duas tags aparece duas
vezes, com mesmo ID, tamanho e "em uso por".

Colunas (padrão `list_header` + `list_row` da UI Zed):

| Coluna       | Fonte            | Estilo                          |
|--------------|------------------|---------------------------------|
| repo:tag     | `repo_tags[i]`   | `SANS_MEDIUM`, `Fill`, corta    |
| ID           | 12 chars após `sha256:` | `MONO`, `muted`          |
| Tamanho      | `size` legível (KB/MB/GB, base 1000 como o Docker) | `MONO`, `muted` |
| Idade        | `created` relativa ("3 dias") | `muted`            |
| Em uso por   | `used_by.join(", ")` ou "—" | `muted`, corta        |
| Ação         | botão remover    | ícone Lucide                    |

### Navegação e ações

- Novo `Screen::Images` na sidebar, depois de Redes; `ActionId::GoImages` com atalho `Ctrl+4`
  (e `4` no modo de lista, como `2`/`3` hoje).
- `ActionId::PruneImages` na command palette ("Limpar imagens sem tag"); de outra tela, leva a
  Imagens e abre o diálogo, como `PruneVolumes`.
- Busca por repo:tag ou ID (case-insensitive), reaproveitando o filtro das outras listas.

### Remover

- Botão desabilitado quando `used_by` não está vazio; tooltip "Em uso por: web, db".
- Clique abre o diálogo de confirmação com a referência ("Remover `postgres:16`?"). Se a imagem
  tem outras tags, o corpo avisa que só a tag sai e a imagem continua.
- Durante a operação a linha fica `pending` (botão desabilitado), como em volumes.
- Se a linha some enquanto o diálogo está aberto (evento externo), o diálogo fecha.

### Prune

- Botão "Limpar sem tag" na barra da tela. O diálogo lista as dangling que serão apagadas
  (ID curto + tamanho) e o total estimado; "Confirmar" desabilitado quando a lista está vazia.
- Ao concluir: toast de sucesso "N imagens removidas, X MB liberados" (com `PruneReport`).
- A lista do diálogo se recalcula quando a lista de imagens muda, como `VolumeConfirm::Prune`.

### Atualização

- Carrega ao entrar na tela e em todo evento com `ResourceKind::Image`. Eventos de container
  (create/destroy) também recarregam imagens quando a tela está aberta, porque mudam `used_by`.

## 4. Erros

- Falhas do daemon (ex.: conflito "image is being used" numa corrida, imagem já removida)
  viram toast de erro via `friendly_error`, sem derrubar a tela; a lista recarrega em seguida.
- Docker indisponível: mesmo tratamento das outras listas (onboarding / estado de erro).

## 5. i18n

Chaves novas em `locales/en.yml` e `locales/pt-BR.yml` sob `images.*` (título, colunas, tooltip,
diálogos, toast) e `actions.go_images`, `actions.prune_images`.

## 6. Testes

- `convert`: `image_from` com tags, dangling (`repo_tags` vazio ou `["<none>:<none>"]`), tamanho;
  `event_from` com tipo `image` → `Some(ResourceKind::Image)`.
- Ajustar o teste existente `event_ignores_unknown_types` (hoje usa `"image"` como tipo
  desconhecido) para outro tipo, ex. `"plugin"`.
- View (`update` com `FakeClient`): linhas por tag; dangling vira `<none>:<none>` com ID;
  remover bloqueado quando em uso; diálogo fecha quando a linha some; prune com lista vazia não
  confirma; toast com contagem e bytes após prune; busca por tag e por ID.
- Formatação: tamanho (base 1000) e idade relativa.
- `--ignored` contra Docker real: `list_images` vê uma imagem puxada no setup; `remove_image`
  numa tag criada pelo teste; `prune_images` apaga uma dangling criada pelo teste.
