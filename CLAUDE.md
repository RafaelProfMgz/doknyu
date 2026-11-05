# doknyu — instruções para agentes

## Branches e PRs

```
feat/* · fix/* · ci/* · docs/*  ──PR──▶  develop  ──PR──▶  main  ──tag v*──▶  release
```

- Todo trabalho sai de uma branch criada **a partir da `develop`** e volta por PR **para a `develop`**.
- A `main` **só aceita PR vindo da `develop`**. O check "PR para main vem da develop"
  (`.github/workflows/pr-source.yml`) falha em qualquer outra origem, e o ruleset da `main` o exige.
  Não há push direto na `main`, nem force-push.
- Correção urgente também passa pela `develop` (fix/* → develop → main).
- Tags `v*` são criadas na `main`, depois do merge da `develop`, e só pelo mantenedor.
- Depois de cada promoção, o workflow `back-merge.yml` abre e mergeia sozinho o PR `main` → `develop`
  (só o commit de merge da promoção, sem conteúdo). Não abra esse PR à mão.
- O branch padrão do GitHub é a `develop`: PRs abrem para ela por padrão. Ainda assim, passe
  `--base develop` explicitamente.

## Commits e PRs

- Mensagem de commit: `<tipo>(<escopo opcional>): <assunto>` em inglês, imperativo, minúsculo,
  sem ponto final (tipos e exemplos em `CONTRIBUTING.md`; template em `.gitmessage`).
- PR: título no mesmo formato, corpo seguindo `.github/pull_request_template.md`, base `develop`.
- Labels do PR (`enhancement`, `bug`, `documentation`, `translation`…) definem a seção nas notas
  de release (`.github/release.yml`).

## O que roda no CI

| Evento | Workflows |
|---|---|
| PR para `develop` | CI leve: `fmt`, `clippy`, `test`, `msrv` (pulado se só mudar `*.md`, `docs/**`, `LICENSE-*`, `.claude/**`) + conferência do `cargo-sources.json` |
| PR `develop` → `main` | CI leve + Flatpak + Release (build do `.deb`/AppImage) + Origem do PR |
| tag `v*` | Release (publica) + Flatpak |

Antes do PR, rode localmente o mesmo que o CI leve: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test`.

## Plano e agentes

Spec em `docs/specs/`, planos por fase em `docs/plans/`, regras de execução dos subagentes em
`docs/plans/execucao-agentes.md`.
