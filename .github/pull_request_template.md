<!-- PRs go to `develop`. `main` only accepts PRs from `develop`. -->

## What and why

<!-- What does this change and why? Link the issue: "Closes #123". -->

## How it was tested

<!-- Tests added, manual steps, screenshots for UI changes. -->

## Checklist

- [ ] Base branch is `develop`
- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass
- [ ] New user-visible strings are in `locales/en.yml` and `locales/pt-BR.yml`
- [ ] If `Cargo.lock` changed: `scripts/update-cargo-sources.sh` was run
