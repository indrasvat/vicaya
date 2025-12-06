# 1.0.0 (2025-12-06)


### Bug Fixes

* **cli:** correct border alignment in status command ([b72cc07](https://github.com/indrasvat/vicaya/commit/b72cc07048c7742bfa9e41b14cfe9022cffd34f7))
* **cli:** fix Daemon row alignment in status command ([43b74f9](https://github.com/indrasvat/vicaya/commit/43b74f97e7b892783d5ad153e751ab14ab267616))
* **cli:** fix status UI border alignment issues ([83a1954](https://github.com/indrasvat/vicaya/commit/83a19546a55b1455c8ec6fb242c9f78cb0f225ac))
* **config:** resolve clippy ptr_arg warning and add comprehensive tests ([015cf30](https://github.com/indrasvat/vicaya/commit/015cf30835ef4595989fd34f0e7f850243f14d57))
* **config:** use ~/ by default and implement tilde expansion ([1eca9c8](https://github.com/indrasvat/vicaya/commit/1eca9c8aa122822e75a25176d9c4510853c6ebbd))
* **hooks:** run full CI pipeline (make ci) in pre-push hook ([4b269f1](https://github.com/indrasvat/vicaya/commit/4b269f1ea32be6519e7f3d52140ad7ebdabf8ede))
* **make:** add daemon readiness check before launching TUI ([5d9ac5e](https://github.com/indrasvat/vicaya/commit/5d9ac5ebe79d428544b4a43d9e141d91958181ca))
* **make:** add missing commands to run target ([3c6b868](https://github.com/indrasvat/vicaya/commit/3c6b868425bb9f6bb15dcffe1110d10aa4c2c3d0))
* **scanner:** use path component matching instead of substring matching ([d0228d7](https://github.com/indrasvat/vicaya/commit/d0228d7c5be4b82c4c6c3b83309e977fdd22242e))
* **tui:** fix editor not opening by executing after TUI exits ([3fc44e2](https://github.com/indrasvat/vicaya/commit/3fc44e2021cb0eee73e179644078b56d49fdd25f))
* **tui:** implement focus system and fix all interaction issues ([c567ae1](https://github.com/indrasvat/vicaya/commit/c567ae1964b03fca66435a21066fd843c2f03e4b))


### Features

* **ci:** add release-please and PR preview releases ([#5](https://github.com/indrasvat/vicaya/issues/5)) ([2f61f4e](https://github.com/indrasvat/vicaya/commit/2f61f4ebf87c1f495ed710de8a34c591387e6bb4))
* **ci:** add universal builds, release tooling, and version metadata ([eaee5e9](https://github.com/indrasvat/vicaya/commit/eaee5e96d5792dcf9a108d783340db05418cf14d))
* **ci:** replace release-please with semantic-release ([#6](https://github.com/indrasvat/vicaya/issues/6)) ([43f0600](https://github.com/indrasvat/vicaya/commit/43f06000f16854a14b03fb422ff7360494b9f231))
* **cli:** add 'vicaya init' command for frictionless first-time setup ([8e310d7](https://github.com/indrasvat/vicaya/commit/8e310d7e49f4a540bf992a937c7012db9f249100))
* **cli:** enhance default exclusions to 60+ comprehensive patterns ([0cd74e6](https://github.com/indrasvat/vicaya/commit/0cd74e6908b168ed1c0aaeb5dfd8eab3f84bab11))
* **cli:** enhance status command with beautiful UI and JSON support ([3c2775e](https://github.com/indrasvat/vicaya/commit/3c2775e6996adb8cfe6d4f71dea189e65189301e))
* **core:** implement vicaya filesystem search foundation ([837cce6](https://github.com/indrasvat/vicaya/commit/837cce615b43ca403b2aac926c2005cedc4e298b))
* **daemon:** implement complete daemon lifecycle management ([67c3e52](https://github.com/indrasvat/vicaya/commit/67c3e520d9a1a5358f0248bc0594982602cdee4a))
* **ipc:** implement Unix socket-based daemon communication ([02e0d98](https://github.com/indrasvat/vicaya/commit/02e0d983a436cb65665c94d64e86f175098b2c6f))
* **make:** add colored output to help command ([7e0c06c](https://github.com/indrasvat/vicaya/commit/7e0c06cdc6fbb2eaa4701ef0367b4c8559a981b5))
* **make:** add convenient workflow commands ([4e889c7](https://github.com/indrasvat/vicaya/commit/4e889c78becdb50025ec04ac79b49db5ccc56f7f))
* **make:** add dev target for quick start without installation ([4a5f51b](https://github.com/indrasvat/vicaya/commit/4a5f51b2ddfb03e7ea17b15e20fd6a32e35a6202))
* **make:** add help target with command documentation ([3b089d8](https://github.com/indrasvat/vicaya/commit/3b089d8bef3d828beb165cfd6733cb5d1994dc64))
* **search:** implement smart abbreviation matching ([acaeac6](https://github.com/indrasvat/vicaya/commit/acaeac6b64babf9a3c34e0308b6c8b075e35d560))
* **tui:** add file actions and improve UX with path display ([083ca8a](https://github.com/indrasvat/vicaya/commit/083ca8ae92e92f50595510768c14c307e0a3610e))
* **tui:** implement beautiful dark mode TUI with real-time search ([0d73c7a](https://github.com/indrasvat/vicaya/commit/0d73c7aeb5c129d91bf497766828d4a8271d4faa))


### Performance Improvements

* **benchmarks:** add comprehensive performance analysis vs find/grep ([5d5ca6e](https://github.com/indrasvat/vicaya/commit/5d5ca6eff54647a140efff35e0a97ded0d740d48))
* **index:** add early termination for non-matching linear searches ([98dd1bf](https://github.com/indrasvat/vicaya/commit/98dd1bf9147126067f3ba4d5ed20216ddf36bac4))
* **index:** optimize FileId from u64 to u32 for 40-50MB memory savings ([998acb3](https://github.com/indrasvat/vicaya/commit/998acb361d11068e68e3d97d5335dcc6d420a478))

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
