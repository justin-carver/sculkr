# sculkr

<div align="center">

<img src=".github/assets/sculk-chute.png" width="25%"/>

[![crates.io](https://img.shields.io/crates/v/sculkr?style=flat-square&logo=rust&logoColor=white&label=crates.io)](https://crates.io/crates/sculkr)
![Crates.io Total Downloads](https://img.shields.io/crates/d/sculkr?style=flat-square&logo=rust&color=blue&link=https%3A%2F%2Fcrates.io%2Fcrates%2Fsculkr)
[![release](https://img.shields.io/github/v/release/justin-carver/sculkr?style=flat-square&logo=github&label=release&sort=semver)](https://github.com/justin-carver/sculkr/releases/latest)

[![CI](https://img.shields.io/github/actions/workflow/status/justin-carver/sculkr/ci.yml?branch=main&style=flat-square&logo=githubactions&logoColor=white&label=CI)](https://github.com/justin-carver/sculkr/actions/workflows/ci.yml)
[![GitHub_Actions](https://img.shields.io/github/actions/workflow/status/justin-carver/sculkr/ci.yml?branch=main&style=flat-square&logo=github&logoColor=white&label=GitHub%20Actions)](https://github.com/justin-carver/sculkr/actions/workflows/ci.yml)

![install_size](https://img.shields.io/crates/size/sculkr?style=flat-square&logo=rust&label=install%20size&link=https%3A%2F%2Fcrates.io%2Fcrates%2Fsculkr)
[![msrv](https://img.shields.io/badge/MSRV-1.88%2B-b7410e?style=flat-square&logo=rust&logoColor=white)](https://github.com/justin-carver/sculkr/blob/main/Cargo.toml)
[![platforms](https://img.shields.io/badge/platforms-linux%20%7C%20macOS%20%7C%20Windows-blue?style=flat-square)](https://github.com/justin-carver/sculkr/releases/latest)
[![license](https://img.shields.io/crates/l/sculkr?style=flat-square&color=DFBE6F)](https://github.com/justin-carver/sculkr/blob/main/LICENSE)
[![PRs](https://img.shields.io/badge/Welcome!-brightgreen?style=flat-square&logoColor=white&label=PRs)](https://github.com/justin-carver/sculkr/actions/workflows/ci.yml)

A companion CLI application for `packwiz` that parses its output data to deliver advanced utility commands and extended features for Minecraft modpack development.

</div>

## Features

- **Modlist generation** from any [packwiz](https://packwiz.infra.link/) pack, Modrinth and CurseForge alike.
- **Custom modlist templates** via `--format`, across 16 project fields. See [Formatting](#formatting).
- **JSON export** of the entire pack with `--json`, for feeding other tools.
- **Folder-agnostic**, so all `resourcepacks/`, `shaderpacks/` and `datapacks/` are monitored too.
- **Cached per pinned version.** A fully cached run makes zero API calls.
- **Shareable config** in committed `.sculk` files, global and per-pack, centralized modpack configs.
- **Keeps `CF_API_KEY` out of your pack**, redacted in output and never processed in output files.
- **Pipes cleanly**, since every log line goes to stderr instead of into your modlist.

## Installation

Requires Rust **1.88** or newer (edition 2024).

### Dependencies

As of right now, this CLI tool **does not** make modifications to your modlist managed by `packwiz`. [We are currently considering this.](https://github.com/justin-carver/sculkr/issues/10) `sculkr` simply elevates ways to interact with already generated `packwiz` modlists. Due to this, [packwiz](https://github.com/packwiz/packwiz) is considered it's only external dependency (aside from the Cargo deps, of course).

### From crates.io

```sh
cargo install sculkr
```

### Prebuilt binaries

Every tagged release ships archives for Linux, macOS, and Windows on the
[releases page](https://github.com/justin-carver/sculkr/releases/latest) — x86_64 and
aarch64 for Linux and macOS, x86_64 for Windows. Download the archive for your
platform, extract it, and put `sculkr` somewhere on your `PATH`.

Each release also carries `SHA256SUMS.txt` and a build provenance attestation:

```sh
# Checksums
sha256sum --check --ignore-missing SHA256SUMS.txt

# Provenance — proves the archive came from this repo's release workflow
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
path   = "."                            # anywhere in the pack, same as --path
output = "modlist.md"                   # same as --output; omit for stdout
format = '- [{NAME}]({URL}) - {DESC}\n'  # same as --format
verbose = 1                             # 0-3, same as -v/-vv/-vvv
quiet   = false                         # same as --quiet
json    = false                         # same as --json; overrides format
sort-by = "name"                        # same as --sort-by; any placeholder but INDEX
reverse = false                         # same as --reverse; Z-A instead of A-Z

[secrets]                               # read the Secrets section below first
cf-api-key = '$2a$10$...'               # same as CF_API_KEY
```

Two files are read, and then merged, in descending order. Anything passed on the command line takes priority:

| Location                                         | Applies to                       |
| ------------------------------------------------ | -------------------------------- |
| `~/.config/sculkr/.sculk` (or the OS equivalent) | every pack on the machine        |
| `<pack root>/.sculk`                             | that pack, overriding the global |

`sculkr config` prints which files were found. Keep in mind:

- **Relative paths resolve against the file they are written in**, not the
  working directory, so a `path` in the global config means the same thing from
  anywhere.
- **Use single quotes for `format`.** TOML resolves `\n` inside double quotes;
  a literal string hands the escape through to the
  formatter, which is where the [formatting notes](#formatting-notes) apply.
- **An unknown key is a warning, not an error.** If the key does not exist, or does not
  parse due to a typo, then it will not be read.
- **.sculk files <u>should</u> be pushed to Git and bundled with modpacks, .env should not.** This allows modpack maintainers to create configurations that are shared between users. That is also exactly why the `[secrets]` table below needs care.

### Secrets in `.sculk`

A `.sculk` may carry a CurseForge key, or potentially future secrets, so one file can serve every pack on the machine instead of a `.env` per pack:

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

`sculkr` reads its secrets from the environment at run time, and loads a `.env` from the working directory, if one is present. Copy [`.env.example`](.env.example), modify it's contents, and rename to `.env` to get started.

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

# Change the path to something relative or absolute
sculkr -p ~/modpack/mods

# Write it to a file instead of stdout
sculkr -p mods -o modlist.md

# Apply a custom template, one mod per line
sculkr -p mods -f '{INDEX}. {NAME} ({SLUG}) - {LICENSE_ID}\n'

# Sort by author instead of name (A-Z), or Z-A with --reverse
sculkr -p mods --sort-by authors
sculkr -p mods --sort-by authors --reverse

# Debug logging on stderr, modlist still outputs cleanly to file
sculkr -p mods -vv -o modlist.md

# Just the mod names, nothing else
sculkr -p mods -q -f '{NAME}\n'

# Version, authors, repository
sculkr about

# View runtime information about sculkr and the modpack
sculkr config
```

`-p` defaults to the current directory, so point it at wherever your `*.pw.toml`
files live. Logs go to stderr, so a redirected or piped modlist stays clean.

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

A placeholder with no value for a given mod renders as an empty string.

### Formatting Notes

- Line breaks _inside_ a value are collapsed to single spaces before
  substitution. Both Modrinth and CurseForge allow them in a description, and one arriving mid-entry
  would otherwise split a list item or table row across lines — so the only line
  breaks in the output are the ones custom format's request.

- CurseForge exposes no license anywhere in its public API, even though it is shown on the project page. `{LICENSE*}` is
  therefore empty for CurseForge mods.

- Modrinth author names cost extra lookups, because a project is credited to a
  _Team_ rather than to a list of users:
    - Team members come from a bulk `/v2/teams` call, sorted owner-first so the
      credit line is stable between runs.
    - A project owned by an _organization_ has an empty team, and the site credits
      the organization — so `{AUTHORS}` gets the organization
      (`Forgified Fabric API :: Sinytra`). This is the one place `sculkr` touches
      Modrinth's `/v3` API, which is documented as unstable, so a failure there
      logs a warning and leaves those authors empty rather than failing the run... perhaps it'll be stable later.

**Neither API lookup runs when every mod is already cached.**

> If you are getting rate-limited by an API, it is advisible to update the cache only once all mod changes are finished.

`sculkr --help` prints the same table, generated from the same source.

## Issues & Contributions

If you encounter any bugs, have questions, or notice areas for improvement, your feedback is highly welcome! Please feel free to open an issue to report problems or suggest enhancements. If you'd like to contribute directly, you can also submit a PR with your proposed fixes or updates, and I'll get to it when I can.

See [CONTRIBUTING.md](CONTRIBUTING.md) for how to set up a development environment, what CI checks against, and how releases are cut.

---

_<strong>sculkr</strong> began as a fork of [packwiz-modlist](https://github.com/Ricky12Awesome/packwiz-modlist)
by Ricky12Awesome, rewritten and renamed with their consent ([discussion](https://github.com/Ricky12Awesome/packwiz-modlist/issues/4)).
Large portions and functionality have been rewritten from the ground-up, with more and more features being added monthly._

_I am currently going through the original `packwiz-modlist` args and attempting to port those over, changing functionality where it makes most sense, adding things here or there. If you have an idea, or would like something yourself, let me know!_

_Licensed under Apache-2.0; see [NOTICE](NOTICE)._
