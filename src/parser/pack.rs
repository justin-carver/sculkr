//! Reads packwiz's own `pack.toml` -- the pack-level metadata sitting beside
//! the `*.pw.toml` files that [`super::packwiz`] scans.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{Error, error::IoContext};

pub const PACK_FILE_NAME: &str = "pack.toml";

/// Deserialized from packwiz's kebab-case, serialized back out as `snake_case`
/// so the export reads consistently. `[options]` is ignored.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all(deserialize = "kebab-case", serialize = "snake_case"))]
pub struct Pack {
    pub name: String,
    pub author: Option<String>,
    pub version: Option<String>,
    /// e.g. `packwiz:1.1.0`.
    #[serde(rename(deserialize = "pack-format", serialize = "pack-format"))]
    pub format: Option<String>,
    #[serde(default)]
    pub versions: PackVersions,

    /// Where the file manifest lives. Read, but kept out of the export, which
    /// describes the pack rather than packwiz's bookkeeping.
    #[serde(skip_serializing)]
    pub index: Option<PackIndex>,
}

/// The `[index]` table: which file lists everything in the pack.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all(deserialize = "kebab-case", serialize = "snake_case"))]
pub struct PackIndex {
    pub file: String,
    pub hash_format: Option<String>,
    pub hash: Option<String>,
}

/// `[versions]` holds `minecraft` plus whichever loader the pack uses.
///
/// The known loaders are named rather than collected so every export carries
/// the same keys, and a consumer can read `versions.fabric` without first
/// checking whether it exists. A loader packwiz gains later still rides along
/// in `other` rather than being dropped.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct PackVersions {
    pub fabric: Option<String>,
    pub forge: Option<String>,
    pub minecraft: Option<String>,
    pub neoforge: Option<String>,
    pub quilt: Option<String>,

    #[serde(flatten)]
    pub other: BTreeMap<String, String>,
}

impl PackVersions {
    /// The loader and its version, if the pack names one.
    pub fn loader(&self) -> Option<(&str, &str)> {
        [
            ("fabric", &self.fabric),
            ("forge", &self.forge),
            ("neoforge", &self.neoforge),
            ("quilt", &self.quilt),
        ]
        .into_iter()
        .find_map(|(name, version)| version.as_deref().map(|version| (name, version)))
        .or_else(|| {
            self.other
                .iter()
                .next()
                .map(|(name, version)| (name.as_str(), version.as_str()))
        })
    }
}

/// The directory holding `pack.toml`, searching `start` and then its ancestors.
///
/// `--path` may legitimately point anywhere inside a pack. packwiz keeps
/// `pack.toml` at the root while content sits in `mods/`, `resourcepacks/`, etc.
/// so the root is wherever that file first turns up going upward.
pub fn find_root<P>(start: P) -> Option<PathBuf>
where
    P: AsRef<Path>,
{
    let start = start.as_ref();
    let resolved = start.canonicalize();
    let from = resolved.as_deref().unwrap_or(start);

    from.ancestors()
        .find(|directory| directory.join(PACK_FILE_NAME).is_file())
        .map(Path::to_path_buf)
}

impl Pack {
    /// Looks for `pack.toml` in `start`, then in each ancestor.
    ///
    /// packwiz keeps `pack.toml` at the pack root while the `*.pw.toml` files
    /// live in `mods/`, `resourcepacks/` and so on. Pointing `--path` at the
    /// mods directory is the normal way to run, which puts the metadata a level
    /// or more above it.
    ///
    /// A `pack.toml` that exists but will not parse is reported and treated as
    /// absent, rather than taking down a run that may not have needed it.
    pub fn discover<P>(start: P) -> Option<Self>
    where
        P: AsRef<Path>,
    {
        let root = find_root(start)?;

        match Self::read(root.join(PACK_FILE_NAME)) {
            Ok(pack) => Some(pack),
            Err(err) => {
                log::warn!(
                    "ignoring the {PACK_FILE_NAME} in \"{}\": {err}",
                    root.display()
                );
                None
            }
        }
    }

    /// Reads one specific `pack.toml`.
    pub fn read<P>(path: P) -> Result<Self, Error>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        let data = std::fs::read_to_string(path).path_ctx(path, "read file")?;

        toml::from_str(&data).map_err(|err| Error::TomlFile(path.to_owned(), err))
    }

    pub fn load_from<P>(pack_root: P) -> Result<Self, Error>
    where
        P: AsRef<Path>,
    {
        let path: PathBuf = pack_root.as_ref().join(PACK_FILE_NAME);

        log::debug!("reading pack metadata from \"{}\"", path.display());

        let data = std::fs::read_to_string(&path).path_ctx(&path, "read file")?;

        toml::from_str(&data).map_err(|err| Error::TomlFile(path, err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A representative packwiz `pack.toml`, loader key and all.
    const SAMPLE: &str = r#"
name = "Example Pack"
author = "minecraft_steve"
version = "1.2.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = "deadbeef"

[versions]
minecraft = "1.20.1"
fabric = "0.14.21"
"#;

    #[test]
    fn parses_a_pack_toml() {
        let pack: Pack = toml::from_str(SAMPLE).expect("sample should parse");

        assert_eq!(pack.name, "Example Pack");
        assert_eq!(pack.author.as_deref(), Some("minecraft_steve"));
        assert_eq!(pack.format.as_deref(), Some("packwiz:1.1.0"));
        assert_eq!(pack.versions.minecraft.as_deref(), Some("1.20.1"));
    }

    /// `minecraft` must not be mistaken for the loader.
    #[test]
    fn finds_the_loader_beside_minecraft() {
        let pack: Pack = toml::from_str(SAMPLE).expect("sample should parse");

        assert_eq!(pack.versions.loader(), Some(("fabric", "0.14.21")));
    }

    #[test]
    fn a_pack_without_versions_is_still_valid() {
        let pack: Pack = toml::from_str(r#"name = "Bare""#).expect("should parse");

        assert_eq!(pack.versions.minecraft, None);
        assert_eq!(pack.versions.loader(), None);
    }
}
