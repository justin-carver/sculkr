# Contributing to sculkr

Thanks for taking an interest! Bug reports, ideas, and pull requests are all
welcome. This is a small project, so nothing here is heavy process — it's mostly
a description of what CI checks so you can run the same things locally before
opening a PR.

## Reporting bugs and ideas

Open an [issue](https://github.com/justin-carver/sculkr/issues). For a bug, the
most useful report includes:

- what you ran (the full `sculkr` invocation, including any `--format` string),
- what you expected and what happened instead,
- the output of the same command with `-vv`, which turns logging all the way up to trace.

Please redact `CF_API_KEY` if it ever shows up in output — it shouldn't, since
the key is wrapped in a `Secret` that redacts itself in both `Display` and
`Debug`, but check anyway.

Feature ideas are welcome too; the [open issues](https://github.com/justin-carver/sculkr/issues)
are a good sense of where things are headed.

## Development setup

You need two toolchains: **stable** for building and testing, **nightly** for
formatting only.

```sh
rustup toolchain install stable nightly
rustup component add rustfmt clippy --toolchain nightly

git clone https://github.com/justin-carver/sculkr.git
cd sculkr
cargo build
```

The minimum supported Rust version is **1.85** (edition 2024), declared as
`rust-version` in `Cargo.toml`.

To run against a real pack, point `--path` at it: `cargo run -- -p ~/modpack`.
The flag accepts any path inside a pack, since the root is found by searching
upward for `pack.toml`.

A pack holding CurseForge mods also needs a key. Copy `.env.example` to `.env`
and fill in `CF_API_KEY`. See [Configuration](README.md#configuration) for the
precedence rules and the single-quotes-around-`CF_API_KEY` trap.

## Before you open a PR

CI runs these on every push and pull request, so running them locally saves a
round trip:

```sh
just             # fmt, clippy, docs, tests -- everything CI's lint and test jobs run
just pre-push    # the above plus the MSRV check
```

The [`justfile`](justfile) is the single source of truth for what CI checks, and
pins the same toolchains, so a green `just` means a green pipeline. Individual
recipes are available too:

| Recipe | What it runs |
| --- | --- |
| `just fmt` | `cargo +nightly fmt --all --check` |
| `just fmt-fix` | The same, rewriting files instead of reporting |
| `just lint` | `cargo +stable clippy --locked --all-targets --all-features -- -D warnings` |
| `just docs` | `cargo +stable doc --locked --no-deps --all-features` under `RUSTDOCFLAGS: -D warnings` |
| `just test` | `cargo +stable nextest run --locked --all-features` |
| `just msrv` | `cargo +1.88.0 check --locked --all-targets --all-features` |

`just` comes from [casey/just](https://github.com/casey/just) and `cargo nextest`
from [cargo-nextest](https://nexte.st/). Plain `cargo test` works fine locally if
you'd rather not install nextest.

Every recipe clears `RUSTFLAGS`. A personal `~/.cargo/config.toml` carrying
nightly-only `-Z` flags otherwise makes each `+stable` and `+1.88.0` invocation
fail before it compiles anything.

Formatting is nightly-only on purpose. `.rustfmt.toml` sets a handful of
nightly options (`group_imports`, `imports_granularity`, `wrap_comments`, and
friends); stable rustfmt warns about each and then ignores them, so `cargo fmt`
on stable will leave the file in a state CI rejects. Use `cargo +nightly fmt`.

## Things the codebase cares about

A few conventions that aren't obvious from reading a single file:

- **No compile-time environment lookups.** `build.rs` scans `src/` and fails the
  build if it finds one. Anything baked in at compile time ends up as a
  plaintext string inside every published release artifact, which is exactly how
  API keys leak. Read values at run time through `crate::env` instead.
- **Secrets go through `Secret`.** It redacts itself in `Display` and `Debug`,
  so it stays hidden even when some future error path wraps it in a message.
  Call `.expose()` at the point of use and never earlier.
- **API failures degrade where they reasonably can.** The Modrinth `/v3`
  organization lookup is the standing example: `/v3` is documented as unstable,
  so a failure there logs a warning and leaves authors empty rather than killing
  the whole run.
- **`--help` is generated from the placeholder table in `src/format.rs`.** If
  you add a placeholder, add it there rather than into the help text, and add a
  matching row to the README's placeholder table, which is written by hand.
- **Output line breaks come only from the format string.** Values are collapsed
  to single spaces before substitution, because a description containing a
  newline would otherwise split a table row across lines.
- **Test suites live in `src/tests/`, one file per module.** The module under
  test declares its suite with `#[cfg(test)] #[path = "tests/<module>.rs"] mod
  tests;`, so the suite stays a child module and can reach private items
  without widening their visibility. Fixtures more than one suite needs go in
  `src/tests/support.rs`. Suites still written inline in their module (`args`,
  `config`, `env`, `format`, `request` and the parsers) move over as they grow.

## Commits and changelog

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/)
(`feat:`, `fix:`, `chore:`, `docs:`, …). Keep them in the imperative mood.

`CHANGELOG.md` is generated from these commits by
[git-cliff](https://git-cliff.org/), so the commit *is* the changelog entry.
Nothing is written into the file by hand any more.

Two things follow from that:

- **The subject line becomes the bullet, and only the subject line.** Write it so
  it reads well on a release page, not just in `git log`. Keep a Changelog
  entries are one line each, so the body is never rendered into the changelog.
- **The body is for the reader of `git log`.** Write it as a Markdown bullet
  list, in the imperative mood, with commands, files and flags in backticks.
  State what the commit does rather than why it was done that way.

Do not put a gitmoji in the subject. The shortcode is not part of the
Conventional Commits spec; `cliff.toml` still strips one on the way in so that
commits predating this rule render correctly.

```
feat(export): add JSON export for modlists

- Add a `--json` flag that emits the whole pack as one document.
- Carry a `schema_version` so consumers can branch on the shape.
```

Types map onto [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
sections: `feat` to Added, `fix` to Fixed, `refactor`/`perf`/`docs` to Changed,
`deprecate` to Deprecated, `remove` to Removed, `sec` to Security. A commit of
any type carrying a `security` scope, such as `ci(security)`, also lands in
Security. `chore`, `ci`, `build`, `test` and `style` are dropped, as are merge
and release commits. The mapping lives in [`cliff.toml`](cliff.toml).

A release whose commits were all dropped is a maintenance release, and
`scripts/changelog.sh` writes it a note saying so rather than an empty section.

To see what the next release would look like at any point:

```sh
git cliff --unreleased
```

Entries for 0.2.0 and earlier were written by hand and are left frozen; only new
sections are generated.

## Releases

Maintainer-only, recorded here so the process isn't folklore.

Releases go through [cargo-release](https://github.com/crate-ci/cargo-release),
configured in `[package.metadata.release]`:

```sh
cargo release <patch|minor|major>
```

That runs [`scripts/changelog.sh`](scripts/changelog.sh) as a pre-release hook,
which asks git-cliff for the commits since the last tag, splices the new section
into `CHANGELOG.md` below the `<!-- next-header -->` marker, and rebuilds the
compare links at the bottom. Then it commits as `release vX.Y.Z`, tags `vX.Y.Z`,
and pushes. Requires `cargo install git-cliff`.

Dates are stamped in `America/Chicago`, set in `cliff.toml`. git-cliff's own
`{{ date }}` is UTC, which is what put 0.2.0 on the wrong day.

Add `--dry-run` to print the section that would be written without touching the
file.

Pushing the tag triggers [`tagged_release.yml`](.github/workflows/tagged_release.yml),
which re-runs lint and tests, builds all five targets, packages archives with
`SHA256SUMS.txt`, attaches a build provenance attestation, publishes the GitHub
release with this version's `CHANGELOG.md` section as the release body (GitHub
appends its own generated commit list underneath), and then publishes to
crates.io via OIDC — no long-lived registry token
lives in the repo. `cargo-release` itself is configured with `publish = false`
precisely so that the workflow owns that step.

A tag whose version doesn't match `Cargo.toml` fails the workflow's first job on
purpose. Running the workflow manually (`workflow_dispatch`) builds and verifies
everything but publishes nothing, which is a good way to test changes to it.

## License

By contributing, you agree that your contributions are licensed under the
[Apache License 2.0](LICENSE), the same as the rest of the project. sculkr began
as a fork of [packwiz-modlist](https://github.com/Ricky12Awesome/packwiz-modlist);
see [NOTICE](NOTICE) for the attribution that carries with it.
