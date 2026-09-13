//! Tests for [`Cache`].
//!
//! Grouped by what the cache is asked to do: find an entry, prune what left the
//! pack, read a file, and write one. Anything that touches disk works in its
//! own [`TempDir`].

use std::{
    fs,
    path::{Path, PathBuf},
};

use super::*;
use crate::tests::support::{TempDir, curseforge, modrinth, sample_mod};

/// A clean, empty cache pointed at `file`.
fn empty_at(file: PathBuf) -> Cache {
    Cache {
        file,
        is_dirty: false,
        data: HashMap::default(),
    }
}

/// A clean cache holding three mods across both hosts, as it would look after a
/// fully cached run.
fn populated_at(file: PathBuf) -> Cache {
    let mut cache = empty_at(file);

    cache.set_mod(modrinth("AANobbMI", "v1"), sample_mod("AANobbMI", "Sodium"));
    cache.set_mod(
        modrinth("gvQqBUqZ", "v1"),
        sample_mod("gvQqBUqZ", "Lithium"),
    );
    cache.set_mod(curseforge(238_222, "5101366"), sample_mod("238222", "JEI"));
    cache.is_dirty = false;

    cache
}

/// For tests that never reach [`Cache::save`], so the path is never opened.
fn populated() -> Cache {
    populated_at(PathBuf::from("never-written.json"))
}

/// Writes `mods` in the on-disk shape under any `version`, including ones this
/// build would not write itself.
fn write_cache(path: &Path, version: u32, mods: &CacheData) {
    let json = serde_json::to_string(&CacheFile { version, mods }).expect("serialize fixture");
    fs::write(path, json).expect("write fixture");
}

/// [`Mod`] has no `PartialEq`, so entries are compared through their serialized
/// form, which is also the form the cache file stores. The map keys come back
/// sorted, so `HashMap` iteration order cannot fail a comparison.
fn as_json(data: &CacheData) -> serde_json::Value {
    serde_json::to_value(data).expect("serialize cache data")
}

fn sorted_ids(cache: &Cache) -> Vec<&str> {
    let mut ids: Vec<&str> = cache.data.keys().map(String::as_str).collect();
    ids.sort_unstable();
    ids
}

/// [`Cache::get_mod`] and [`Cache::set_mod`]: which entry a pinned mod resolves
/// to, and when a stored entry is no longer good enough.
mod lookup {
    use super::*;

    #[test]
    fn a_matching_pinned_version_is_a_hit() {
        let cache = populated();

        let hit = cache.get_mod(modrinth("AANobbMI", "v1"));

        assert_eq!(hit.map(|m| m.title.as_str()), Some("Sodium"));
    }

    #[test]
    fn an_unknown_id_is_a_miss() {
        let cache = populated();

        assert!(cache.get_mod(modrinth("P7dR8mSH", "v1")).is_none());
    }

    /// This is what makes a version bump refetch rather than serve stale data.
    #[test]
    fn a_different_pinned_version_is_a_miss() {
        let cache = populated();

        assert!(cache.get_mod(modrinth("AANobbMI", "v2")).is_none());
        assert!(
            cache.data.contains_key("AANobbMI"),
            "a miss must not remove the entry"
        );
    }

    #[test]
    fn a_lookup_leaves_the_cache_clean() {
        let cache = populated();

        let _ = cache.get_mod(modrinth("AANobbMI", "v1"));
        let _ = cache.get_mod(modrinth("AANobbMI", "v2"));

        assert!(!cache.is_dirty);
    }

    #[test]
    fn setting_a_new_version_replaces_the_entry_in_place() {
        let mut cache = populated();

        cache.set_mod(
            modrinth("AANobbMI", "v2"),
            sample_mod("AANobbMI", "Sodium Next"),
        );

        assert!(cache.is_dirty);
        assert_eq!(cache.data.len(), 3, "an update must not add a second entry");
        assert!(cache.get_mod(modrinth("AANobbMI", "v1")).is_none());
        assert_eq!(
            cache
                .get_mod(modrinth("AANobbMI", "v2"))
                .map(|m| m.title.as_str()),
            Some("Sodium Next")
        );
    }

    #[test]
    fn a_curseforge_id_is_keyed_by_its_decimal_string() {
        let cache = populated();

        assert!(cache.data.contains_key("238222"));
        assert_eq!(
            cache
                .get_mod(curseforge(238_222, "5101366"))
                .map(|m| m.title.as_str()),
            Some("JEI")
        );
    }
}

/// [`Cache::retain_only`]: dropping entries for mods that have left the pack.
mod prune {
    use super::*;

    fn keep(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|&id| id.to_owned()).collect()
    }

    #[test]
    fn entries_no_longer_in_the_pack_are_dropped_and_counted() {
        let mut cache = populated();

        let removed = cache.retain_only(&keep(&["AANobbMI", "238222"]));

        assert_eq!(removed, 1);
        assert_eq!(sorted_ids(&cache), ["238222", "AANobbMI"]);
        assert!(cache.is_dirty);
    }

    #[test]
    fn nothing_to_drop_leaves_the_cache_clean() {
        let mut cache = populated();

        // A pack mod that was never cached is not a reason to rewrite the file.
        let removed = cache.retain_only(&keep(&["AANobbMI", "gvQqBUqZ", "238222", "P7dR8mSH"]));

        assert_eq!(removed, 0);
        assert_eq!(cache.data.len(), 3);
        assert!(!cache.is_dirty);
    }

    #[test]
    fn an_empty_pack_drops_everything() {
        let mut cache = populated();

        let removed = cache.retain_only(&HashSet::new());

        assert_eq!(removed, 3);
        assert!(cache.data.is_empty());
        assert!(cache.is_dirty);
    }
}

/// [`Cache::load`]: every file it can find, and which of them it trusts.
///
/// Anything short of a current, well-formed file loads as an empty cache. The
/// dirty flag is what separates "nothing there" from "something there that has
/// to be replaced".
mod load {
    use super::*;

    #[test]
    fn an_absent_file_is_empty_and_clean() {
        let dir = TempDir::new("cache-load-absent");

        let cache = Cache::load(dir.cache_file()).expect("an absent file is not an error");

        assert!(cache.data.is_empty());
        assert!(!cache.is_dirty, "there is nothing on disk to replace");
    }

    #[test]
    fn a_current_file_is_loaded_and_clean() {
        let dir = TempDir::new("cache-load-current");
        let written = populated();
        write_cache(&dir.cache_file(), CACHE_VERSION, &written.data);

        let cache = Cache::load(dir.cache_file()).expect("load a current file");

        assert_eq!(as_json(&cache.data), as_json(&written.data));
        assert!(!cache.is_dirty);
    }

    #[test]
    fn an_older_version_is_discarded_and_marked_dirty() {
        let dir = TempDir::new("cache-load-older-version");
        let older = CACHE_VERSION
            .checked_sub(1)
            .expect("a version before this one");
        write_cache(&dir.cache_file(), older, &populated().data);

        let cache = Cache::load(dir.cache_file()).expect("an old version is not an error");

        assert!(cache.data.is_empty());
        assert!(cache.is_dirty);
    }

    #[test]
    fn unparseable_json_is_discarded_and_marked_dirty() {
        let dir = TempDir::new("cache-load-unparseable");
        fs::write(dir.cache_file(), "{ not json").expect("write fixture");

        let cache = Cache::load(dir.cache_file()).expect("a corrupt file is not an error");

        assert!(cache.data.is_empty());
        assert!(cache.is_dirty);
    }

    /// The case the `CACHE_VERSION` docs describe: a file from a build that had
    /// not yet added a field [`Mod`] now requires.
    #[test]
    fn an_entry_missing_a_field_is_discarded_and_marked_dirty() {
        let dir = TempDir::new("cache-load-missing-field");
        let file = format!(
            r#"{{"version":{CACHE_VERSION},"mods":{{"AANobbMI":{{"cache_id":"v1","id":"AANobbMI"}}}}}}"#
        );
        fs::write(dir.cache_file(), file).expect("write fixture");

        let cache = Cache::load(dir.cache_file()).expect("an incomplete file is not an error");

        assert!(cache.data.is_empty());
        assert!(cache.is_dirty);
    }

    /// Only a missing file is forgiven at open time. Windows reports this path
    /// as not found rather than not a directory, so the case is Unix-only.
    #[cfg(unix)]
    #[test]
    fn a_path_that_cannot_be_opened_is_an_error() {
        let dir = TempDir::new("cache-load-unopenable");
        let not_a_dir = dir.path().join("file");
        fs::write(&not_a_dir, "").expect("write fixture");

        let result = Cache::load(not_a_dir.join("cache.json"));

        assert!(matches!(result, Err(Error::FileIo(..))), "got {result:?}");
    }
}

/// [`Cache::save`]: when the file is written, and what ends up in it.
mod save {
    use super::*;

    #[test]
    fn a_clean_cache_leaves_an_existing_file_untouched() {
        let dir = TempDir::new("cache-save-clean-existing");
        write_cache(&dir.cache_file(), CACHE_VERSION, &populated().data);
        let cache = Cache::load(dir.cache_file()).expect("load a current file");

        // Swap the file out from under the loaded cache, so a write would show.
        fs::write(dir.cache_file(), "sentinel").expect("overwrite fixture");
        cache.save().expect("save a clean cache");

        assert_eq!(
            fs::read_to_string(dir.cache_file()).expect("read back"),
            "sentinel"
        );
    }

    #[test]
    fn a_clean_cache_does_not_create_a_file() {
        let dir = TempDir::new("cache-save-clean-absent");
        let cache = Cache::load(dir.cache_file()).expect("an absent file is not an error");

        cache.save().expect("save a clean cache");

        assert!(!dir.cache_file().exists());
    }

    #[test]
    fn a_dirty_cache_round_trips_through_load() {
        let dir = TempDir::new("cache-save-round-trip");
        let mut cache = populated_at(dir.cache_file());
        cache.is_dirty = true;

        cache.save().expect("save a dirty cache");
        let reloaded = Cache::load(dir.cache_file()).expect("load what was saved");

        assert_eq!(as_json(&reloaded.data), as_json(&cache.data));
        // Clean on reload means the version check passed, so `save` stamped the
        // version this build reads.
        assert!(!reloaded.is_dirty);
    }

    /// The reason [`Cache::load`] marks a rejected file dirty.
    #[test]
    fn a_rejected_file_is_replaced_even_with_nothing_to_add() {
        let dir = TempDir::new("cache-save-replaces-rejected");
        fs::write(dir.cache_file(), "{ not json").expect("write fixture");

        Cache::load(dir.cache_file())
            .expect("a corrupt file is not an error")
            .save()
            .expect("save over the corrupt file");
        let reloaded = Cache::load(dir.cache_file()).expect("load what was saved");

        assert!(reloaded.data.is_empty());
        assert!(!reloaded.is_dirty, "the corrupt file should be gone");
    }

    #[test]
    fn a_prune_is_persisted() {
        let dir = TempDir::new("cache-save-prune");
        write_cache(&dir.cache_file(), CACHE_VERSION, &populated().data);
        let mut cache = Cache::load(dir.cache_file()).expect("load a current file");

        cache.retain_only(&HashSet::from(["AANobbMI".to_owned()]));
        cache.save().expect("save the pruned cache");
        let reloaded = Cache::load(dir.cache_file()).expect("load what was saved");

        assert_eq!(sorted_ids(&reloaded), ["AANobbMI"]);
    }

    /// Changing this snapshot means changing the file format, which means
    /// bumping [`CACHE_VERSION`] in the same change.
    #[test]
    fn the_on_disk_shape_is_pinned() {
        let dir = TempDir::new("cache-save-shape");
        let mut cache = empty_at(dir.cache_file());
        cache.set_mod(modrinth("AANobbMI", "v1"), sample_mod("AANobbMI", "Sodium"));

        cache.save().expect("save a dirty cache");

        // Reparsed only to pretty-print; keys come back sorted, which keeps the
        // snapshot stable without depending on field or map order.
        let written: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.cache_file()).expect("read back"))
                .expect("saved file is JSON");
        let pretty = serde_json::to_string_pretty(&written).expect("pretty-print");

        insta::assert_snapshot!(pretty);
    }
}

/// [`Cache::try_read`] and [`Cache::previous_caches`]: finding cache files left
/// behind by the projects sculkr was forked from, before the real one loads.
mod preflight {
    use super::*;

    fn files(found: &[Result<Cache, Error>]) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = found
            .iter()
            .map(|result| result.as_ref().expect("a readable cache").file.clone())
            .collect();
        files.sort_unstable();
        files
    }

    #[test]
    fn nothing_at_the_path_is_none() {
        let dir = TempDir::new("cache-try-read-absent");

        let read = Cache::try_read(dir.cache_file()).expect("an absent file is not an error");

        assert!(read.is_none());
    }

    /// A directory with a cache file's name is not a cache file.
    #[test]
    fn a_directory_at_the_path_is_none() {
        let dir = TempDir::new("cache-try-read-directory");
        fs::create_dir(dir.cache_file()).expect("create fixture directory");

        let read = Cache::try_read(dir.cache_file()).expect("a directory is not an error");

        assert!(read.is_none());
    }

    #[test]
    fn a_current_file_is_read() {
        let dir = TempDir::new("cache-try-read-current");
        write_cache(&dir.cache_file(), CACHE_VERSION, &populated().data);

        let cache = Cache::try_read(dir.cache_file())
            .expect("read a current file")
            .expect("the file exists");

        assert_eq!(cache.data.len(), 3);
        assert!(!cache.is_dirty);
    }

    /// An old fork's file is unlikely to parse, and still has to be reported
    /// as found.
    #[test]
    fn a_file_in_another_format_is_still_found() {
        let dir = TempDir::new("cache-try-read-foreign");
        fs::write(dir.cache_file(), r#"{"AANobbMI":{"cacheId":"v1"}}"#).expect("write fixture");

        let cache = Cache::try_read(dir.cache_file())
            .expect("an unreadable file is not an error")
            .expect("the file exists");

        assert!(cache.data.is_empty());
        assert!(cache.is_dirty);
    }

    #[test]
    fn the_path_is_normalized_for_reporting() {
        let dir = TempDir::new("cache-try-read-normalized");
        write_cache(&dir.cache_file(), CACHE_VERSION, &CacheData::new());
        let roundabout = dir
            .path()
            .join(".")
            .join("sub")
            .join("..")
            .join("cache.json");
        fs::create_dir(dir.join("sub")).expect("create fixture directory");

        let cache = Cache::try_read(roundabout)
            .expect("read a current file")
            .expect("the file exists");

        assert_eq!(cache.file, dir.cache_file());
    }

    /// Guards against warning about old caches on every run of a pack that has
    /// none.
    #[test]
    fn a_directory_without_old_caches_finds_nothing() {
        let dir = TempDir::new("cache-previous-none");

        assert!(Cache::previous_caches(dir.path()).is_empty());
    }

    #[test]
    fn every_old_cache_name_is_found() {
        let dir = TempDir::new("cache-previous-all");
        for name in PREVIOUS_CACHE_FILES {
            fs::write(dir.join(name), "{}").expect("write fixture");
        }

        let found = Cache::previous_caches(dir.path());

        let mut expected: Vec<PathBuf> = PREVIOUS_CACHE_FILES
            .iter()
            .map(|name| dir.join(name))
            .collect();
        expected.sort_unstable();
        assert_eq!(files(&found), expected);
    }

    #[test]
    fn only_the_old_caches_present_are_found() {
        let dir = TempDir::new("cache-previous-some");
        fs::write(dir.join(".packwizml.cache.json"), "{}").expect("write fixture");

        let found = Cache::previous_caches(dir.path());

        assert_eq!(files(&found), [dir.join(".packwizml.cache.json")]);
    }

    #[test]
    fn the_current_cache_is_not_an_old_one() {
        let dir = TempDir::new("cache-previous-current");
        write_cache(&dir.join(CACHE_PATH), CACHE_VERSION, &populated().data);

        assert!(Cache::previous_caches(dir.path()).is_empty());
    }
}
