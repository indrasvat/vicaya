# Contributing to vicaya

Thank you for your interest in contributing to vicaya! This document provides guidelines for contributing to the project.

## Getting Started

### Prerequisites

- Rust 1.70+ (via `rustup`)
- macOS 10.15+ for full testing
- Git

### Setup

```bash
# Clone the repository
git clone https://github.com/indrasvat/vicaya.git
cd vicaya

# Build the project
cargo build --workspace

# Run tests
cargo test --workspace

# Run linting
cargo clippy --workspace --all-targets --all-features

# Format code
cargo fmt --all
```

## Development Workflow

### Code Style

- Follow Rust standard conventions
- Use `cargo fmt` for consistent formatting
- Ensure `cargo clippy` passes with no warnings
- Add tests for new functionality
- Document public APIs with rustdoc comments

### Commit Messages

Use conventional commit format:

```
type(scope): brief description

Longer description if needed

Types: feat, fix, docs, style, refactor, test, chore, perf, build
Scopes: core, index, scanner, watcher, daemon, cli, ui
```

Examples:
```bash
git commit -m "feat(index): add trigram-based substring search"
git commit -m "docs(readme): update installation instructions"
git commit -m "fix(scanner): handle symlinks correctly"
```

### Pull Requests

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes
4. Run tests and linting (`make check`)
5. Commit your changes using conventional commits
6. Push to your fork
7. Open a Pull Request

### Code Organization

- **vicaya-core**: Core types, config, errors, logging
- **vicaya-index**: Data structures for indexing
- **vicaya-scanner**: Filesystem scanning logic
- **vicaya-watcher**: File system watching
- **vicaya-daemon**: Background service
- **vicaya-cli**: Command-line interface

### Testing

- Write unit tests for new functionality
- Place tests in `#[cfg(test)]` modules within the same file
- Use integration tests in `tests/` for cross-crate testing
- Aim for >80% coverage in core and index crates

### Performance

- Profile before optimizing
- Add benchmarks for performance-critical code
- Document performance characteristics

## Documentation

- Update README.md for user-facing changes
- Update docs/vicaya.md for implementation changes
- Add rustdoc comments for public APIs
- Update CHANGELOG.md following Keep a Changelog format

## Questions?

- Open an issue for bugs or feature requests
- Tag issues appropriately (bug, enhancement, documentation, etc.)
- Be respectful and constructive in discussions

## License

By contributing, you agree that your contributions will be licensed under the same terms as the project (MIT or Apache-2.0).
