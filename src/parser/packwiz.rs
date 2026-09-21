use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    Error,
    error::IoContext,
    parser::{
        ParsedCurseForgeId, ParsedModrinthId, Parser,
        index::{INDEX_FILE_NAME, Index},
    },
};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all(deserialize = "kebab-case", serialize = "snake_case"))]
pub struct PackwizModUpdate {
    pub modrinth: Option<PackwizModUpdateModrinth>,
    pub curseforge: Option<PackwizModUpdateCurseforge>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all(deserialize = "kebab-case", serialize = "snake_case"))]
pub struct PackwizModUpdateModrinth {
    pub mod_id: String,
    pub version: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all(deserialize = "kebab-case", serialize = "snake_case"))]
pub struct PackwizModUpdateCurseforge {
    pub file_id: u32,
    pub project_id: i32,
}

/// Where packwiz fetches the jar from.
///
/// `CurseForge` entries carry `mode = "metadata:curseforge"` and no `url`, since
/// the file is resolved through the API rather than downloaded directly.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all(deserialize = "kebab-case", serialize = "snake_case"))]
pub struct PackwizDownload {
    pub url: Option<String>,
    pub hash: String,
    pub hash_format: String,
    pub mode: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all(deserialize = "kebab-case", serialize = "snake_case"))]
pub struct PackwizMod {
    pub name: String,
    pub filename: String,
    /// `client`, `server` or `both`.
    pub side: Option<String>,
    pub download: PackwizDownload,
    pub update: PackwizModUpdate,

    /// The folder this came from, relative to the pack root: `mods`,
    /// `resourcepacks`, `shaderpacks`, `datapacks`, whatever the pack uses.
    /// Filled in from the index after parsing, never read from the file.
    #[serde(skip_deserializing)]
    pub category: Option<String>,

    /// The record's own path, relative to the pack root.
    #[serde(skip_deserializing)]
    pub path: PathBuf,
}

/// Matches `1.21.1` or `v0.6.5`, returning it without the `v`.
///
/// Requires at least one dot.
fn version_token(token: &str) -> Option<&str> {
    let token = token.strip_prefix('v').unwrap_or(token);
    let mut parts = token.split('.');

    let numeric = parts
        .clone()
        .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));

    (numeric && parts.nth(1).is_some()).then_some(token)
}

/// Reads a mod's release out of the jar filename packwiz records.
///
/// Takes the last version-shaped token, skipping `minecraft` where the pack
/// names it: `jei-1.21.1-neoforge-19.21.0.247.jar` is `19.21.0.247`.
///
/// `None` when nothing in the name looks like a version.
pub fn release_version(filename: &str, minecraft: Option<&str>) -> Option<String> {
    let stem = filename.strip_suffix(".jar").unwrap_or(filename);

    let candidates: Vec<&str> = stem
        .split(['-', '+', '_'])
        .filter_map(version_token)
        .collect();

    let filtered: Vec<&str> = candidates
        .iter()
        .copied()
        .filter(|token| Some(*token) != minecraft)
        .collect();

    // Keep the Minecraft version where it is the only candidate.
    filtered
        .last()
        .or_else(|| candidates.last())
        .map(|token| (*token).to_owned())
}

#[derive(Debug, Clone)]
pub struct PackwizParser {
    pub modrinth_mods: Vec<ParsedModrinthId>,
    pub curseforge_mods: Vec<ParsedCurseForgeId>,
    /// Every `*.pw.toml` as it was written, kept for the export.
    pub mods: Vec<PackwizMod>,
}

impl PackwizParser {
    /// Reads every metafile the pack's index lists.
    ///
    /// `pack_root` is the directory holding `pack.toml`, not a content folder.
    /// Going through `index.toml` is what makes this folder-agnostic: mods,
    /// resourcepacks, shaderpacks and datapacks all arrive together, and a
    /// folder packwiz adds later needs no change here.
    ///
    /// A pack with no index falls back to scanning `pack_root` itself, which
    /// keeps a bare directory of `*.pw.toml` files working.
    ///
    /// `minecraft` is the pack's Minecraft version, skipped when reading a
    /// release off a filename.
    pub fn load_from<P>(
        pack_root: P,
        index_file: Option<&str>,
        minecraft: Option<&str>,
    ) -> Result<Self, Error>
    where
        P: AsRef<Path>,
    {
        let pack_root = pack_root.as_ref();
        let index_name = index_file.unwrap_or(INDEX_FILE_NAME);
        let index_path = pack_root.join(index_name);

        let parsed_mods = if index_path.is_file() {
            Self::from_index(pack_root, &index_path)?
        } else {
            log::warn!(
                "no {index_name} in \"{}\"; scanning that directory for *.pw.toml instead",
                crate::util::resolve_for_display(pack_root).display()
            );

            Self::from_directory(pack_root)?
        };

        Ok(Self::from_records(parsed_mods, minecraft))
    }

    fn from_index(pack_root: &Path, index_path: &Path) -> Result<Vec<PackwizMod>, Error> {
        let index = Index::read(index_path)?;

        log::debug!(
            "index lists {} file(s) across {:?}",
            index.files.len(),
            index.categories()
        );

        let mut parsed_mods = Vec::new();

        for entry in index.metafiles() {
            let path = pack_root.join(&entry.file);

            // An index that has drifted from disk should name the missing file
            // rather than failing the run with a bare io error.
            if !path.is_file() {
                log::warn!(
                    "{index_name} lists \"{}\" but it is not on disk; skipping",
                    entry.file.display(),
                    index_name = INDEX_FILE_NAME
                );

                continue;
            }

            let data = std::fs::read_to_string(&path).path_ctx(&path, "read file")?;
            let mut parsed = toml::from_str::<PackwizMod>(&data)
                .map_err(|err| Error::TomlFile(path.clone(), err))?;

            parsed.category = entry.category().map(str::to_owned);
            // parsed.path = entry.file.clone();
            parsed.path.clone_from(&entry.file);

            log::trace!("parsed \"{}\" as \"{}\"", entry.file.display(), parsed.name);
            parsed_mods.push(parsed);
        }

        log::info!(
            "found {} mod(s) across {} folder(s)",
            parsed_mods.len(),
            index.categories().len()
        );

        Ok(parsed_mods)
    }

    fn from_directory(directory: &Path) -> Result<Vec<PackwizMod>, Error> {
        let resolved = crate::util::resolve_for_display(directory);

        log::debug!("scanning for *.pw.toml in \"{}\"", resolved.display());

        let entries = directory.read_dir().path_ctx(&resolved, "read directory")?;
        let mut parsed_mods = Vec::new();
        let mut skipped = 0usize;

        for entry in entries {
            let entry = entry.path_ctx(&resolved, "read directory entry")?;
            let path = entry.path();

            if !entry.file_name().to_string_lossy().ends_with(".pw.toml") {
                skipped.saturating_add(1);
                log::trace!("skipping non-pw.toml entry \"{}\"", path.display());
                continue;
            }

            let data = std::fs::read_to_string(&path).path_ctx(&path, "read file")?;
            let mut parsed = toml::from_str::<PackwizMod>(&data)
                .map_err(|err| Error::TomlFile(path.clone(), err))?;

            parsed.path = entry.file_name().into();

            log::debug!("parsed \"{}\" as \"{}\"", path.display(), parsed.name);
            parsed_mods.push(parsed);
        }

        log::info!(
            "found {} mod(s) in \"{}\" ({} entries skipped)",
            parsed_mods.len(),
            resolved.display(),
            skipped
        );

        if parsed_mods.is_empty() {
            log::warn!("no *.pw.toml files found in \"{}\"", resolved.display());
        }

        Ok(parsed_mods)
    }

    fn from_records(parsed_mods: Vec<PackwizMod>, minecraft: Option<&str>) -> Self {
        let modrinth_mods = parsed_mods
            .iter()
            .filter_map(|m| m.update.modrinth.as_ref().map(|data| (m, data)))
            .map(|(record, data)| ParsedModrinthId {
                cache_id: data.version.clone(),
                id: data.mod_id.clone(),
                version_name: release_version(&record.filename, minecraft),
            })
            .collect();

        let curseforge_mods = parsed_mods
            .iter()
            .filter_map(|m| m.update.curseforge.as_ref().map(|data| (m, data)))
            .map(|(record, data)| ParsedCurseForgeId {
                cache_id: data.file_id.to_string(),
                id: data.project_id,
                version_name: release_version(&record.filename, minecraft),
            })
            .collect();

        Self {
            modrinth_mods,
            curseforge_mods,
            mods: parsed_mods,
        }
    }
}

impl Parser for PackwizParser {
    fn get_mods_owned(self) -> (Vec<ParsedModrinthId>, Vec<ParsedCurseForgeId>) {
        (self.modrinth_mods, self.curseforge_mods)
    }

    fn get_modrinth_mods(&self) -> Vec<ParsedModrinthId> {
        self.modrinth_mods.clone()
    }

    fn get_curseforge_mods(&self) -> Vec<ParsedCurseForgeId> {
        self.curseforge_mods.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`release_version`]: reading a release off a jar filename.
    mod release {
        use super::*;

        const MC: Option<&str> = Some("1.21.1");

        /// Real packwiz filenames across both hosts and every loader.
        #[test]
        fn the_mods_own_release_wins_over_the_minecraft_version() {
            for (filename, want) in [
                ("sodium-fabric-0.6.5+mc1.21.1.jar", "0.6.5"),
                ("lithium-fabric-0.14.3+mc1.21.1.jar", "0.14.3"),
                ("jei-1.21.1-neoforge-19.21.0.247.jar", "19.21.0.247"),
                ("Create-1.21.1-6.0.4.jar", "6.0.4"),
                ("appliedenergistics2-19.2.5.jar", "19.2.5"),
                ("ftb-teams-neoforge-2101.1.5.jar", "2101.1.5"),
                ("iris-fabric-1.8.8+mc1.21.1.jar", "1.8.8"),
                (
                    "DistantHorizons-2.3.0-b-1.21.1-neoforge-fabric.jar",
                    "2.3.0",
                ),
                ("journeymap-neoforge-1.21.1-6.0.0-beta.29.jar", "6.0.0"),
                ("modmenu-11.0.3.jar", "11.0.3"),
                ("cloth-config-15.0.140-neoforge.jar", "15.0.140"),
                ("architectury-13.0.8-neoforge.jar", "13.0.8"),
                ("Xaeros_Minimap_25.2.0_NeoForge_1.21.1.jar", "25.2.0"),
                ("voicechat-neoforge-1.21.1-2.5.29.jar", "2.5.29"),
            ] {
                assert_eq!(
                    release_version(filename, MC).as_deref(),
                    Some(want),
                    "parsing {filename}"
                );
            }
        }

        /// The Minecraft version is kept when nothing else is left.
        #[test]
        fn the_minecraft_version_is_kept_when_it_is_all_there_is() {
            assert_eq!(
                release_version("somemod-1.21.1.jar", MC).as_deref(),
                Some("1.21.1")
            );
        }

        /// What `None` costs: a filename carrying both versions can only
        /// guess at which is the mod's.
        #[test]
        fn an_unknown_minecraft_version_falls_back_to_the_last_token() {
            assert_eq!(
                release_version("jei-1.21.1-neoforge-19.21.0.247.jar", None).as_deref(),
                Some("19.21.0.247")
            );
            assert_eq!(
                release_version("DistantHorizons-2.3.0-b-1.21.1-neoforge.jar", None).as_deref(),
                Some("1.21.1"),
                "the mc version wins here, which is why the pack's is passed in"
            );
        }

        #[test]
        fn a_name_with_no_version_in_it_is_none() {
            assert_eq!(release_version("ftb-teams-neoforge.jar", MC), None);
            assert_eq!(release_version("somemod.jar", MC), None);
            // A trailing digit is not a version; it needs a dot.
            assert_eq!(release_version("appliedenergistics2.jar", MC), None);
        }

        #[test]
        fn a_leading_v_is_dropped() {
            assert_eq!(
                release_version("somemod-v1.2.3.jar", MC).as_deref(),
                Some("1.2.3")
            );
        }
    }
}
