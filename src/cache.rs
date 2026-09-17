use std::{
    collections::{HashMap, HashSet},
    fs::OpenOptions,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    Mod,
    error::{Error, IoContext},
};

pub const CACHE_PATH: &str = ".sculkr.cache.json";

/// Cache file names from the projects sculkr was forked from.
const PREVIOUS_CACHE_FILES: [&str; 4] = [
    ".packwiz-modlist.cache",
    ".packwiz-modlist.cache.json",
    ".packwizml.cache",
    ".packwizml.cache.json",
];

pub type CacheData = HashMap<String, CacheMod>;

/// Bump whenever [`Mod`] gains or changes a field.
///
/// An older file is not corrupt so much as incomplete: every entry in it would
/// stay pinned to whatever the previous build knew how to fetch, because the
/// cache key is the mod's version and that has not changed. Rebuilding costs
/// one round of API calls.
///
/// 2: Modrinth authors, which entries written before then left empty.
/// 3: `snake_case` field names throughout, where [`Mod`] was camelCase.
pub const CACHE_VERSION: u32 = 3;
/// The on-disk shape. Generic over the map so writing can borrow it and
/// reading can own it, without a second struct or a clone of the whole cache.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct CacheFile<ModRef> {
    version: u32,
    mods: ModRef,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CacheMod {
    pub cache_id: String,
    #[serde(flatten)]
    pub data: Mod,
}

#[derive(Debug, Clone)]
pub struct CacheId {
    cache_id: String,
    mod_id: String,
}

impl From<crate::parser::ParsedModrinthId> for CacheId {
    fn from(id: crate::parser::ParsedModrinthId) -> Self {
        Self {
            cache_id: id.cache_id,
            mod_id: id.id,
        }
    }
}

impl From<crate::parser::ParsedCurseForgeId> for CacheId {
    fn from(id: crate::parser::ParsedCurseForgeId) -> Self {
        Self {
            cache_id: id.cache_id,
            mod_id: id.id.to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VersionChange {
    pub id: String,
    pub from: String,
    pub to: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct CacheDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<VersionChange>, // mod version change
    pub unchanged: usize,
}

#[derive(Debug, Clone)]
pub struct Cache {
    file: PathBuf,
    is_dirty: bool,
    data: CacheData,
    diff: CacheDiff,
}

impl Cache {
    pub fn load<T>(file: T) -> Result<Self, Error>
    where
        T: Into<PathBuf>,
    {
        let file = file.into();
        let resolved = crate::util::resolve_for_display(&file);

        log::debug!("loading cache from \"{}\"", resolved.display());

        match OpenOptions::new().read(true).open(&file) {
            Ok(reader) => match serde_json::from_reader::<_, CacheFile<CacheData>>(reader) {
                Ok(cache) if cache.version == CACHE_VERSION => {
                    log::debug!(
                        "loaded {} cached mod(s) from \"{}\"",
                        cache.mods.len(),
                        resolved.display()
                    );
                    Ok(Self {
                        file,
                        is_dirty: false,
                        data: cache.mods,
                        diff: CacheDiff::default(),
                    })
                }
                Ok(cache) => {
                    log::info!(
                        "cache at \"{}\" is version {} but this build writes version {CACHE_VERSION}; rebuilding it",
                        resolved.display(),
                        cache.version
                    );
                    Ok(Self {
                        file,
                        is_dirty: true,
                        data: HashMap::default(),
                        diff: CacheDiff::default(),
                    })
                }
                // A cache written by an older build is missing any field added since.
                // Rebuilding costs one round of API calls; refusing to start costs the
                // whole run, so we treat an unreadable cache as an empty one.
                Err(err) => {
                    log::warn!(
                        "ignoring unreadable cache at \"{}\" ({err}); it will be rebuilt",
                        resolved.display()
                    );
                    Ok(Self {
                        file,
                        // Mark dirty so the stale file is replaced even if nothing changes.
                        is_dirty: true,
                        data: HashMap::default(),
                        diff: CacheDiff::default(),
                    })
                }
            },
            Err(err) => match err.kind() {
                ErrorKind::NotFound => {
                    log::debug!(
                        "no cache at \"{}\", starting with an empty cache",
                        resolved.display()
                    );
                    Ok(Self {
                        file,
                        is_dirty: false,
                        data: HashMap::default(),
                        diff: CacheDiff::default(),
                    })
                }
                _ => Err(Error::FileIo(resolved, err, "open cache file")),
            },
        }
    }

    /// Very similar to [`crate::parser::pack`], except that nothing at `path` is
    /// `None` rather than an empty cache, since [`Self::load`] cannot tell the
    /// caller which of the two it found.
    fn try_read<P>(path: P) -> Result<Option<Self>, Error>
    where
        P: AsRef<Path>,
    {
        let cache_path = crate::util::resolve_for_display(path);

        if !cache_path.is_file() {
            return Ok(None);
        }

        Self::load(cache_path).map(Some)
    }

    /// Every old fork cache file in `dir`.
    ///
    /// Only looked for, never read: reading one through [`Self::load`] would
    /// log that it is being rebuilt, and nothing ever touches these files.
    fn previous_caches(dir: &Path) -> Vec<PathBuf> {
        PREVIOUS_CACHE_FILES
            .iter()
            .map(|name| dir.join(name))
            .filter(|path| path.is_file())
            .collect()
    }

    /// There may be some conditional checks that need to occur before intializing the cache.
    pub fn preflight<P>(dir: P)
    where
        P: AsRef<Path>,
    {
        // Checking for any old fork cache files in the pack_root
        let found = Self::previous_caches(dir.as_ref());

        if found.is_empty() {
            return;
        }

        for path in &found {
            log::warn!(
                "found a packwiz-modlist cache at \"{}\"",
                crate::util::resolve_for_display(path).display()
            );
        }

        // TODO: Implement `sculkr convert` to migrate older/broken caches to .sculkr.cache.json
        log::warn!(
            "sculkr does not read packwiz-modlist caches and keeps its own in {CACHE_PATH}, so the old file can be deleted"
        );
    }

    pub fn set_data(&mut self, data: CacheData) {
        self.data.extend(data);
        self.is_dirty = true;
    }

    pub const fn get_data(&self) -> &CacheData {
        &self.data
    }

    /// Drops cached entries for mods that are no longer in the pack.
    ///
    /// [`Self::set_mod`] overwrites by key, so updated mods replace themselves
    /// cleanly. Returns how many were pruned.
    pub fn retain_only(&mut self, keep: &HashSet<String>) -> usize {
        let before = self.diff.removed.len();

        self.diff.removed.extend(
            self.data
                .extract_if(|mod_id, _| !keep.contains(mod_id))
                .map(|(mod_id, _)| mod_id),
        );

        let removed = self.diff.removed.len().saturating_sub(before);

        if removed > 0 {
            self.is_dirty = true;
        }

        removed
    }

    pub fn set_mod<T>(&mut self, id: T, data: Mod)
    where
        T: Into<CacheId>,
    {
        let id = id.into();

        self.is_dirty = true;
        let prev_mod = self.data.insert(id.mod_id.clone(), CacheMod {
            cache_id: id.cache_id.clone(),
            data,
        });

        match prev_mod {
            // The version of the mod has changed, so lets update the `CacheDiff`.
            Some(pm) if id.cache_id != pm.cache_id => self.diff.changed.push(VersionChange {
                id: id.mod_id,
                from: pm.cache_id,
                to: id.cache_id,
            }),
            // This is the exact same mod, do not update the diff
            Some(_) => (),
            // This is a brand new mod being added to the cache
            None => self.diff.added.push(id.mod_id),
        }
    }

    pub fn get_mod<T>(&self, id: T) -> Option<&Mod>
    where
        T: Into<CacheId>,
    {
        let id = id.into();
        let m = self.data.get(&id.mod_id)?;

        if id.cache_id == m.cache_id {
            Some(&m.data)
        } else {
            None
        }
    }

    pub fn get_diff(self) -> CacheDiff {
        CacheDiff {
            unchanged: 20,
            ..self.diff
        }
    }

    pub fn save(&self) -> Result<(), Error> {
        if self.is_dirty {
            let resolved = crate::util::resolve_for_display(&self.file);

            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&self.file)
                .path_ctx(&resolved, "write cache file")?;

            serde_json::to_writer(file, &CacheFile {
                version: CACHE_VERSION,
                mods: &self.data,
            })?;

            log::debug!(
                "wrote {} cached mod(s) to \"{}\"",
                self.data.len(),
                resolved.display()
            );
        } else {
            log::debug!("cache unchanged, nothing to write");
        }

        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/cache.rs"]
mod tests;
