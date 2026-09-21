# sculkr

<div align="center">

<img src=".github/assets/sculk-chute.png" width="25%"/>

[![crates.io](https://img.shields.io/crates/v/sculkr?style=flat-square&logo=rust&logoColor=white&label=crates.io)](https://crates.io/crates/sculkr)
![Crates.io Total Downloads](https://img.shields.io/crates/d/sculkr?style=flat-square&logo=rust&color=blue&link=https%3A%2F%2Fcrates.io%2Fcrates%2Fsculkr)
[![release](https://img.shields.io/github/v/release/justin-carver/sculkr?style=flat-square&logo=github&label=release&sort=semver)](https://github.com/justin-carver/sculkr/releases/latest)

[![CI](https://img.shields.io/github/actions/workflow/status/justin-carver/sculkr/ci.yml?branch=main&style=flat-square&logo=githubactions&logoColor=white&label=CI)](https://github.com/justin-carver/sculkr/actions/workflows/ci.yml)
[![msrv](https://img.shields.io/badge/MSRV-1.88%2B-b7410e?style=flat-square&logo=rust&logoColor=white)](https://github.com/justin-carver/sculkr/blob/main/Cargo.toml)
[![license](https://img.shields.io/crates/l/sculkr?style=flat-square&color=DFBE6F)](https://github.com/justin-carver/sculkr/blob/main/LICENSE)

A companion CLI application for `packwiz` that parses its output data to deliver advanced utility commands and extended features for Minecraft modpack development.

</div>

## Features

- **Modlist generation** from any [packwiz](https://packwiz.infra.link/) pack, Modrinth and CurseForge alike.
- **Custom modlist templates** via `--format`, with 16 placeholders. See [Formatting](#formatting).
- **Sortable** by any placeholder with `--sort-by`, A-Z or Z-A.
- **JSON export** of the entire pack with `--json`, for feeding other tools. See [JSON Export](#json-export).
- **Folder-agnostic**, so `resourcepacks/`, `shaderpacks/` and `datapacks/` are listed too.
- **Cached per pinned version.** A fully cached run makes zero API calls. See [Cache](#cache).
- **`sculkr diff`** shows what changed since the last run, offline. See [Tracking Changes](#tracking-changes).
- **Shareable config** in committed `.sculk` files, global and per-pack.
- **Keeps `CF_API_KEY` out of your pack**, redacted in output and never written to output files.
- **Pipes cleanly**, since every log line goes to stderr instead of into your modlist.

## Installation

Requires Rust **1.88** or newer (edition 2024).

### Dependencies

`sculkr` reads the files [packwiz](https://github.com/packwiz/packwiz) writes
(`pack.toml`, `index.toml` and every `*.pw.toml`) and never runs packwiz itself,
so packwiz only needs to be installed wherever you manage the pack. It does not
modify your pack — the only files it writes are its own [cache](#cache) and the
output file you name.
[Writing to packs is under discussion.](https://github.com/justin-carver/sculkr/issues/10)

### From crates.io

```sh
cargo install sculkr
```

### Prebuilt binaries

Every tagged release ships archives on the
[releases page](https://github.com/justin-carver/sculkr/releases/latest) — x86_64
and aarch64 for Linux and macOS, x86_64 for Windows. Extract one and put `sculkr`
on your `PATH`. Each release also carries `SHA256SUMS.txt` and a provenance
attestation:

```sh
sha256sum --check --ignore-missing SHA256SUMS.txt
gh attestation verify sculkr-<version>-<target>.tar.gz --repo justin-carver/sculkr
```

### From source

```sh
git clone https://github.com/justin-carver/sculkr.git
cd sculkr
cargo install --path .
```

## Configuration

### `.sculk` Config Files

Flags you would otherwise retype on every run can live in a `.sculk` file, which
is plain TOML. Every key is optional and matches a long flag by name:

```toml
# .sculk
path       = "."                             # anywhere in the pack, same as --path
output     = "modlist.md"                    # same as --output; omit for stdout
format     = '- [{NAME}]({URL}) - {DESC}\n'  # same as --format
sort-by    = "name"                          # same as --sort-by; any placeholder but INDEX
reverse    = false                           # same as --reverse; Z-A instead of A-Z
json       = false                           # same as --json; overrides format
verbose    = 1                               # 0-3, same as -v/-vv/-vvv
quiet      = false                           # same as --quiet
color-mode = "auto"                          # auto, always or never, same as --color-mode

[secrets]                                    # read the Secrets section below first
cf-api-key = '$2a$10$...'                    # same as CF_API_KEY
```

`--force` and `--cache` are command-line only. The full list is under [Flags](#flags).

Two files are read, and then merged, in descending order. Anything passed on the command line takes priority:

| Location                                         | Applies to                       |
| ------------------------------------------------ | -------------------------------- |
| `~/.config/sculkr/.sculk` (or the OS equivalent) | every pack on the machine        |
| `<pack root>/.sculk`                             | that pack, overriding the global |

`sculkr config` prints which files were found. Keep in mind:

- **Relative paths resolve against the file they are written in**, not the
  working directory, so a global `path` means the same thing from anywhere.
- **Use single quotes for `format`.** TOML resolves `\n` inside double quotes; a
  literal string hands the escape to the formatter instead.
- **An unknown or unparseable key is a warning, not an error.** It is skipped.
- **`.sculk` files belong in Git, `.env` does not.** Committing one lets a pack
  ship its own configuration, which is exactly why `[secrets]` below needs care.

### Secrets in `.sculk`

A `.sculk` may carry a CurseForge key, so one file can serve every pack on the
machine instead of a `.env` per pack:

```toml
# ~/.config/sculkr/.sculk
[secrets]
cf-api-key = '$2a$10$...'
```

> [!WARNING]
> Put it in the **global** file, not in a pack's. A pack's `.sculk` is meant to
> be committed and shipped with the modpack, so a key written into one is a key
> handed to everybody who downloads it. `sculkr` warns on every run when it
> finds one there — it still uses the key, because a private pack repo is a
> real thing, but the warning does not go away.

Three places are checked, and the first one with a value wins:

| Order | Source                                             |
| ----- | -------------------------------------------------- |
| 1     | ↓ `CF_API_KEY` in the environment, `.env` included |
| 2     | ↓ `[secrets]` in `<pack root>/.sculk`              |
| 3     | \_ `[secrets]` in the global `.sculk`              |

The environment takes precedence over both files. `sculkr config` prints which of the three the key in
effect came from, alongside its fingerprint.

Quote the key with **SINGLE** quotes, the same as `format`, so nothing inside it is read as a TOML escape.

### Environment Vars (.env)

`sculkr` reads its secrets from the environment at run time, and loads a `.env` from the working directory (or the nearest parent that has one), if present. Copy [`.env.example`](.env.example) to `.env` and fill in its values to get started.

| Variable     | Required            | Value                                                                                                                                                                                                                                   |
| ------------ | ------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `CF_API_KEY` | For CurseForge mods | A [CurseForge API key](https://console.curseforge.com/). Only requested when the pack actually contains CurseForge mods — a Modrinth-only pack never needs one. Overrides [`secrets.cf-api-key`](#secrets-in-sculk) in either `.sculk`. |

Same as the above `.sculk` `[secrets]` formatting, quote `CF_API_KEY` with **single** quotes. CurseForge keys are bcrypt-shaped
(`$2a$10$...`) and dotenv expands `$VAR` inside double quotes, which silently
truncates the key and earns you a `403` with an empty body.

Nothing is read at compile time — `build.rs` fails the build if anything under
`src/` tries — so no key can be baked into a published binary.

## Usage

```sh
# Print the modlist to stdout (default Markdown list format)
sculkr

# Point at a pack from anywhere; any directory inside it works
sculkr -p ~/modpack/mods

# Write it to a file instead of stdout, and overwrite it on later runs
sculkr -p ~/modpack -o modlist.md --force

# Apply a custom template, one mod per line
sculkr -f '{INDEX}. {NAME} ({SLUG}) - {LICENSE_ID}\n'

# Sort by author instead of name (A-Z), or Z-A with --reverse
sculkr --sort-by authors --reverse

# The whole pack as JSON, for other tools
sculkr --json -o pack.json

# Debug logging on stderr, modlist still outputs cleanly to file
sculkr -vv -o modlist.md

# See what changed since the last run, without fetching anything
sculkr diff

# Runtime information about sculkr and the modpack
sculkr config
```

`-p` can point anywhere inside a pack: `sculkr` searches upward for `pack.toml`
and reads the pack from there, so running from `mods/` and from the pack root
list the same thing. Logs go to stderr, so a redirected or piped modlist stays
clean.

### Flags

Anything in the `.sculk` column can be set in a [`.sculk` file](#sculk-config-files)
instead. A flag on the command line always wins over the file.

<details>
<summary><strong>Every flag</strong></summary>

| Flag                  | `.sculk` key | Default             | Description                                                                                                   |
| --------------------- | ------------ | ------------------- | ------------------------------------------------------------------------------------------------------------- |
| `-p`, `--path <PATH>` | `path`       | current directory   | Any directory inside the pack. The root is found by searching upward for `pack.toml`.                         |
| `-o`, `--output <PATH>` | `output`   | stdout              | Write the modlist to a file. If the file exists and `--force` is not set, the list goes to stdout with a warning instead. |
| `-F`, `--force`       |              | off                 | Overwrite the `--output` file if it already exists.                                                           |
| `-f`, `--format <FORMAT>` | `format` | `- [{NAME}]({URL}) - {DESCRIPTION}\n` | Template for each mod. See [Formatting](#formatting).                         |
| `-s`, `--sort-by <FIELD>` | `sort-by` | `NAME`             | Sort by any placeholder except `INDEX`. Numbers compare by value (`Mod 2` before `Mod 10`), and mods with no value for the field come last. |
| `-r`, `--reverse`     | `reverse`    | off                 | Sort Z-A instead of A-Z.                                                                                      |
| `--json`              | `json`       | off                 | Emit the whole pack as one JSON document instead of a modlist. `--format` is ignored. See [JSON Export](#json-export). |
| `--cache <PATH>`      |              | `.sculkr.cache.json` | Read and write the cache at this path instead. See [Cache](#cache).                                          |
| `-v`, `--verbose`     | `verbose`    | warnings only       | `-v` adds info, `-vv` adds debug and trace. All of it goes to stderr.                                         |
| `-q`, `--quiet`       | `quiet`      | off                 | Errors only.                                                                                                  |
| `-c`, `--color-mode <MODE>` | `color-mode` | `auto`       | `auto`, `always` or `never`. `auto` colors a terminal and respects `NO_COLOR`.                                |
| `-h`, `--help`        |              |                     | `-h` for a summary, `--help` for the full text, including every placeholder.                                  |
| `-V`, `--version`     |              |                     | Print the version.                                                                                            |

</details>

### Commands

| Command         | Description                                                                                             |
| --------------- | ------------------------------------------------------------------------------------------------------- |
| _(none)_        | Generate the modlist, or the JSON export with `--json`.                                                 |
| `sculkr config` | Show the settings in effect and where each came from: config files, pack root, mod counts, cache, and which `CF_API_KEY` source is used. Makes no API calls. |
| `sculkr diff`   | Show what the next run would fetch, update and prune. Makes no API calls. See [Tracking Changes](#tracking-changes). |
| `sculkr about`  | Version, authors and repository.                                                                        |
| `sculkr help`   | Same as `--help`; `sculkr help <command>` for one command.                                              |

## Tracking Changes

`sculkr diff` compares the versions your pack pins against what the cache holds,
and prints what the next run would do. It reads the pack and the cache off disk
and makes no API calls, so it works offline and without a CurseForge key.

```
  ⣿ sculkr ⣿
  Changes since the last cached state

  + Added (2)
      Iris Shaders      modrinth
      Distant Horizons  curseforge

  - Removed (1)
      OptiFabric        modrinth

  ~ Updated (2)
      Sodium            0.6.0   -> 0.6.5
      Lithium           0.14.0  -> 0.14.3

  45 mods, 41 unchanged

  Run sculkr to merge these into the cache (fetch 2, update 2, drop 1).
```

| Section     | Meaning                                                              |
| ----------- | -------------------------------------------------------------------- |
| `Added`     | Pinned by the pack, not in the cache. The next run fetches it.       |
| `Removed`   | Still cached, no longer in the pack. The next run prunes it.         |
| `Updated`   | Pinned to a different version than the cache holds.                  |
| `Unchanged` | Pinned to the version already cached, and served without a request.  |

Version numbers are read from the jar filename packwiz records, skipping the
pack's Minecraft version so `jei-1.21.1-neoforge-19.21.0.247.jar` reads as
`19.21.0.247`. Where a filename carries no version at all, the pinned id is
shown in its place.

A normal run reports the same thing on stderr once it finishes, at `-v`:

```
INFO  [sculkr::app] cache updated: 2 added, 2 updated, 1 removed (41 unchanged)
```

`-vv` adds a line per mod. Nothing is printed when the cache already matches.

## Cache

Project data fetched from Modrinth and CurseForge is kept in `.sculkr.cache.json`,
one entry per mod, keyed on the version the pack pins:

- **A mod whose pinned version changes is fetched again**; everything else is
  served from the cache, so a run with nothing new makes no API calls.
- **Mods removed from the pack are pruned** from the cache at the end of the run,
  and the file is only rewritten when something changed.
- **Each entry records the release it is pinned to**, which is what lets
  [`sculkr diff`](#tracking-changes) print version numbers instead of raw ids.
- **An unreadable cache, or one from an older sculkr, is not an error.** It is
  treated as empty and rebuilt, which costs one round of API calls.
- **The cache is written to the working directory** by default, or wherever
  `--cache` points. Resolving it against the pack root instead is tracked in
  [#12](https://github.com/justin-carver/sculkr/issues/12).

A cache left behind by `packwiz-modlist` (`.packwiz-modlist.cache.json` and
similar) is not read. `sculkr` warns when it finds one in the pack root, and
it can be deleted.

## JSON Export

`--json` writes one document describing the whole pack, for scripts, CI and
other tools. It needs the pack's `pack.toml`.

| Key              | Contents                                                                                  |
| ---------------- | ----------------------------------------------------------------------------------------- |
| `schema_version` | Currently `2`. Bumped whenever a key is removed, renamed or changes meaning.               |
| `pack`           | `pack.toml`: `name`, `author`, `version`, `pack_format`, and `versions` (Minecraft and loader). |
| `mods`           | Every `*.pw.toml` record: `name`, `filename`, `side`, `download` and `update`, plus the `category` folder and `path` it came from. |
| `projects`       | What Modrinth and CurseForge returned, tagged by host: `{"Modrinth": {...}}` or `{"CurseForge": {...}}`. |
| `settings`       | The `format`, `output` and `path` set in `.sculk` files. Secrets are never included.      |

Every key is `snake_case`. `--sort-by` and `--reverse` order `projects` the same
way they order a modlist.

## Formatting

`--format` / `-f` takes a string literal with `{PLACEHOLDER}` holes in it, one
per field the cache holds.

The default modpack output is (Markdown List format):

```
- [{NAME}]({URL}) - {DESCRIPTION}\n
```

```sh
# Markdown table rows
sculkr -f '| {NAME} | {AUTHORS} | {LICENSE_ID} |\n'
# HTML list-item anchor tags with a newline
sculkr -f '<li><a href="{URL}">{NAME}</a> — {DESC}</li>\n'
# Perhaps something a bit more complicated (see image below)
sculkr -f '| {INDEX}. | <img src="{ICON_URL}" width="128px" /> | <a href="{URL}">{NAME}</a><br/><code>{DESC}</code><br/><br/><i>by {AUTHORS_MD}</i> |\n'
```

![Complex Custom Formatting](.github/assets/complex-format.png)

Backslash escapes (`\n`, `\t`, `\r`, `\0`, `\\`, `\{`, `\}`) are resolved by
sculkr rather than by the shell, so quote the template and write `\n`
wherever you want a line break — nothing is appended for you. Placeholder names
are case-insensitive, and a bad template is rejected before any API calls are
made.

<details>
<summary><strong>Every placeholder</strong></summary>

| Placeholder               | Value                                                     |
| ------------------------- | --------------------------------------------------------- |
| `{ID}`                    | Project id (Modrinth base62, CurseForge numeric)          |
| `{SLUG}`                  | URL slug, e.g. `sodium`                                   |
| `{NAME}`, `{TITLE}`       | Project title                                             |
| `{DESCRIPTION}`, `{DESC}` | Short description / summary                               |
| `{URL}`                   | Project page on Modrinth/CurseForge                       |
| `{ICON_URL}`              | Project icon image                                        |
| `{SOURCE_URL}`            | Source repository                                         |
| `{ISSUES_URL}`            | Issue tracker                                             |
| `{WIKI_URL}`              | Wiki / documentation                                      |
| `{LICENSE}`               | License name                                              |
| `{LICENSE_ID}`            | License id, e.g. `MIT`                                    |
| `{LICENSE_URL}`           | License text                                              |
| `{AUTHORS}`               | Author names, or the owning organization, comma separated |
| `{AUTHOR_URLS}`           | Author pages, comma separated                             |
| `{AUTHORS_MD}`            | Authors as markdown links                                 |
| `{INDEX}`                 | This mod's position in the list, starting at 1            |

</details>

A placeholder with no value for a given mod renders as an empty string.
`sculkr --help` prints the same table, generated from the same source.

<details>
<summary><strong>Formatting notes</strong></summary>

- Line breaks _inside_ a value are collapsed to single spaces before
  substitution. Both Modrinth and CurseForge allow them in a description, and one arriving mid-entry
  would otherwise split a list item or table row across lines — so the only line
  breaks in the output are the ones your format asks for.

- CurseForge exposes no license anywhere in its public API, even though it is shown on the project page. `{LICENSE*}` is
  therefore empty for CurseForge mods.

- Modrinth credits a _Team_ rather than a list of users, so author names cost a
  bulk `/v2/teams` call, sorted owner-first to keep the credit line stable. A
  project owned by an _organization_ has an empty team and is credited to the
  organization instead (`Forgified Fabric API :: Sinytra`). That is the one place
  `sculkr` touches Modrinth's `/v3` API, which is documented as unstable; a
  failure there warns and leaves those authors empty rather than failing the run.

**Neither API lookup runs when every mod is already cached.**

</details>

> If you are getting rate-limited by an API, run `sculkr` once your mod changes
> are finished rather than after each one, so the cache is only refreshed once.

## Issues & Contributions

Bug reports, questions and suggestions are all welcome — open an
[issue](https://github.com/justin-carver/sculkr/issues), or send a PR.

See [CONTRIBUTING.md](CONTRIBUTING.md) for how to set up a development
environment, what CI checks against, and how releases are cut.

---

_<strong>sculkr</strong> began as a fork of [packwiz-modlist](https://github.com/Ricky12Awesome/packwiz-modlist)
by Ricky12Awesome, rewritten and renamed with their consent ([discussion](https://github.com/Ricky12Awesome/packwiz-modlist/issues/4)).
Most of it has since been rewritten from the ground up._

_Everything `packwiz-modlist` offered has been ported over, reworked where that
made more sense. Licensed under Apache-2.0; see [NOTICE](NOTICE)._
