# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- _Nothing yet_

### Changed
- _Nothing yet_

### Deprecated
- N/A

### Removed
- N/A

### Fixed
- N/A

### Security
- N/A

## [0.2.0] - TBD

### Added
- Initial Rust workspace structure
- Core crates: core, index, scanner, watcher, daemon, cli
- File table with efficient string arena
- Trigram-based inverted index for substring search
- Query engine with scoring and ranking
- Parallel filesystem scanner
- Basic CLI interface with search, rebuild, status commands
- Configuration system with TOML support
- Structured logging with tracing
- GitHub Actions CI pipeline
- Makefile for common dev tasks
- Multi-job CI with Linux + macOS builds, Codecov uploads, and universal macOS artifacts
- macOS release workflow producing `.pkg` and `.tar.gz` installers plus SHA256 checksums
- Shared build metadata module powering consistent `--version` output across CLI, daemon, and TUI
- Coverage badge + documentation links to Codecov dashboards

### Changed
- README now documents coverage/reporting locations and upcoming download artifacts

## [0.1.0] - TBD

Initial development release.

[Unreleased]: https://github.com/indrasvat/vicaya/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/indrasvat/vicaya/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/indrasvat/vicaya/releases/tag/v0.1.0
