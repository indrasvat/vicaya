# 1.0.0 (2026-01-16)


### Bug Fixes

* **ci:** build artifacts within semantic-release workflow ([#7](https://github.com/indrasvat/vicaya/issues/7)) ([e43e155](https://github.com/indrasvat/vicaya/commit/e43e155fdbd49fe1a76ef2d82849e4ff64308395))
* **cli:** correct border alignment in status command ([87e664d](https://github.com/indrasvat/vicaya/commit/87e664d4b16d187ee2960d8709baa3171eb384e0))
* **cli:** fix Daemon row alignment in status command ([25ecec6](https://github.com/indrasvat/vicaya/commit/25ecec6105407ca05f7997abdc7c10a4ef77f904))
* **cli:** fix status UI border alignment issues ([8e81790](https://github.com/indrasvat/vicaya/commit/8e817902ea40be063f58ba50f749de492605973b))
* **config:** resolve clippy ptr_arg warning and add comprehensive tests ([9b243a3](https://github.com/indrasvat/vicaya/commit/9b243a3434c669b766604968ed40cca82d01209d))
* **config:** use ~/ by default and implement tilde expansion ([0fa434a](https://github.com/indrasvat/vicaya/commit/0fa434a9ad27719c04ff27b3441b9befafa440c7))
* **daemon:** self-heal stale index + robust IPC ([#8](https://github.com/indrasvat/vicaya/issues/8)) ([fff601d](https://github.com/indrasvat/vicaya/commit/fff601d099527c1138bebc6cddb6a713977b019e))
* **hooks:** run full CI pipeline (make ci) in pre-push hook ([5e5d56a](https://github.com/indrasvat/vicaya/commit/5e5d56adc86134452c961e9c49cf8c4d2a6260d0))
* **make:** add daemon readiness check before launching TUI ([b257a5b](https://github.com/indrasvat/vicaya/commit/b257a5b0e3d165611e77b59277183bc8798c89d3))
* **make:** add missing commands to run target ([575696c](https://github.com/indrasvat/vicaya/commit/575696c56cd00de390085e17ffdce25c70a3aca3))
* **scanner:** use path component matching instead of substring matching ([93ddb61](https://github.com/indrasvat/vicaya/commit/93ddb6106a442cd81bcc3a80682f7c030c9b0035))
* **tui:** fix editor not opening by executing after TUI exits ([405f427](https://github.com/indrasvat/vicaya/commit/405f4279fdce2256f943eb65b88101b55b3c0c30))
* **tui:** implement focus system and fix all interaction issues ([987c4ea](https://github.com/indrasvat/vicaya/commit/987c4ea6694d05e6c659393304a59affa9d0a803))


### Features

* **ci:** add release-please and PR preview releases ([#5](https://github.com/indrasvat/vicaya/issues/5)) ([8d4c740](https://github.com/indrasvat/vicaya/commit/8d4c74034c8366b7c41c25fb9a479780ad40269d))
* **ci:** add universal builds, release tooling, and version metadata ([1c22fee](https://github.com/indrasvat/vicaya/commit/1c22fee29bee8f776223f6c42eedd8b3c4bdd815))
* **ci:** replace release-please with semantic-release ([#6](https://github.com/indrasvat/vicaya/issues/6)) ([0ba4280](https://github.com/indrasvat/vicaya/commit/0ba42807c69cfb4bb190eac14b921c3f9a28aee8))
* **cli:** add 'vicaya init' command for frictionless first-time setup ([dc4f80c](https://github.com/indrasvat/vicaya/commit/dc4f80c9a576b58366921051e9f81848417ff7d0))
* **cli:** enhance default exclusions to 60+ comprehensive patterns ([ff375b2](https://github.com/indrasvat/vicaya/commit/ff375b2ee7f8a1012fd026f04c28863ff8b9178a))
* **cli:** enhance status command with beautiful UI and JSON support ([f179186](https://github.com/indrasvat/vicaya/commit/f1791865396b36345096f6b1f4088beb98648899))
* **core:** implement vicaya filesystem search foundation ([7e65011](https://github.com/indrasvat/vicaya/commit/7e650116ecb436700bce222bebca9496fe12f894))
* **daemon:** implement complete daemon lifecycle management ([45213f5](https://github.com/indrasvat/vicaya/commit/45213f56bddf734655cc455fa8192d094a1b5168))
* **index:** improve ranking with context + scope ([#11](https://github.com/indrasvat/vicaya/issues/11)) ([f02b4ed](https://github.com/indrasvat/vicaya/commit/f02b4ed19b43031c0cff2de1124f9e612dd05fd5))
* **ipc:** implement Unix socket-based daemon communication ([6ac0a5d](https://github.com/indrasvat/vicaya/commit/6ac0a5db87161cc946cf6ece72a4e67d8fc88532))
* **make:** add colored output to help command ([845fb12](https://github.com/indrasvat/vicaya/commit/845fb12c900b5f3a0a46c391c271ad498f49876f))
* **make:** add convenient workflow commands ([5571d07](https://github.com/indrasvat/vicaya/commit/5571d07797f66a4c4c17faad1c2e1fcc7667667e))
* **make:** add dev target for quick start without installation ([7e87975](https://github.com/indrasvat/vicaya/commit/7e879755bc8549f55506f7bd2b868b22ba2a5fa9))
* **make:** add help target with command documentation ([c307c83](https://github.com/indrasvat/vicaya/commit/c307c8393a32491c9cf864b3b4ad3db315b25cff))
* **search:** implement smart abbreviation matching ([3665487](https://github.com/indrasvat/vicaya/commit/366548741ce09a886b0bf3a233eb6f90ae256c4b))
* **tui:** add drishti switcher + preview pane ([#9](https://github.com/indrasvat/vicaya/issues/9)) ([3bdaade](https://github.com/indrasvat/vicaya/commit/3bdaadeffba5a9c40dcf686b9d43903ddf2881a9))
* **tui:** add file actions and improve UX with path display ([c59b88c](https://github.com/indrasvat/vicaya/commit/c59b88c9310394345a543a9867d6d3d5e30dba75))
* **tui:** implement beautiful dark mode TUI with real-time search ([0c11592](https://github.com/indrasvat/vicaya/commit/0c115922273783181d5d3dbc033f62dc66900d5a))
* **tui:** ksetra/niyama/varga + preview search + kriya-suchi ([#10](https://github.com/indrasvat/vicaya/issues/10)) ([0a44c11](https://github.com/indrasvat/vicaya/commit/0a44c111d31190d054a9c5a40c7fa7c5be206ebd))
* **tui:** show recent files on startup ([#14](https://github.com/indrasvat/vicaya/issues/14)) ([2e302ca](https://github.com/indrasvat/vicaya/commit/2e302ca61e658ede64b5e6d008533488b3602471))


### Performance Improvements

* **benchmarks:** add comprehensive performance analysis vs find/grep ([e1b532c](https://github.com/indrasvat/vicaya/commit/e1b532c4664e30d0f8c983d421901902ee434e08))
* **index:** add early termination for non-matching linear searches ([e6b46a4](https://github.com/indrasvat/vicaya/commit/e6b46a48d01b176e9dacbd3b24441f0dcbc7936e))
* **index:** optimize FileId from u64 to u32 for 40-50MB memory savings ([b529370](https://github.com/indrasvat/vicaya/commit/b5293703dbb4f96e90f4dbfa27dfc8f9d4ad8eb9))
* reduce daemon memory footprint and add runtime metrics ([#12](https://github.com/indrasvat/vicaya/issues/12)) ([b0f1fb6](https://github.com/indrasvat/vicaya/commit/b0f1fb6d8b19006f4a7c9e3501270d81222ec70d))

# 1.0.0 (2026-01-10)


### Bug Fixes

* **ci:** build artifacts within semantic-release workflow ([#7](https://github.com/indrasvat/vicaya/issues/7)) ([e43e155](https://github.com/indrasvat/vicaya/commit/e43e155fdbd49fe1a76ef2d82849e4ff64308395))
* **cli:** correct border alignment in status command ([87e664d](https://github.com/indrasvat/vicaya/commit/87e664d4b16d187ee2960d8709baa3171eb384e0))
* **cli:** fix Daemon row alignment in status command ([25ecec6](https://github.com/indrasvat/vicaya/commit/25ecec6105407ca05f7997abdc7c10a4ef77f904))
* **cli:** fix status UI border alignment issues ([8e81790](https://github.com/indrasvat/vicaya/commit/8e817902ea40be063f58ba50f749de492605973b))
* **config:** resolve clippy ptr_arg warning and add comprehensive tests ([9b243a3](https://github.com/indrasvat/vicaya/commit/9b243a3434c669b766604968ed40cca82d01209d))
* **config:** use ~/ by default and implement tilde expansion ([0fa434a](https://github.com/indrasvat/vicaya/commit/0fa434a9ad27719c04ff27b3441b9befafa440c7))
* **daemon:** self-heal stale index + robust IPC ([#8](https://github.com/indrasvat/vicaya/issues/8)) ([fff601d](https://github.com/indrasvat/vicaya/commit/fff601d099527c1138bebc6cddb6a713977b019e))
* **hooks:** run full CI pipeline (make ci) in pre-push hook ([5e5d56a](https://github.com/indrasvat/vicaya/commit/5e5d56adc86134452c961e9c49cf8c4d2a6260d0))
* **make:** add daemon readiness check before launching TUI ([b257a5b](https://github.com/indrasvat/vicaya/commit/b257a5b0e3d165611e77b59277183bc8798c89d3))
* **make:** add missing commands to run target ([575696c](https://github.com/indrasvat/vicaya/commit/575696c56cd00de390085e17ffdce25c70a3aca3))
* **scanner:** use path component matching instead of substring matching ([93ddb61](https://github.com/indrasvat/vicaya/commit/93ddb6106a442cd81bcc3a80682f7c030c9b0035))
* **tui:** fix editor not opening by executing after TUI exits ([405f427](https://github.com/indrasvat/vicaya/commit/405f4279fdce2256f943eb65b88101b55b3c0c30))
* **tui:** implement focus system and fix all interaction issues ([987c4ea](https://github.com/indrasvat/vicaya/commit/987c4ea6694d05e6c659393304a59affa9d0a803))


### Features

* **ci:** add release-please and PR preview releases ([#5](https://github.com/indrasvat/vicaya/issues/5)) ([8d4c740](https://github.com/indrasvat/vicaya/commit/8d4c74034c8366b7c41c25fb9a479780ad40269d))
* **ci:** add universal builds, release tooling, and version metadata ([1c22fee](https://github.com/indrasvat/vicaya/commit/1c22fee29bee8f776223f6c42eedd8b3c4bdd815))
* **ci:** replace release-please with semantic-release ([#6](https://github.com/indrasvat/vicaya/issues/6)) ([0ba4280](https://github.com/indrasvat/vicaya/commit/0ba42807c69cfb4bb190eac14b921c3f9a28aee8))
* **cli:** add 'vicaya init' command for frictionless first-time setup ([dc4f80c](https://github.com/indrasvat/vicaya/commit/dc4f80c9a576b58366921051e9f81848417ff7d0))
* **cli:** enhance default exclusions to 60+ comprehensive patterns ([ff375b2](https://github.com/indrasvat/vicaya/commit/ff375b2ee7f8a1012fd026f04c28863ff8b9178a))
* **cli:** enhance status command with beautiful UI and JSON support ([f179186](https://github.com/indrasvat/vicaya/commit/f1791865396b36345096f6b1f4088beb98648899))
* **core:** implement vicaya filesystem search foundation ([7e65011](https://github.com/indrasvat/vicaya/commit/7e650116ecb436700bce222bebca9496fe12f894))
* **daemon:** implement complete daemon lifecycle management ([45213f5](https://github.com/indrasvat/vicaya/commit/45213f56bddf734655cc455fa8192d094a1b5168))
* **index:** improve ranking with context + scope ([#11](https://github.com/indrasvat/vicaya/issues/11)) ([f02b4ed](https://github.com/indrasvat/vicaya/commit/f02b4ed19b43031c0cff2de1124f9e612dd05fd5))
* **ipc:** implement Unix socket-based daemon communication ([6ac0a5d](https://github.com/indrasvat/vicaya/commit/6ac0a5db87161cc946cf6ece72a4e67d8fc88532))
* **make:** add colored output to help command ([845fb12](https://github.com/indrasvat/vicaya/commit/845fb12c900b5f3a0a46c391c271ad498f49876f))
* **make:** add convenient workflow commands ([5571d07](https://github.com/indrasvat/vicaya/commit/5571d07797f66a4c4c17faad1c2e1fcc7667667e))
* **make:** add dev target for quick start without installation ([7e87975](https://github.com/indrasvat/vicaya/commit/7e879755bc8549f55506f7bd2b868b22ba2a5fa9))
* **make:** add help target with command documentation ([c307c83](https://github.com/indrasvat/vicaya/commit/c307c8393a32491c9cf864b3b4ad3db315b25cff))
* **search:** implement smart abbreviation matching ([3665487](https://github.com/indrasvat/vicaya/commit/366548741ce09a886b0bf3a233eb6f90ae256c4b))
* **tui:** add drishti switcher + preview pane ([#9](https://github.com/indrasvat/vicaya/issues/9)) ([3bdaade](https://github.com/indrasvat/vicaya/commit/3bdaadeffba5a9c40dcf686b9d43903ddf2881a9))
* **tui:** add file actions and improve UX with path display ([c59b88c](https://github.com/indrasvat/vicaya/commit/c59b88c9310394345a543a9867d6d3d5e30dba75))
* **tui:** implement beautiful dark mode TUI with real-time search ([0c11592](https://github.com/indrasvat/vicaya/commit/0c115922273783181d5d3dbc033f62dc66900d5a))
* **tui:** ksetra/niyama/varga + preview search + kriya-suchi ([#10](https://github.com/indrasvat/vicaya/issues/10)) ([0a44c11](https://github.com/indrasvat/vicaya/commit/0a44c111d31190d054a9c5a40c7fa7c5be206ebd))


### Performance Improvements

* **benchmarks:** add comprehensive performance analysis vs find/grep ([e1b532c](https://github.com/indrasvat/vicaya/commit/e1b532c4664e30d0f8c983d421901902ee434e08))
* **index:** add early termination for non-matching linear searches ([e6b46a4](https://github.com/indrasvat/vicaya/commit/e6b46a48d01b176e9dacbd3b24441f0dcbc7936e))
* **index:** optimize FileId from u64 to u32 for 40-50MB memory savings ([b529370](https://github.com/indrasvat/vicaya/commit/b5293703dbb4f96e90f4dbfa27dfc8f9d4ad8eb9))
* reduce daemon memory footprint and add runtime metrics ([#12](https://github.com/indrasvat/vicaya/issues/12)) ([b0f1fb6](https://github.com/indrasvat/vicaya/commit/b0f1fb6d8b19006f4a7c9e3501270d81222ec70d))

# 1.0.0 (2026-01-10)


### Bug Fixes

* **ci:** build artifacts within semantic-release workflow ([#7](https://github.com/indrasvat/vicaya/issues/7)) ([e43e155](https://github.com/indrasvat/vicaya/commit/e43e155fdbd49fe1a76ef2d82849e4ff64308395))
* **cli:** correct border alignment in status command ([87e664d](https://github.com/indrasvat/vicaya/commit/87e664d4b16d187ee2960d8709baa3171eb384e0))
* **cli:** fix Daemon row alignment in status command ([25ecec6](https://github.com/indrasvat/vicaya/commit/25ecec6105407ca05f7997abdc7c10a4ef77f904))
* **cli:** fix status UI border alignment issues ([8e81790](https://github.com/indrasvat/vicaya/commit/8e817902ea40be063f58ba50f749de492605973b))
* **config:** resolve clippy ptr_arg warning and add comprehensive tests ([9b243a3](https://github.com/indrasvat/vicaya/commit/9b243a3434c669b766604968ed40cca82d01209d))
* **config:** use ~/ by default and implement tilde expansion ([0fa434a](https://github.com/indrasvat/vicaya/commit/0fa434a9ad27719c04ff27b3441b9befafa440c7))
* **daemon:** self-heal stale index + robust IPC ([#8](https://github.com/indrasvat/vicaya/issues/8)) ([fff601d](https://github.com/indrasvat/vicaya/commit/fff601d099527c1138bebc6cddb6a713977b019e))
* **hooks:** run full CI pipeline (make ci) in pre-push hook ([5e5d56a](https://github.com/indrasvat/vicaya/commit/5e5d56adc86134452c961e9c49cf8c4d2a6260d0))
* **make:** add daemon readiness check before launching TUI ([b257a5b](https://github.com/indrasvat/vicaya/commit/b257a5b0e3d165611e77b59277183bc8798c89d3))
* **make:** add missing commands to run target ([575696c](https://github.com/indrasvat/vicaya/commit/575696c56cd00de390085e17ffdce25c70a3aca3))
* **scanner:** use path component matching instead of substring matching ([93ddb61](https://github.com/indrasvat/vicaya/commit/93ddb6106a442cd81bcc3a80682f7c030c9b0035))
* **tui:** fix editor not opening by executing after TUI exits ([405f427](https://github.com/indrasvat/vicaya/commit/405f4279fdce2256f943eb65b88101b55b3c0c30))
* **tui:** implement focus system and fix all interaction issues ([987c4ea](https://github.com/indrasvat/vicaya/commit/987c4ea6694d05e6c659393304a59affa9d0a803))


### Features

* **ci:** add release-please and PR preview releases ([#5](https://github.com/indrasvat/vicaya/issues/5)) ([8d4c740](https://github.com/indrasvat/vicaya/commit/8d4c74034c8366b7c41c25fb9a479780ad40269d))
* **ci:** add universal builds, release tooling, and version metadata ([1c22fee](https://github.com/indrasvat/vicaya/commit/1c22fee29bee8f776223f6c42eedd8b3c4bdd815))
* **ci:** replace release-please with semantic-release ([#6](https://github.com/indrasvat/vicaya/issues/6)) ([0ba4280](https://github.com/indrasvat/vicaya/commit/0ba42807c69cfb4bb190eac14b921c3f9a28aee8))
* **cli:** add 'vicaya init' command for frictionless first-time setup ([dc4f80c](https://github.com/indrasvat/vicaya/commit/dc4f80c9a576b58366921051e9f81848417ff7d0))
* **cli:** enhance default exclusions to 60+ comprehensive patterns ([ff375b2](https://github.com/indrasvat/vicaya/commit/ff375b2ee7f8a1012fd026f04c28863ff8b9178a))
* **cli:** enhance status command with beautiful UI and JSON support ([f179186](https://github.com/indrasvat/vicaya/commit/f1791865396b36345096f6b1f4088beb98648899))
* **core:** implement vicaya filesystem search foundation ([7e65011](https://github.com/indrasvat/vicaya/commit/7e650116ecb436700bce222bebca9496fe12f894))
* **daemon:** implement complete daemon lifecycle management ([45213f5](https://github.com/indrasvat/vicaya/commit/45213f56bddf734655cc455fa8192d094a1b5168))
* **index:** improve ranking with context + scope ([#11](https://github.com/indrasvat/vicaya/issues/11)) ([f02b4ed](https://github.com/indrasvat/vicaya/commit/f02b4ed19b43031c0cff2de1124f9e612dd05fd5))
* **ipc:** implement Unix socket-based daemon communication ([6ac0a5d](https://github.com/indrasvat/vicaya/commit/6ac0a5db87161cc946cf6ece72a4e67d8fc88532))
* **make:** add colored output to help command ([845fb12](https://github.com/indrasvat/vicaya/commit/845fb12c900b5f3a0a46c391c271ad498f49876f))
* **make:** add convenient workflow commands ([5571d07](https://github.com/indrasvat/vicaya/commit/5571d07797f66a4c4c17faad1c2e1fcc7667667e))
* **make:** add dev target for quick start without installation ([7e87975](https://github.com/indrasvat/vicaya/commit/7e879755bc8549f55506f7bd2b868b22ba2a5fa9))
* **make:** add help target with command documentation ([c307c83](https://github.com/indrasvat/vicaya/commit/c307c8393a32491c9cf864b3b4ad3db315b25cff))
* **search:** implement smart abbreviation matching ([3665487](https://github.com/indrasvat/vicaya/commit/366548741ce09a886b0bf3a233eb6f90ae256c4b))
* **tui:** add drishti switcher + preview pane ([#9](https://github.com/indrasvat/vicaya/issues/9)) ([3bdaade](https://github.com/indrasvat/vicaya/commit/3bdaadeffba5a9c40dcf686b9d43903ddf2881a9))
* **tui:** add file actions and improve UX with path display ([c59b88c](https://github.com/indrasvat/vicaya/commit/c59b88c9310394345a543a9867d6d3d5e30dba75))
* **tui:** implement beautiful dark mode TUI with real-time search ([0c11592](https://github.com/indrasvat/vicaya/commit/0c115922273783181d5d3dbc033f62dc66900d5a))
* **tui:** ksetra/niyama/varga + preview search + kriya-suchi ([#10](https://github.com/indrasvat/vicaya/issues/10)) ([0a44c11](https://github.com/indrasvat/vicaya/commit/0a44c111d31190d054a9c5a40c7fa7c5be206ebd))


### Performance Improvements

* **benchmarks:** add comprehensive performance analysis vs find/grep ([e1b532c](https://github.com/indrasvat/vicaya/commit/e1b532c4664e30d0f8c983d421901902ee434e08))
* **index:** add early termination for non-matching linear searches ([e6b46a4](https://github.com/indrasvat/vicaya/commit/e6b46a48d01b176e9dacbd3b24441f0dcbc7936e))
* **index:** optimize FileId from u64 to u32 for 40-50MB memory savings ([b529370](https://github.com/indrasvat/vicaya/commit/b5293703dbb4f96e90f4dbfa27dfc8f9d4ad8eb9))
* reduce daemon memory footprint and add runtime metrics ([#12](https://github.com/indrasvat/vicaya/issues/12)) ([b0f1fb6](https://github.com/indrasvat/vicaya/commit/b0f1fb6d8b19006f4a7c9e3501270d81222ec70d))

## [1.0.1](https://github.com/indrasvat/vicaya/compare/v1.0.0...v1.0.1) (2025-12-06)


### Bug Fixes

* **ci:** build artifacts within semantic-release workflow ([#7](https://github.com/indrasvat/vicaya/issues/7)) ([b4a9f60](https://github.com/indrasvat/vicaya/commit/b4a9f60e2ef856bccf6feb96da7ff274c116df78))

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
- Daemon reconciliation to self-heal when filesystem events are missed.

### Changed
- IPC responses are written atomically to avoid truncated JSON in clients.

### Deprecated
- N/A

### Removed
- N/A

### Fixed
- Daemon now reconciles missed filesystem changes on startup and on a daily schedule.
- TUI/CLI no longer hit JSON parse errors from partial IPC reads/writes.

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
