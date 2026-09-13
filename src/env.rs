//! After some research, looks like GitHub Action artifacts are a great way
//! to accidentally leak secrets, so let's fix that here.
//!
//! None of these are resolved while compiling. The compiler never sees the
//! values, so they cannot end up as string literals inside a published binary.
//! `build.rs` enforces that: it fails the build if a compile-time variable
//! lookup shows up anywhere under `src/`.

use std::{fmt, path::PathBuf};

use serde::{Deserialize, Deserializer};

use crate::error::Error;

pub const CF_API_KEY: &str = "CF_API_KEY";

/// A value that must not reach logs, errors, or serialized output.
///
/// Custom implements for `Debug` and `Display` both redact, so it stays hidden even when something
/// wraps it in a message on a path nobody thought about.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    /// Hands out the real value. Call this at the point of use, never earlier.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Whitespace and nothing else is the same as no value at all
    pub fn is_blank(&self) -> bool {
        self.0.trim().is_empty()
    }

    /// The first and last few characters, e.g. `$2a$...e345`.
    pub fn fingerprint(&self) -> String {
        const KEEP: usize = 4;

        let chars: Vec<char> = self.0.chars().collect();

        if chars.len() < KEEP * 3 {
            return "<redacted>".to_owned();
        }

        // let head: String = chars[..KEEP].iter().collect();
        let head: String = chars
            .get(..KEEP)
            .map_or_else(String::new, |head| head.iter().collect());

        let tail_start = chars.len().saturating_sub(KEEP);
        let tail: String = chars
            .get(tail_start..)
            .map_or_else(String::new, |tail| tail.iter().collect());

        format!("{head}...{tail}")
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Written out by hand rather than derived, so nothing can quietly pick up a
/// `Serialize` alongside it and write the value back out again.
impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self)
    }
}

/// Pulls `.env` into the process environment for local runs. Absent file is
/// fine; anything already set in the real environment wins.
pub fn load_dotenv() {
    match dotenvy::dotenv() {
        Ok(path) => log::debug!("loaded environment from \"{}\"", path.display()),
        Err(err) if err.not_found() => {}
        Err(err) => log::warn!("could not read .env: {err}"),
    }
}

/// The key as the process environment has it, `.env` included.
///
/// Only the environment. A key from a `.sculk` file is resolved by
/// [`crate::config::Loaded::curseforge_api_key`], which falls back to this.
pub fn curseforge_api_key() -> Option<Secret> {
    std::env::var(CF_API_KEY)
        .ok()
        .map(Secret)
        .filter(|key| !key.is_blank())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_redacts_itself_in_both_formats() {
        let secret = Secret("hunter2".to_owned());

        assert_eq!(format!("{secret}"), "<redacted>");
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
        assert_eq!(secret.expose(), "hunter2");
    }

    #[test]
    fn a_fingerprint_shows_only_the_ends_of_a_long_value() {
        let secret = Secret("$2a$10$abcdefghijklmnope345".to_owned());

        assert_eq!(secret.fingerprint(), "$2a$...e345");
    }

    #[test]
    fn a_secret_is_blank_only_when_there_is_nothing_in_it() {
        assert!(Secret(String::new()).is_blank());
        assert!(Secret("  \n ".to_owned()).is_blank());
        assert!(!Secret("hunter2".to_owned()).is_blank());
    }

    /// Four characters off each end of a short value is most of the value.
    #[test]
    fn a_short_value_is_not_fingerprinted_at_all() {
        assert_eq!(Secret("hunter2".to_owned()).fingerprint(), "<redacted>");
        assert_eq!(Secret(String::new()).fingerprint(), "<redacted>");
    }
}
