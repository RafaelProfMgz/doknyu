# Execução com subagentes — equipe, mapa de tarefas e orçamento

Os agentes ficam em `.claude/agents/`. O orquestrador é a sessão principal do Claude Code, que segue
este documento e o skill `superpowers:subagent-driven-development`.

## Equipe

| Agente | Modelo / esforço | `maxTurns` | Ferramentas | Skills pré-carregadas | Web |
|--------|------------------|-----------:|-------------|-----------------------|-----|
| `rust-implementer` | Sonnet / medium | 40 | Read, Write, Edit, Bash, Glob, Grep | test-driven-development, verification-before-completion | ✗ |
| `ui-implementer` | Sonnet / high | 50 | Read, Write, Edit, Bash, Glob, Grep | test-driven-development, verification-before-completion | ✗ |
| `research-implementer` | Sonnet / high | 35 | idem + WebFetch, WebSearch (máx. 8 buscas) | — | ✓ |
| `task-reviewer` | Sonnet / medium | 15 | Read, Bash, Glob, Grep (sem escrita) | — | ✗ |
| `phase-reviewer` | **Opus** / high | 25 | Read, Bash, Glob, Grep (sem escrita) | — | ✗ |

**Plugins e MCP:** nenhum agente recebe ferramentas MCP (Vercel, Postman, PostHog, Supabase,
Cloudflare, navegador etc.), porque a lista `tools` é uma allowlist e não inclui `mcp__*`. Nenhum
agente recebe a ferramenta `Agent`, então **nenhum subagente pode criar outro subagente**. As skills
de outros plugins (frontend-design, vercel:*, …) não se aplicam a um app iced e não são carregadas.

## Mapa tarefa → agente

| Fase | Tarefa | Implementa | Revisa |
|------|--------|-----------|--------|
| 0 | 0.1 Projeto + CI | `research-implementer` (precisa fixar versões) | `task-reviewer` |
| 0 | 0.2 Spike iced_term | `research-implementer` | — (é descartável; o humano lê a decisão) |
| 1 | 1.1 – 1.5 Core | `rust-implementer` | `task-reviewer` |
| 2 | 2.1 – 2.4 Shell + containers | `ui-implementer` | `task-reviewer` |
| 3 | 3.1 LogBuffer | `rust-implementer` | `task-reviewer` |
| 3 | 3.2 – 3.3 Logs/Stats | `ui-implementer` | `task-reviewer` |
| 4 | 4.1 – 4.2 Volumes/Redes | `ui-implementer` | `task-reviewer` |
| 5 | 5.1 Terminal externo | `rust-implementer` | `task-reviewer` |
| 5 | 5.2 Terminal embutido | `ui-implementer` | `task-reviewer` |
| 6 | 6.1 – 6.2 i18n + ícone | `rust-implementer` | `task-reviewer` |
| 6 | 6.3 Release | `research-implementer` | `task-reviewer` |
| todas | fim de fase | — | `phase-reviewer` |

## Regras de orçamento (o que de fato limita o gasto)

Não existe limite de tokens ou de dinheiro por subagente no Claude Code: os subagentes consomem o
mesmo limite de uso da sessão. O controle é feito por estas regras:

1. **Um agente por vez.** Nada roda em paralelo; `.claude/settings.json` define
   `CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS=1` como trava.
2. **Teto de turnos por agente** (`maxTurns` acima). Se um agente esgotar os turnos, o resultado
   volta como parcial e o orquestrador **para e avisa o humano**, sem relançar automaticamente.
3. **No máximo 2 ciclos de correção por tarefa** (implementa → revisa → corrige → revisa →
   corrige → revisa). Se a tarefa ainda não for aprovada no 3º review, ela é escalada ao humano.
4. **Parada obrigatória ao fim de cada fase:** o `phase-reviewer` roda, o humano faz a verificação
   manual e aprova antes da próxima fase começar. Assim o gasto máximo de uma vez é o de uma fase.
5. **Opus só no `phase-reviewer`** (7 execuções no total). Todo o resto roda em Sonnet.
6. **Contexto enxuto:** cada agente lê o README do plano e a sua tarefa, não a spec inteira. As
   respostas finais têm tamanho limitado (15 a 25 linhas).
7. **Web só no `research-implementer`**, com no máximo 8 buscas por tarefa.

### Estimativa de execuções

| Fase | Implementações | Reviews de tarefa | Review de fase | Total (sem retrabalho) | Pior caso (2 correções/tarefa) |
|------|---:|---:|---:|---:|---:|
| 0 | 2 | 1 | 1 | 4 | 8 |
| 1 | 5 | 5 | 1 | 11 | 31 |
| 2 | 4 | 4 | 1 | 9 | 25 |
| 3 | 3 | 3 | 1 | 7 | 19 |
| 4 | 2 | 2 | 1 | 5 | 13 |
| 5 | 2 | 2 | 1 | 5 | 13 |
| 6 | 3 | 3 | 1 | 7 | 19 |
| **Total** | **21** | **20** | **7** | **48** | **128** |

## Fluxo do orquestrador por tarefa

1. Lança o implementador indicado com: arquivo da fase, número da tarefa e feedback anterior (se houver).
2. `BLOCKED` ou resultado parcial → para e pergunta ao humano.
3. `DONE` → lança o `task-reviewer` com o hash do commit.
4. `CHANGES_REQUESTED` → volta ao passo 1 com o feedback (respeitando a regra 3).
5. `APPROVED` → próxima tarefa. Ao fim da fase, lança o `phase-reviewer` e para para o humano.
