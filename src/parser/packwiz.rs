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
    pub fn load_from<P>(pack_root: P, index_file: Option<&str>) -> Result<Self, Error>
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

        Ok(Self::from_records(parsed_mods))
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

    fn from_records(parsed_mods: Vec<PackwizMod>) -> Self {
        let modrinth_mods = parsed_mods
            .iter()
            .filter_map(|m| m.update.modrinth.as_ref())
            .map(|data| ParsedModrinthId {
                cache_id: data.version.clone(),
                id: data.mod_id.clone(),
            })
            .collect();

        let curseforge_mods = parsed_mods
            .iter()
            .filter_map(|m| m.update.curseforge.as_ref())
            .map(|data| ParsedCurseForgeId {
                cache_id: data.file_id.to_string(),
                id: data.project_id,
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
