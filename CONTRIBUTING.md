# Contributing to doknyu

Thanks for your interest! Bug reports, ideas, translations and code are all welcome.
By participating you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Questions, bugs and ideas

- **Questions and ideas:** [Discussions](https://github.com/RafaelProfMgz/doknyu/discussions).
- **Bugs and feature requests:** [open an issue](https://github.com/RafaelProfMgz/doknyu/issues/new/choose)
  using one of the templates.
- **Security problems:** do not open a public issue; see [SECURITY.md](SECURITY.md).

Issues labelled `good first issue` are a good place to start.

## Branches

- **`main` is production:** it only holds what was already approved on `develop`; releases are
  tagged from it.
- **`develop` is development:** every change lands here first and is promoted to `main` in
  batches, through a `develop` → `main` pull request.

```
your-branch ──PR──▶ develop ──PR──▶ main ──tag v*──▶ release
```

- Create your branch from **`develop`** (the default branch) and open the pull request
  **against `develop`**.
- `main` only accepts pull requests from `develop`; a check fails any other source.
- Releases are tagged on `main` by the maintainer.

Branch names are free; the maintainers use `feat/…`, `fix/…`, `docs/…`, `ci/…`, `build/…`.

## Development setup

Rust 1.89 or newer. On Ubuntu/Debian, the build needs
`libxkbcommon-dev libwayland-dev libvulkan-dev pkg-config`.

```sh
cargo run                    # needs a running Docker daemon
cargo run --features fake    # in-memory fake client, no Docker needed
```

## Before opening a pull request

Run the same checks as the CI:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test -- --ignored      # integration tests, need Docker
```

If you changed `Cargo.lock`, also regenerate the Flatpak sources:
`scripts/update-cargo-sources.sh` (the CI checks it).

Guidelines:

- **Tests:** new behaviour comes with tests. UI logic is tested through `update` with the
  in-memory `FakeClient` (`src/docker/fake.rs`), so most tests do not need Docker.
- **Translations:** every user-visible string goes through `t!("key")`, with entries in both
  `locales/en.yml` and `locales/pt-BR.yml`. Improvements to either language are welcome.
- **Docker types stay in `src/docker/`:** the rest of the app only sees the types in `src/model.rs`.
- **One topic per pull request.** Keep unrelated changes in separate pull requests.
- **Bigger changes:** open an issue or discussion first. Designs live in `docs/specs/` and the
  phased plans in `docs/plans/`.

## Commit messages

```
<type>(<optional scope>): <what changed, imperative, lower case, no final period>

<optional body: why, and anything a reviewer needs; wrap at 72 columns>

<optional footer: Closes #123>
```

| Type | For |
|---|---|
| `feat` | a new user-visible feature |
| `fix` | a bug fix |
| `docs` | documentation only |
| `test` | tests only |
| `refactor` | code change that neither fixes a bug nor adds a feature |
| `perf` | performance |
| `build` | packaging, dependencies, build scripts |
| `ci` | GitHub Actions workflows |
| `chore` | anything else (repository settings, tooling) |

Examples: `feat: images screen`, `fix(terminal): keep the cursor after resize`,
`build(aur): pin doknyu-git to main`. Subject up to ~72 characters, in English.

A template with this reminder is in [`.gitmessage`](.gitmessage); enable it once with
`git config commit.template .gitmessage`.

## Pull requests

- Target **`develop`**. The [PR template](.github/pull_request_template.md) has the checklist.
- Title in the same format as a commit subject (`feat: …`); it is used in the release notes.
- Small and focused is easier to review. Draft PRs are welcome for early feedback.
- CI must be green. PRs from first-time contributors need a maintainer's approval to run CI.
- Maintainers merge into `develop`; `develop` is promoted to `main` in batches before a release.

## Packaging

Packaging files live in `packaging/<channel>/`; see [packaging/README.md](packaging/README.md)
(in Portuguese) for how each channel is built and checked.

## License

By contributing, you agree that your contributions are licensed under the same terms as the
project: [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at the user's option.
