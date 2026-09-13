//! The `.sculk` config file.
//!
//! TOML, and every key optional. A file is a set of defaults for the global
//! flags plus a `[secrets]` table, nothing more, so a config that predates a
//! release still loads and an unknown key is a warning rather than a failed
//! run.
//!
//! Two locations are read, global first and then the pack's own, so a
//! machine-wide default can be overridden per pack:
//!
//! ```text
//! ~/.config/sculkr/.sculk   defaults for every pack on this machine
//! <pack root>/.sculk        this pack only, wins over the global file
//! ```
//!
//! Anything passed on the command line wins over both.
//!
//! Secrets live in their own `[secrets]` table, and the environment wins over
//! every file, because a pack's `.sculk` travels with the pack.

use std::{
    collections::BTreeMap,
    fmt, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use clap::{ColorChoice, ValueEnum};
use directories::ProjectDirs;
use serde::{Deserialize, Deserializer, de};

use crate::{
    env::{CF_API_KEY, Secret},
    error::{Error, IoContext},
    format::SortKey,
};

/// **Note:** If a future config file changes the `.sculk` file extension, after
/// this has been merged, that would be considered a breaking change and MUST be
/// noted for future releases.
pub const CONFIG_FILE_NAME: &str = ".sculk";

/// Grabs config path of .sculk file, regardless of OS or XDG base dir.
///
/// `None` when the platform has no home directory to hang a config dir off,
/// which is normal in containers and on CI.
pub fn global_path() -> Option<PathBuf> {
    ProjectDirs::from("", "", env!("CARGO_BIN_NAME"))
        .map(|dirs| dirs.config_dir().join(CONFIG_FILE_NAME))
}

/// Which of the two files a config came from.
///
/// The pack's file ships with the pack and is normally committed, which is the
/// entire reason a secret in it is worth saying something about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Global,
    Pack,
}

/// The pack's own `.sculk`, sitting next to `pack.toml`.
pub fn local_path<P>(pack_root: P) -> PathBuf
where
    P: AsRef<Path>,
{
    pack_root.as_ref().join(CONFIG_FILE_NAME)
}

/// Defaults for the global flags, all optional.
///
/// Keys are the long flag names. Unset means "no opinion", which is what lets
/// two files merge and the command line win over the result.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    /// The packwiz root. Relative paths resolve against the file's own
    /// directory, so a global config cannot silently mean two things.
    pub path: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub format: Option<String>,
    pub verbose: Option<u8>,
    pub quiet: Option<bool>,
    pub json: Option<bool>,
    #[serde(default, deserialize_with = "color_choice")]
    pub color_mode: Option<ColorChoice>,
    pub sort_by: Option<SortKey>,
    pub reverse: Option<bool>,

    #[serde(default)]
    pub secrets: Secrets,

    /// Everything else in the file. Kept so a typo can be named in a warning
    /// instead of parsed into silence.
    #[serde(flatten)]
    unknown: BTreeMap<String, toml::Value>,
}

/// `ColorChoice` from has no serde impls of its own, so it goes through clap's
/// parser instead, which keeps the file accepting what `--color-mode` does.
fn color_choice<'de, D>(deserializer: D) -> Result<Option<ColorChoice>, D::Error>
where
    D: Deserializer<'de>,
{
    let name = String::deserialize(deserializer)?;

    <ColorChoice as ValueEnum>::from_str(&name, true)
        .map(Some)
        .map_err(|_| {
            let expected = ColorChoice::value_variants()
                .iter()
                .filter_map(ValueEnum::to_possible_value)
                .map(|value| value.get_name().to_owned())
                .collect::<Vec<_>>()
                .join(", ");

            de::Error::custom(format!(
                "unknown color mode \"{name}\", expected one of: {expected}"
            ))
        })
}

/// Credentials a `.sculk` may carry.
///
/// Kept in a table of their own so the header is a hint in itself: everything
/// above it is a flag anyone may read, everything below it is not.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Secrets {
    /// A `CurseForge` API key, the same value as `CF_API_KEY`.
    pub cf_api_key: Option<Secret>,

    #[serde(flatten)]
    unknown: BTreeMap<String, toml::Value>,
}

impl Secrets {
    fn overlay(&mut self, other: Self) {
        let Self {
            cf_api_key,
            unknown,
        } = other;

        self.cf_api_key = cf_api_key.or_else(|| self.cf_api_key.take());
        self.unknown.extend(unknown);
    }

    /// Whether this file carries a key worth resolving, and worth warning about.
    fn has_api_key(&self) -> bool {
        self.cf_api_key.as_ref().is_some_and(|key| !key.is_blank())
    }
}

impl Config {
    /// Reads one file. `Ok(None)` means there was nothing there to read, which
    /// is the usual case and not a problem.
    pub fn load<P>(file: P) -> Result<Option<Self>, Error>
    where
        P: AsRef<Path>,
    {
        let file = file.as_ref();

        let text = match fs::read_to_string(file) {
            Ok(text) => text,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err).path_ctx(file, "reading config"),
        };

        let mut config: Self = toml::from_str(&text)
            .map_err(|err| Error::TomlFile(crate::util::resolve_for_display(file), err))?;

        config.rebase(file.parent().unwrap_or_else(|| Path::new(".")));

        Ok(Some(config))
    }

    /// Makes this file's relative paths mean what they say in this file.
    fn rebase(&mut self, dir: &Path) {
        for path in [&mut self.path, &mut self.output].into_iter().flatten() {
            if path.is_relative() {
                *path = dir.join(&*path);
            }
        }
    }

    /// Folds `other` in on top of self: any key it sets replaces ours.
    fn overlay(&mut self, other: Self) {
        let Self {
            path,
            output,
            format,
            verbose,
            quiet,
            json,
            color_mode,
            sort_by,
            reverse,
            secrets,
            unknown,
        } = other;

        self.path = path.or_else(|| self.path.take());
        self.output = output.or_else(|| self.output.take());
        self.format = format.or_else(|| self.format.take());
        self.verbose = verbose.or(self.verbose);
        self.quiet = quiet.or(self.quiet);
        self.json = json.or(self.json);
        self.color_mode = color_mode.or(self.color_mode);
        self.sort_by = sort_by.or(self.sort_by);
        self.reverse = reverse.or(self.reverse);
        self.secrets.overlay(secrets);
        self.unknown.extend(unknown);
    }
}

/// Where the `CurseForge` key in effect came from.
///
/// Reported by `sculkr config`, because "which of my three keys is this" is
/// otherwise unanswerable without exposing the key itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    /// `CF_API_KEY`, exported or read out of a `.env`.
    Environment,
    /// The `[secrets]` table of this file.
    Config(PathBuf),
}

impl fmt::Display for KeySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Environment => f.write_str("the environment"),
            Self::Config(file) => write!(f, "{}", file.display()),
        }
    }
}

/// A pack's `.sculk` is meant to be committed and shipped with the pack, so a
/// key written into one is a key handed to everybody who downloads it.
fn warn_about_committed_key(file: &Path) -> String {
    let elsewhere = global_path().map_or_else(
        || ".env".to_owned(),
        |global| format!(".env or \"{}\"", global.display()),
    );

    format!(
        "{CF_API_KEY} is set in \"{}\". A pack {CONFIG_FILE_NAME} is normally committed and \
         shipped with the pack, so treat that key as public -- keep it in {elsewhere} instead",
        file.display()
    )
}

/// The merged config plus what it took to get there.
///
/// [`Config::load`] runs before logging is initialized (the file can set the
/// log level), so anything worth saying is held here and replayed once there is
/// somewhere to say it.
#[derive(Debug, Default)]
pub struct Loaded {
    pub config: Config,
    /// Files that existed and were read, in the order applied.
    pub sources: Vec<PathBuf>,
    pub warnings: Vec<String>,
    /// The file whose `[secrets]` set the key that survived the merge. Kept
    /// separately because [`Config::overlay`] keeps values, not their origins.
    secret_source: Option<PathBuf>,
}

impl Loaded {
    /// Reads the global file and then the pack's, merging as it goes.
    ///
    /// A file that cannot be read or parsed is reported and skipped. Refusing
    /// to run at all would leave a broken global config bricking every pack on
    /// the machine.
    pub fn discover<P>(pack_root: P) -> Self
    where
        P: AsRef<Path>,
    {
        let mut loaded = Self::default();

        let files = global_path()
            .map(|file| (file, Scope::Global))
            .into_iter()
            .chain(std::iter::once((local_path(pack_root), Scope::Pack)));

        for (file, scope) in files {
            match Config::load(&file) {
                Ok(Some(config)) => {
                    loaded.absorb(crate::util::resolve_for_display(&file), scope, config);
                }
                Ok(None) => {}
                // The message can run to several lines, so the verdict leads.
                Err(err) => loaded
                    .warnings
                    .push(format!("ignoring this {CONFIG_FILE_NAME}: {err}")),
            }
        }

        let unknown = loaded.config.unknown.keys().map(String::clone).chain(
            loaded
                .config
                .secrets
                .unknown
                .keys()
                .map(|key| format!("secrets.{key}")),
        );

        for key in unknown {
            loaded.warnings.push(format!(
                "unknown key \"{key}\" in {CONFIG_FILE_NAME}; ignoring it"
            ));
        }

        loaded
    }

    /// Folds one file that was read into the merged result.
    ///
    /// Split from [`Self::discover`] so the bookkeeping around a secret can be
    /// tested without two real files in two real directories.
    fn absorb(&mut self, file: PathBuf, scope: Scope, config: Config) {
        if config.secrets.has_api_key() {
            if scope == Scope::Pack {
                self.warnings.push(warn_about_committed_key(&file));
            }
            self.secret_source = Some(file.clone());
        }

        self.config.overlay(config);
        self.sources.push(file);
    }

    /// The `CurseForge` key in effect, and where it came from.
    ///
    /// The environment wins over both files: a key exported on this machine
    /// belongs to whoever is sitting at it, while one in a `.sculk` may have
    /// arrived with somebody else's modpack.
    pub fn curseforge_api_key(&self) -> Option<(Secret, KeySource)> {
        self.resolve_api_key(crate::env::curseforge_api_key())
    }

    /// Split out so the precedence can be tested without a process
    /// environment to set up and tear down around it.
    fn resolve_api_key(&self, from_env: Option<Secret>) -> Option<(Secret, KeySource)> {
        if let Some(key) = from_env {
            return Some((key, KeySource::Environment));
        }

        // Both are written together in `discover`, so a key without a source
        // cannot happen; matching on the pair beats unwrapping either.
        match (&self.config.secrets.cf_api_key, &self.secret_source) {
            (Some(key), Some(file)) if !key.is_blank() => {
                Some((key.clone(), KeySource::Config(file.clone())))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> Config {
        toml::from_str(toml).expect("valid config")
    }

    /// Round-tripped through the file format because that is the only way to
    /// build a [`Secret`], which is the point of it.
    fn secret(value: &str) -> Secret {
        parse(&format!("[secrets]\ncf-api-key = '{value}'"))
            .secrets
            .cf_api_key
            .expect("a key")
    }

    /// A [`Secret`] has no `PartialEq` on purpose, so assertions go through
    /// the value it hands out rather than through the secret itself.
    fn resolved(loaded: &Loaded, from_env: Option<Secret>) -> Option<(String, KeySource)> {
        loaded
            .resolve_api_key(from_env)
            .map(|(key, source)| (key.expose().to_owned(), source))
    }

    /// The two files as `discover` would have folded them in.
    fn loaded(global: &str, pack: &str) -> Loaded {
        let mut loaded = Loaded::default();

        loaded.absorb(
            PathBuf::from("/home/user/.config/sculkr/.sculk"),
            Scope::Global,
            parse(global),
        );
        loaded.absorb(
            PathBuf::from("/home/user/modpack/.sculk"),
            Scope::Pack,
            parse(pack),
        );

        loaded
    }

    #[test]
    fn an_empty_file_is_a_valid_config() {
        let config = parse("");

        assert!(config.path.is_none());
        assert!(config.format.is_none());
        assert!(config.unknown.is_empty());
    }

    #[test]
    fn every_flag_round_trips_from_the_file() {
        let config = parse(
            r#"
            path = "packs/skyblock"
            output = "modlist.md"
            format = '- {NAME}\n'
            verbose = 2
            quiet = true
        "#,
        );

        assert_eq!(config.path, Some(PathBuf::from("packs/skyblock")));
        assert_eq!(config.output, Some(PathBuf::from("modlist.md")));
        assert_eq!(config.format.as_deref(), Some(r"- {NAME}\n"));
        assert_eq!(config.verbose, Some(2));
        assert_eq!(config.quiet, Some(true));
    }

    /// Spelled the way `--color-mode` spells it, in any case.
    #[test]
    fn a_color_mode_is_read_the_way_clap_reads_it() {
        assert_eq!(
            parse("color-mode = \"never\"").color_mode,
            Some(ColorChoice::Never)
        );
        assert_eq!(
            parse("color-mode = \"Always\"").color_mode,
            Some(ColorChoice::Always)
        );
        assert!(parse("").color_mode.is_none());
    }

    #[test]
    fn an_unknown_color_mode_names_the_valid_ones() {
        let err = toml::from_str::<Config>("color-mode = \"rainbow\"")
            .map(|_| ())
            .unwrap_err()
            .to_string();

        assert!(err.contains("\"rainbow\""), "{err}");
        assert!(err.contains("auto, always, never"), "{err}");
    }

    #[test]
    fn sorting_is_read_from_the_file() {
        let config = parse("sort-by = \"authors\"\nreverse = true");

        assert_eq!(config.sort_by, "AUTHORS".parse().ok());
        assert_eq!(config.reverse, Some(true));
        assert!(config.unknown.is_empty());
    }

    /// Unlike an unknown key, a known key with a bad value fails the file, the
    /// same as `color-mode` does, and says what it would have taken.
    #[test]
    fn an_unsortable_field_names_the_valid_ones() {
        let err = toml::from_str::<Config>("sort-by = \"index\"")
            .map(|_| ())
            .unwrap_err()
            .to_string();

        assert!(err.contains("INDEX"), "{err}");
    }

    /// A key we do not know is a typo or a newer release's key. Neither is
    /// worth failing a run over, but both are worth mentioning.
    #[test]
    fn an_unknown_key_is_kept_rather_than_rejected() {
        let config = parse("fromat = \"oops\"");

        assert!(config.format.is_none());
        assert_eq!(config.unknown.keys().collect::<Vec<_>>(), vec![
            &"fromat".to_owned()
        ]);
    }

    #[test]
    fn a_local_key_wins_over_the_global_one() {
        let mut global = parse("output = \"global.md\"\nformat = \"{NAME}\"");
        global.overlay(parse("output = \"local.md\""));

        assert_eq!(global.output, Some(PathBuf::from("local.md")));
        // Untouched by the local file, so the global value survives.
        assert_eq!(global.format.as_deref(), Some("{NAME}"));
    }

    #[test]
    fn a_key_is_read_out_of_the_secrets_table() {
        let config = parse("[secrets]\ncf-api-key = '$2a$10$abcdefghijklmnope345'");

        assert_eq!(
            config.secrets.cf_api_key.as_ref().map(Secret::expose),
            Some("$2a$10$abcdefghijklmnope345")
        );
        assert!(config.secrets.unknown.is_empty());
        // The key is not a flag, so it must not land in the top-level catch-all
        // and be reported as a typo.
        assert!(config.unknown.is_empty());
    }

    /// A key that reaches a log or a panic message is a key that reaches CI.
    #[test]
    fn a_key_stays_redacted_when_the_whole_config_is_printed() {
        let config = parse("[secrets]\ncf-api-key = 'hunter2'");
        let printed = format!("{config:?}");

        assert!(!printed.contains("hunter2"), "{printed}");
        assert!(printed.contains("<redacted>"), "{printed}");
    }

    #[test]
    fn an_unknown_secret_is_named_under_its_table() {
        let config = parse("[secrets]\ncf-api-kye = 'oops'");

        assert!(config.secrets.cf_api_key.is_none());
        assert_eq!(config.secrets.unknown.keys().collect::<Vec<_>>(), vec![
            &"cf-api-kye".to_owned()
        ]);
    }

    /// The pack's file is normally committed, so a key in it is a key anyone
    /// who downloads the pack now has.
    #[test]
    fn a_key_in_the_packs_file_is_warned_about() {
        let loaded = loaded("", "[secrets]\ncf-api-key = 'hunter2'");

        assert_eq!(loaded.warnings.len(), 1, "{:?}", loaded.warnings);
        assert!(loaded.warnings[0].contains("/home/user/modpack/.sculk"));
        // The warning names the key, never its value.
        assert!(!loaded.warnings[0].contains("hunter2"));
    }

    /// The global file is the place this feature exists for: one key, every
    /// pack on the machine, nothing to leak.
    #[test]
    fn a_key_in_the_global_file_is_not_warned_about() {
        let loaded = loaded("[secrets]\ncf-api-key = 'hunter2'", "");

        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(
            resolved(&loaded, None),
            Some((
                "hunter2".to_owned(),
                KeySource::Config(PathBuf::from("/home/user/.config/sculkr/.sculk"))
            ))
        );
    }

    /// Nothing in a file should displace the key belonging to whoever is
    /// sitting at the machine.
    #[test]
    fn the_environment_wins_over_every_file() {
        let loaded = loaded("[secrets]\ncf-api-key = 'from-the-global-file'", "");

        assert_eq!(
            resolved(&loaded, Some(secret("from-the-environment"))),
            Some(("from-the-environment".to_owned(), KeySource::Environment))
        );
    }

    /// Between the files themselves the usual rule holds, and the source
    /// reported has to follow the key that actually won.
    #[test]
    fn the_packs_key_wins_over_the_global_one() {
        let loaded = loaded(
            "[secrets]\ncf-api-key = 'global'",
            "[secrets]\ncf-api-key = 'pack'",
        );

        assert_eq!(
            resolved(&loaded, None),
            Some((
                "pack".to_owned(),
                KeySource::Config(PathBuf::from("/home/user/modpack/.sculk"))
            ))
        );
    }

    /// An empty key would otherwise be sent as an empty header, which reads as
    /// a mangled key rather than as a missing one.
    #[test]
    fn a_blank_key_is_the_same_as_no_key() {
        let loaded = loaded("[secrets]\ncf-api-key = '   '", "");

        assert_eq!(resolved(&loaded, None), None);
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    }

    /// A relative path in a global config would otherwise point at whatever
    /// directory the user happened to run from.
    #[test]
    fn relative_paths_resolve_against_the_file() {
        let mut config = parse("path = \"packs/skyblock\"\noutput = \"/tmp/modlist.md\"");
        config.rebase(Path::new("/home/user/.config/sculkr"));

        assert_eq!(
            config.path,
            Some(PathBuf::from("/home/user/.config/sculkr/packs/skyblock"))
        );
        // Already absolute, so it is left exactly as written.
        assert_eq!(config.output, Some(PathBuf::from("/tmp/modlist.md")));
    }
}
