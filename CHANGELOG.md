# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

<!-- next-header -->

## [1.0.2] - 2026-09-21

### Added

- Report what a run changed in the cache

### Changed

- Document the diff command and tighten the page

## [1.0.1] - 2026-09-21

### Added

- Read a mod's release from its jar filename
- Report pack changes against the cached state

### Changed

- Add diff context and scaffolding to cache architecture

## [1.0.0] - 2026-09-13

### Added

- Add --color-mode, --sort-by and --reverse ([#15](https://github.com/justin-carver/sculkr/issues/15))
- Add --cache and rename the cache file to .sculkr.cache.json

### Changed

- Adopt strict clippy lints and resolve violations ([#14](https://github.com/justin-carver/sculkr/issues/14))
- Write snake_case field names throughout
- Document every flag, the cache and the JSON export

### Fixed

- **Breaking:** Write pack.pack_format in snake_case like every other key
- Report the --cache path in sculkr config
- Stop the color-mode re-parse from discarding .sculk settings
- Tell users what to do with a packwiz-modlist cache

## [0.2.4] - 2026-09-10

### Changed

- Regenerate the 0.2.2 and 0.2.3 sections
- Point local checks at the justfile
- Correct the commit and changelog conventions
- Drop the removed PACK_ROOT variable
- Add a CODEOWNERS file
- Add a security policy

### Fixed

- Render entries as single lines and skip style commits
- Omit link references for untagged versions
- Route security-scoped commits to the Security section
- Exclude development tooling from the published crate
- Stop the gitmoji preprocessors mangling Rust paths

## [0.2.3] - 2026-09-10

### Changed

- List the current feature set

## [0.2.2] - 2026-09-10

### Added

- Implemented the force `-f, --force` arg to forcibly overwrite specified output files
- Scaffolding helper functions and schemas for packwiz data exports
- Resolve the pack from its root and export it as JSON

### Changed

- Updated .gitignore to add `test/` folder for local development

## [0.2.1] - 2026-09-09

_Maintenance release. No user-facing changes; see the commit log for build, CI and tooling work._

## [0.2.0] - 2026-09-09

### Added

- The `.sculk` config file (`src/config.rs`): flat TOML file, every key is optional, one key per flag (`path`, `output`, `format`, `verbose`, `quiet`). This will make future ideas easier to manage once the pack reaches `v1.0.0`.
    - Two locations are read and merged, global first: `<config dir>/.sculk` for every pack on the machine, then `<pack root>/.sculk` for that pack. Command line flags still win over both.
- Added the `directories` crate to the project, which is what locates the global config file per-OS.
- A `.sculk` file can now carry the CurseForge API key, in a `[secrets]` table of its own: `cf-api-key`. One key in the global `.sculk` can serve every modpack on the machine, instead of a `.env` per pack.
    - `CF_API_KEY` in the environment (a `.env` counts) still wins over both files. A key exported on your own machine should not be displaced by a file that arrived with somebody else's modpack.
    - A key found in a **pack's** `.sculk` logs a warning on every run. That file is normally committed and shipped with the modpack, so anything in it is public. The key is still used -- a private pack repo is a real thing -- but the warning does not go away. A key in the global `.sculk` is silent.
- Added some new badges to the top of the `README.md`, and changed the styling of it.

### Changed

- `sculkr config` now lists the `.sculk` files it read, or names where a global one would go when there are none.
- The `CF_API_KEY` line of `sculkr config` now names where the key came from, e.g. `$2a$...e345 (from the environment)`, since there are now three places it could be.
- The CurseForge API key is resolved once at startup and handed down to the request, rather than each request reaching into the process environment for itself.
- `--format` no longer carries a clap default, so a value from a `.sculk` file can be told apart from a flag the user actually passed. The default is applied afterwards and `--help` renders exactly as before.
- Updated the design of the `sculkr config` output. Tried to use some colors, and some divider ASCII things?
- Updated `README.md` to include information about `.sculk` config file, how it works, priorities, save locations, formatting, etc.

### Removed

- Removed the older `packwiz-modlist` `template/` directory. The `format` arg can probably do this without needing dedicated template files. (or write a Rust/Bash/PS script!)

### Fixed

- Edited `README.md` to fix icon centering issues by replacing `<center>` with `<div align="center">`. Apparently, GitHub sanitizes HTML input. Who knew! /s

## [0.1.4] - 2026-09-09

### Added

- Greatly expanded the `#[test]` suite of the main command/arg processing core in `src/args.rs`. Ensures that all listed args and commands have appropriate surface coverage for any parsable context the app can provide.
- Implemented `insta` and `cargo-insta` into the build pipeline, for local development and testing related to snapshot comparisons. These will soon be integrating into the GH Actions/CI workflow.

### Changed

- Modified `.gitignore` to properly untrack pending `insta` snapshots.
- Updated the way `sculkr config` operates. Originally planned as a drop-in/path enabled way to configure sculkr on the fly, this instead now provides a overview of the local modpack environment with a single command. As this app grows, more content will be added to it's output.
- Updated `README.md` with a more in-depth `Usage` section, detailing how various arguments should be used.
- Decided that going forward, args/commands/subcommands, etc., will implement a writer pattern when flushing data to disk or stdout/err, which will _hopefully_ prevent IO lock issues, if the writes are massive.

### Removed

- Removed various mentions of argument/flag workarounds in `README.md`, due to the include of the `Usage` section.

### Security

- `Secret` now deserializes, so a key can be read out of a `.sculk` without ever becoming a plain `String`. It keeps redacting itself in `Debug`, which means printing a whole `Config` cannot leak the key. There is deliberately no `Serialize` and no `PartialEq`.
- Extended the `Secret()` implement to show the first 4 and last 4 characters `(e.g. $2a$...e345)` of a secret string, to determine if any token/API issues are causing problems.

## [0.1.3] - 2026-09-09

### Added

- Added `-o, --output` flag and verified arg chaining is working! Defaults to stdout for displaying mod lists, but can flush contents to a custom file path.
- Added a proper `-p, --path`command to prefer relative paths and runtime args, instead of relying on hardcoded or `.env` values.

### Fixed

- Adjusted context, layout, description of `about` command when running `help`, added color, and some neat formatting.
- Updated `README.md` and `.env.example` to remove `PACK_ROOT` references.

### Removed

- Removed `PACK_ROOT` from `src/env.rs`

## [0.1.2] - 2026-09-08

### Added

- Added instructions on how to install via `cargo`, build from source, or locate prebuilt binaries once they are introduced (hopefully very soon!)

### Fixed

- Updated `src/args.rs` to include new description information relating to the new branding of sculkr.
- Updated information in README.md to make more sense to new users, added project icon image, README badges, fixed overall layout of README.

## [0.1.1] - 2026-09-08

<!-- next-url -->
[Unreleased]: https://github.com/justin-carver/sculkr/compare/v1.0.2...HEAD
[1.0.2]: https://github.com/justin-carver/sculkr/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/justin-carver/sculkr/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/justin-carver/sculkr/compare/v0.2.4...v1.0.0
[0.2.4]: https://github.com/justin-carver/sculkr/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/justin-carver/sculkr/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/justin-carver/sculkr/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/justin-carver/sculkr/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/justin-carver/sculkr/compare/v0.1.4...v0.2.0
[0.1.4]: https://github.com/justin-carver/sculkr/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/justin-carver/sculkr/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/justin-carver/sculkr/tree/v0.1.2
