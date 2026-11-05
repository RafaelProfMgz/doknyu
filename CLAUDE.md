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
- O branch padrão do GitHub é a `develop`: PRs abrem para ela por padrão. Ainda assim, passe
  `--base develop` explicitamente.

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
