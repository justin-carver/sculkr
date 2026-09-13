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
const CACHE_VERSION: u32 = 2;

/// The on-disk shape. Generic over the map so writing can borrow it and
/// reading can own it, without a second struct or a clone of the whole cache.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct CacheFile<M> {
    version: u32,
    mods: M,
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

#[derive(Debug, Clone)]
pub struct Cache {
    file: PathBuf,
    is_dirty: bool,
    data: CacheData,
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

    /// Every old fork cache file in `dir`, read or failed.
    fn previous_caches(dir: &Path) -> Vec<Result<Self, Error>> {
        PREVIOUS_CACHE_FILES
            .iter()
            .filter_map(|name| Self::try_read(dir.join(name)).transpose())
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

        for result in &found {
            match result {
                // TODO: This should probably state this to the user in a better way
                Ok(cache) => log::warn!(
                    "older cache file detected: {}",
                    cache.file.as_path().display()
                ),
                Err(e) => log::error!("{}", format_args!("{e:#?}")),
            }
        }

        // TODO: Implement `sculkr convert` to migrate older/broken caches to .sculkr.cache.json
        log::warn!(
            "if any of these should be converted, please run `sculkr convert <CACHE_PATH>`."
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
        let before = self.data.len();

        self.data.retain(|mod_id, _| keep.contains(mod_id));

        let removed = before.saturating_sub(self.data.len());

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
        self.data.insert(id.mod_id, CacheMod {
            cache_id: id.cache_id,
            data,
        });
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
