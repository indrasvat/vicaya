# Repository Guidelines

## Project Structure & Module Organization
The root `Cargo.toml` drives the workspace; crates live in `crates/`. `vicaya-core` owns config/logging, `vicaya-index` handles storage, `vicaya-scanner` walks the filesystem, `vicaya-watcher` wraps FSEvents, `vicaya-daemon` keeps the index hot, and `vicaya-cli` plus `vicaya-tui` expose user interfaces. Shared docs live in `docs/` (see `docs/vicaya.md`), reference configs in `config/`, and build artifacts in `target/`.

## Build, Test & Development Commands
- `make build` / `cargo build --workspace` – compile every crate with the current toolchain.
- `make test` – run `cargo test --workspace --all-features` for unit + integration coverage.
- `make fmt`, `make lint`, `make check` – format via `rustfmt`, lint with `clippy -D warnings`, or run the full gate (fmt + lint + test).
- `make dev` – compile, spin up the daemon (`vicaya-daemon`), and launch the TUI in release mode.
- `make install-dev` – install the CLI from `crates/vicaya-cli` for local dogfooding; use `make run` for the full CLI+daemon+TUI flow.

## Coding Style & Naming Conventions
Follow standard Rust style (4-space indent, `snake_case` modules, `UpperCamelCase` types, `SCREAMING_SNAKE_CASE` consts). Always run `cargo fmt --all` before committing and keep `cargo clippy --workspace --all-targets --all-features -D warnings` clean. Document public APIs with `///` rustdoc and co-locate helpers with their call sites to keep modules compact.

## Testing Guidelines
Write focused unit tests inside `#[cfg(test)] mod tests` blocks within each module, and place cross-crate scenarios in `crates/<name>/tests`. Use descriptive names such as `it_indexes_hidden_paths`, target >80% coverage for `vicaya-core` and `vicaya-index`, and add regression tests with every bugfix. Run `cargo test --workspace --all-features` (or `make test`) before pushing; use `cargo bench` for performance-sensitive changes.

## Commit & Pull Request Guidelines
Commit messages follow Conventional Commits (`type(scope): summary`) with scopes mapped to crates (`index`, `scanner`, `cli`, etc.). Keep commit messages brief and relevant (one line). Never bulk-stage changes via `git add -A` (or similar); always explicitly list the files to be staged. Ensure `make check` passes, update `README.md`, `docs/vicaya.md`, or `CHANGELOG.md` when behavior shifts, and rebase noisy commits before opening a PR. Pull requests must outline intent, list validation commands, reference issues, and attach CLI/TUI screenshots when UI output changes; request review only after CI is green.

## Security & Configuration Tips
File watching and indexing assume macOS 10.15+ permissions, so confirm Spotlight/full-disk access before filing watcher bugs. Keep secrets out of `config/` defaults—store overrides locally and add them to `.gitignore`. When testing daemon changes, prefer `make daemon-dev` plus `cargo run --package vicaya-cli -- daemon status` to avoid orphaned processes.
