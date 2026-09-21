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
/// 4: `version_name`, the release an entry is pinned to.
pub const CACHE_VERSION: u32 = 4;
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
    /// The release `cache_id` pins. `None` when the pack did not name one.
    #[serde(default)]
    pub version_name: Option<String>,
    #[serde(flatten)]
    pub data: Mod,
}

#[allow(clippy::struct_field_names)]
#[derive(Debug, Clone)]
pub struct CacheId {
    cache_id: String,
    mod_id: String,
    /// The release `cache_id` pins.
    version_name: Option<String>,
}

impl From<crate::parser::ParsedModrinthId> for CacheId {
    fn from(id: crate::parser::ParsedModrinthId) -> Self {
        Self {
            cache_id: id.cache_id,
            mod_id: id.id,
            version_name: id.version_name,
        }
    }
}

impl From<crate::parser::ParsedCurseForgeId> for CacheId {
    fn from(id: crate::parser::ParsedCurseForgeId) -> Self {
        Self {
            cache_id: id.cache_id,
            mod_id: id.id.to_string(),
            version_name: id.version_name,
        }
    }
}

/// Which service a mod comes from.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Modrinth,
    CurseForge,
}

impl Source {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Modrinth => "modrinth",
            Self::CurseForge => "curseforge",
        }
    }

    /// Reads the source off a cached entry's project url, falling back to the
    /// shape of `mod_id` for a url that names neither service.
    fn infer(mod_id: &str, data: &Mod) -> Self {
        if data.mod_url.contains("modrinth.com") {
            Self::Modrinth
        } else if data.mod_url.contains("curseforge.com") || mod_id.parse::<i32>().is_ok() {
            Self::CurseForge
        } else {
            Self::Modrinth
        }
    }
}

/// A mod as the pack pins it: the cache key, the pack's name for it, and its
/// source.
#[derive(Debug, Clone)]
pub struct PinnedMod {
    pub id: CacheId,
    pub name: String,
    pub source: Source,
}

/// One side of an update, ready to print: the release, or the pinned id when
/// no release is known.
fn version_label(version_name: Option<&str>, cache_id: &str) -> String {
    version_name.unwrap_or(cache_id).to_owned()
}

/// One mod in a diff, carrying enough to print its row without another lookup.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DiffEntry {
    pub id: String,
    pub name: String,
    pub source: Source,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VersionChange {
    pub id: String,
    pub name: String,
    pub source: Source,
    /// Already resolved by [`version_label`].
    pub from: String,
    pub to: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct CacheDiff {
    pub added: Vec<DiffEntry>,
    pub removed: Vec<DiffEntry>,
    pub changed: Vec<VersionChange>, // mod version change
    pub unchanged: usize,
}

impl CacheDiff {
    /// Whether the cache matches the pack.
    pub const fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }

    /// Counts what the pack currently pins, which excludes removals.
    pub const fn total(&self) -> usize {
        self.unchanged
            .saturating_add(self.added.len())
            .saturating_add(self.changed.len())
    }
}

#[derive(Debug, Default, Clone)]
pub struct Cache {
    pub file: PathBuf,
    pub is_dirty: bool,
    pub data: CacheData,
    pub diff: CacheDiff,
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
                .map(|(mod_id, entry)| DiffEntry {
                    name: entry.data.title.clone(),
                    source: Source::infer(&mod_id, &entry.data),
                    id: mod_id,
                }),
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

        // Read before `data` moves into the entry below.
        let name = data.title.clone();
        let source = Source::infer(&id.mod_id, &data);

        self.is_dirty = true;
        let prev_mod = self.data.insert(id.mod_id.clone(), CacheMod {
            cache_id: id.cache_id.clone(),
            version_name: id.version_name.clone(),
            data,
        });

        match prev_mod {
            // The version of the mod has changed, so lets update the `CacheDiff`.
            Some(pm) if id.cache_id != pm.cache_id => self.diff.changed.push(VersionChange {
                id: id.mod_id,
                name,
                source,
                from: version_label(pm.version_name.as_deref(), &pm.cache_id),
                to: version_label(id.version_name.as_deref(), &id.cache_id),
            }),
            // This is the exact same mod, do not update the diff
            Some(_) => (),
            // This is a brand new mod being added to the cache
            None => self.diff.added.push(DiffEntry {
                id: id.mod_id,
                name,
                source,
            }),
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

    /// Compares `pinned` against the cache without fetching anything.
    ///
    /// A pin the cache does not hold is `added`, one it holds under another id
    /// is `changed`, and an entry `pinned` never names is `removed`.
    pub fn diff_against<I>(&self, pinned: I) -> CacheDiff
    where
        I: IntoIterator<Item = PinnedMod>,
    {
        let mut diff = CacheDiff::default();
        let mut seen = HashSet::<String>::with_capacity(self.data.len());

        for pin in pinned {
            let PinnedMod {
                id:
                    CacheId {
                        cache_id,
                        mod_id,
                        version_name,
                    },
                name,
                source,
            } = pin;

            match self.data.get(&mod_id) {
                Some(entry) if entry.cache_id != cache_id => diff.changed.push(VersionChange {
                    id: mod_id.clone(),
                    name,
                    source,
                    from: version_label(entry.version_name.as_deref(), &entry.cache_id),
                    to: version_label(version_name.as_deref(), &cache_id),
                }),
                Some(_) => diff.unchanged = diff.unchanged.saturating_add(1),
                None => diff.added.push(DiffEntry {
                    id: mod_id.clone(),
                    name,
                    source,
                }),
            }

            seen.insert(mod_id);
        }

        // Still cached, no longer pinned. `App::close` prunes these on the
        // next run; the name and source come off the cached entry.
        diff.removed.extend(
            self.data
                .iter()
                .filter(|(mod_id, _)| !seen.contains(*mod_id))
                .map(|(mod_id, entry)| DiffEntry {
                    id: mod_id.clone(),
                    name: entry.data.title.clone(),
                    source: Source::infer(mod_id, &entry.data),
                }),
        );

        diff
    }

    /// The diff accumulated over this run.
    ///
    /// `unchanged` is whatever [`Self::data`] holds that was neither `added`
    /// nor `changed`.
    pub fn get_diff(&self) -> CacheDiff {
        let touched = self
            .diff
            .added
            .len()
            .saturating_add(self.diff.changed.len());

        CacheDiff {
            unchanged: self.data.len().saturating_sub(touched),
            ..self.diff.clone()
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
