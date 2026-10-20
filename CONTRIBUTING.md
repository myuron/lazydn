# Contributing to lazydn

Thank you for your interest in contributing!

## Development environment

The recommended setup uses [Nix](https://nixos.org/) with flakes enabled:

```sh
nix develop
```

This provides the Rust stable toolchain and [just](https://github.com/casey/just).
Without Nix, install the latest stable Rust via [rustup](https://rustup.rs/) and `just`.

## Common tasks

| Command      | Purpose                                     |
| ------------ | ------------------------------------------- |
| `just run`   | Run the application                         |
| `just fmt`   | Format Rust and Nix files (`nix fmt`)       |
| `just lint`  | Run clippy with warnings treated as errors  |
| `just test`  | Run the test suite                          |
| `just check` | Run all checks performed by CI              |

Please run `just check` before opening a pull request.

## Commit messages

- Write commit messages in English.
- Explain **why** the change is needed; the diff already shows what changed.

## Pull requests

1. Fork the repository and create a topic branch from `main`.
2. Make your changes and add tests where appropriate.
3. Make sure `just check` passes.
4. Open a pull request in English and fill in the template (What + Why).

## Reporting issues

Use the issue templates for bug reports and feature requests.
