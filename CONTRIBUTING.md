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

## Releasing

1. Start from an up-to-date `main` whose CI is green. Bump the version in `Cargo.toml`, then run `cargo check` so `Cargo.lock` is updated (`nix/rust.nix` reads the version from `Cargo.toml`). Merge the change to `main`.
2. Tag the merge commit and push the tag:

   ```sh
   git tag vX.Y.Z && git push origin vX.Y.Z
   ```

   The tag triggers `.github/workflows/release.yml`: it verifies the tag is on `main` and matches the version, builds the three targets and creates the GitHub Release.
3. If the workflow fails because of a flaky job, re-run that job. If the tagged commit itself is wrong, fix it on `main`, delete the tag (`git push origin :refs/tags/vX.Y.Z && git tag -d vX.Y.Z`) and tag again. If the `release` job already created the GitHub Release, delete it first (`gh release delete vX.Y.Z`).
4. Only after the GitHub Release has succeeded, publish to crates.io by hand. Run `cargo login` once, check out the tag on a clean tree, then run `cargo publish --dry-run` followed by `cargo publish`. This cannot be undone (a version can only be yanked), so it is not part of the workflow.

## Reporting issues

Use the issue templates for bug reports and feature requests.
