//! Fixtures shared by more than one suite in `src/tests/`.
//!
//! Anything only one suite needs stays in that suite.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    Mod,
    parser::{ParsedCurseForgeId, ParsedModrinthId},
    request::{Author, License},
};

/// A scratch directory for one test, removed again when it drops.
///
/// Every test that touches disk gets its own, so tests stay independent
/// whether they run as threads under `cargo test` or as processes under
/// nextest.
pub struct TempDir(PathBuf);

impl TempDir {
    /// `name` only has to be unique across the suite; the process id keeps two
    /// concurrent runs of it apart.
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("sculkr-test-{}-{name}", std::process::id()));

        // A previous run that panicked before dropping may have left one behind.
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch directory");

        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Where a test's cache file goes, for suites that only need the one.
    pub fn cache_file(&self) -> PathBuf {
        self.join("cache.json")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Best effort: a stray directory under the temp dir is not worth a failure.
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A fully populated [`Mod`], so anything that stores or prints one exercises
/// every field.
pub fn sample_mod(id: &str, title: &str) -> Mod {
    Mod {
        id: id.into(),
        slug: title.to_lowercase(),
        title: title.into(),
        description: format!("{title} does things"),
        mod_url: format!("https://modrinth.com/mod/{id}"),
        license: Some(License {
            id: "MIT".into(),
            name: "MIT License".into(),
            url: None,
        }),
        authors: vec![Author {
            name: "someone".into(),
            url: "https://modrinth.com/user/someone".into(),
        }],
        icon_url: Some(format!("https://cdn.modrinth.com/{id}.webp")),
        source_url: Some(format!("https://github.com/someone/{id}")),
        issues_url: None,
        wiki_url: None,
    }
}

/// A Modrinth mod as the parser hands it over, pinned to `version` and naming
/// no release. See [`modrinth_release`].
pub fn modrinth(id: &str, version: &str) -> ParsedModrinthId {
    ParsedModrinthId {
        cache_id: version.into(),
        id: id.into(),
        version_name: None,
    }
}

/// [`modrinth`], pinned to `version` and naming the release it resolves to.
pub fn modrinth_release(id: &str, version: &str, release: &str) -> ParsedModrinthId {
    ParsedModrinthId {
        version_name: Some(release.into()),
        ..modrinth(id, version)
    }
}

/// A `CurseForge` mod as the parser hands it over, pinned to `file_id`.
pub fn curseforge(id: i32, file_id: &str) -> ParsedCurseForgeId {
    ParsedCurseForgeId {
        cache_id: file_id.into(),
        id,
        version_name: None,
    }
}

/// [`curseforge`], pinned to `file_id` and naming the release it resolves to.
pub fn curseforge_release(id: i32, file_id: &str, release: &str) -> ParsedCurseForgeId {
    ParsedCurseForgeId {
        version_name: Some(release.into()),
        ..curseforge(id, file_id)
    }
}
