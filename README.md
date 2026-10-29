# doknyu

Aplicativo desktop para Linux, escrito em Rust (iced + bollard), para gerenciar containers,
logs, stats, volumes, redes e terminal de containers Docker de forma amigável.

## Desenvolvimento

```sh
cargo run
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Rust 1.88 ou superior (exigido pelo `iced` 0.14).

Docs: [spec](docs/specs/2026-10-29-doknyu-design.md) e [plano](docs/plans/README.md).

## Licença

Licenciado sob `MIT OR Apache-2.0`, à sua escolha.
