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
use crate::tests::support::{
    TempDir, curseforge, curseforge_release, modrinth, modrinth_release, sample_mod,
};

/// A clean, empty cache pointed at `file`.
fn empty_at(file: PathBuf) -> Cache {
    Cache {
        file,
        is_dirty: false,
        data: HashMap::default(),
        diff: CacheDiff::default(),
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
    // A CurseForge entry carries a CurseForge url, which is what names the
    // source of an entry the pack no longer pins.
    let mut jei = sample_mod("238222", "JEI");
    jei.mod_url = "https://www.curseforge.com/minecraft/mc-mods/jei".to_owned();
    cache.set_mod(curseforge(238_222, "5101366"), jei);
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

/// [`Cache::diff_against`]: what the next run would do, compared against the
/// pack without fetching anything.
mod diff_against {
    use super::*;

    const EMPTY_IDS: [&str; 0] = [];

    /// The pack's pins as [`Cache::diff_against`] takes them, each named
    /// after its own id.
    fn pinned(mr: &[(&str, &str)], cf: &[(i32, &str)]) -> Vec<PinnedMod> {
        mr.iter()
            .map(|(id, version)| PinnedMod {
                id: CacheId::from(modrinth(id, version)),
                name: format!("{id}-name"),
                source: Source::Modrinth,
            })
            .chain(cf.iter().map(|(id, file)| PinnedMod {
                id: CacheId::from(curseforge(*id, file)),
                name: format!("{id}-name"),
                source: Source::CurseForge,
            }))
            .collect()
    }

    /// The pins that match what [`populated`] already holds.
    fn pinned_as_cached() -> Vec<PinnedMod> {
        pinned(&[("AANobbMI", "v1"), ("gvQqBUqZ", "v1")], &[(
            238_222, "5101366",
        )])
    }

    /// Diff lists come out in `HashMap` order, so sort before comparing.
    fn ids(entries: &[DiffEntry]) -> Vec<&str> {
        let mut ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
        ids.sort_unstable();
        ids
    }

    /// A pack that matches its cache reports every mod as unchanged and
    /// nothing else.
    #[test]
    fn a_matching_pack_is_entirely_unchanged() {
        let cache = populated();

        let diff = cache.diff_against(pinned_as_cached());

        assert_eq!(diff.unchanged, 3);
        assert_eq!(ids(&diff.added), EMPTY_IDS);
        assert_eq!(ids(&diff.removed), EMPTY_IDS);
        assert!(diff.changed.is_empty());
        assert!(diff.is_empty(), "nothing to do means an empty diff");
    }

    #[test]
    fn an_empty_cache_makes_every_pin_an_addition() {
        let cache = empty_at(PathBuf::from("never-written.json"));

        let diff = cache.diff_against(pinned(&[("AANobbMI", "v1")], &[(238_222, "5101366")]));

        assert_eq!(diff.unchanged, 0);
        assert_eq!(ids(&diff.added), ["238222", "AANobbMI"]);
        assert_eq!(ids(&diff.removed), EMPTY_IDS);
    }

    /// A pin that moved since the cache was written is reported while the
    /// cache still holds the old id.
    #[test]
    fn a_moved_pin_is_reported_as_changed() {
        let cache = populated();

        // Only Sodium moves; the other two stay where the cache has them.
        let moved = pinned(&[("AANobbMI", "v2"), ("gvQqBUqZ", "v1")], &[(
            238_222, "5101366",
        )]);
        let diff = cache.diff_against(moved);

        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].id, "AANobbMI");
        assert_eq!(diff.changed[0].from, "v1");
        assert_eq!(diff.changed[0].to, "v2");
        // Counted apart from the mods that did not move.
        assert_eq!(diff.unchanged, 2);
        assert_eq!(ids(&diff.added), EMPTY_IDS);
    }

    /// Both sides of an update print as releases: the old one off the cache,
    /// the new one off the pack.
    #[test]
    fn an_update_prints_releases_on_both_sides() {
        let mut cache = empty_at(PathBuf::from("never-written.json"));
        cache.set_mod(
            modrinth_release("AANobbMI", "v1", "0.6.0"),
            sample_mod("AANobbMI", "Sodium"),
        );

        let diff = cache.diff_against(vec![PinnedMod {
            id: CacheId::from(modrinth_release("AANobbMI", "v2", "0.6.5")),
            name: "Sodium".to_owned(),
            source: Source::Modrinth,
        }]);

        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].from, "0.6.0");
        assert_eq!(diff.changed[0].to, "0.6.5");
    }

    /// An entry with no stored release falls back to its pinned id.
    #[test]
    fn an_unknown_release_falls_back_to_the_pinned_id() {
        let mut cache = empty_at(PathBuf::from("never-written.json"));
        // `modrinth` leaves the release unset, as a pre-v4 entry would.
        cache.set_mod(modrinth("AANobbMI", "v1"), sample_mod("AANobbMI", "Sodium"));

        let diff = cache.diff_against(vec![PinnedMod {
            id: CacheId::from(modrinth_release("AANobbMI", "v2", "0.6.5")),
            name: "Sodium".to_owned(),
            source: Source::Modrinth,
        }]);

        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].from, "v1", "the pin id stands in");
        assert_eq!(
            diff.changed[0].to, "0.6.5",
            "the new side is still a release"
        );
    }

    /// A `CurseForge` pin resolves the same way, off its file id.
    #[test]
    fn a_curseforge_update_prints_releases_too() {
        let mut cache = empty_at(PathBuf::from("never-written.json"));
        cache.set_mod(
            curseforge_release(238_222, "5101366", "19.21.0"),
            sample_mod("238222", "JEI"),
        );

        let diff = cache.diff_against(vec![PinnedMod {
            id: CacheId::from(curseforge_release(238_222, "5209876", "19.21.1")),
            name: "JEI".to_owned(),
            source: Source::CurseForge,
        }]);

        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].from, "19.21.0");
        assert_eq!(diff.changed[0].to, "19.21.1");
    }

    /// Cached but no longer pinned: reported before `App::close` prunes it.
    #[test]
    fn a_dropped_mod_is_reported_as_removed() {
        let cache = populated();

        let diff = cache.diff_against(pinned(&[("AANobbMI", "v1")], &[]));

        assert_eq!(ids(&diff.removed), ["238222", "gvQqBUqZ"]);
        assert_eq!(diff.unchanged, 1);
        assert!(diff.changed.is_empty());
    }

    /// A removal is not counted as unchanged just because the cache holds it.
    #[test]
    fn removals_do_not_inflate_the_unchanged_count() {
        let cache = populated();

        let diff = cache.diff_against(pinned(&[("P7dR8mSH", "v1")], &[]));

        assert_eq!(diff.unchanged, 0);
        assert_eq!(ids(&diff.added), ["P7dR8mSH"]);
        assert_eq!(ids(&diff.removed), ["238222", "AANobbMI", "gvQqBUqZ"]);
        // Only what the pack still pins.
        assert_eq!(diff.total(), 1);
    }

    /// Comparing never writes.
    #[test]
    fn comparing_leaves_the_cache_clean() {
        let mut cache = populated();
        cache.is_dirty = false;

        let _ = cache.diff_against(pinned(&[("AANobbMI", "v2")], &[]));

        assert!(!cache.is_dirty, "comparing should not dirty the cache");
        assert_eq!(sorted_ids(&cache).len(), 3, "nothing should be dropped");
    }

    /// A pinned mod is named and sourced by the pack.
    #[test]
    fn a_pinned_mod_is_named_and_sourced_by_the_pack() {
        let cache = empty_at(PathBuf::from("never-written.json"));

        let diff = cache.diff_against(pinned(&[("AANobbMI", "v1")], &[(238_222, "5101366")]));

        let modrinth_entry = diff
            .added
            .iter()
            .find(|entry| entry.id == "AANobbMI")
            .expect("the Modrinth pin is added");
        assert_eq!(modrinth_entry.name, "AANobbMI-name");
        assert_eq!(modrinth_entry.source, Source::Modrinth);

        let curseforge_entry = diff
            .added
            .iter()
            .find(|entry| entry.id == "238222")
            .expect("the CurseForge pin is added");
        assert_eq!(curseforge_entry.name, "238222-name");
        assert_eq!(curseforge_entry.source, Source::CurseForge);
    }

    /// A removal is named and sourced by the cache.
    #[test]
    fn a_removed_mod_is_named_and_sourced_by_the_cache() {
        let cache = populated();

        let diff = cache.diff_against(pinned(&[("AANobbMI", "v1")], &[]));

        let lithium = diff
            .removed
            .iter()
            .find(|entry| entry.id == "gvQqBUqZ")
            .expect("Lithium is removed");
        assert_eq!(lithium.name, "Lithium", "the cached title, not the pack's");
        assert_eq!(lithium.source, Source::Modrinth);

        // Read off the CurseForge url the cached entry carries.
        let jei = diff
            .removed
            .iter()
            .find(|entry| entry.id == "238222")
            .expect("JEI is removed");
        assert_eq!(jei.name, "JEI");
        assert_eq!(jei.source, Source::CurseForge);
    }
}

/// [`Cache::get_diff`]: what a run did, reported once it is over.
mod get_diff {
    use super::*;

    fn keep(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|&id| id.to_owned()).collect()
    }

    fn ids(entries: &[DiffEntry]) -> Vec<&str> {
        let mut ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
        ids.sort_unstable();
        ids
    }

    /// [`populated`] fills itself through `set_mod`, which counts every entry
    /// as fetched. Clearing the diff leaves what a load from disk produces.
    fn as_loaded(mut cache: Cache) -> Cache {
        cache.diff = CacheDiff::default();
        cache
    }

    /// A run that fetched nothing reports every entry as unchanged.
    #[test]
    fn a_fully_cached_run_is_entirely_unchanged() {
        let diff = as_loaded(populated()).get_diff();

        assert_eq!(diff.unchanged, 3);
        assert!(diff.is_empty());
    }

    /// A run against an empty cache fetched all of it, so none of it is a hit.
    #[test]
    fn a_first_run_reports_everything_as_added() {
        let diff = populated().get_diff();

        assert_eq!(ids(&diff.added), ["238222", "AANobbMI", "gvQqBUqZ"]);
        assert_eq!(diff.unchanged, 0);
    }

    /// Fetches and prunes are counted apart from the entries left alone.
    #[test]
    fn fetches_and_prunes_are_counted_apart_from_hits() {
        let mut cache = as_loaded(populated());

        cache.set_mod(
            modrinth("P7dR8mSH", "v1"),
            sample_mod("P7dR8mSH", "Fabric API"),
        );
        cache.set_mod(modrinth("AANobbMI", "v2"), sample_mod("AANobbMI", "Sodium"));
        cache.retain_only(&keep(&["AANobbMI", "P7dR8mSH", "238222"]));

        let diff = cache.get_diff();

        assert_eq!(ids(&diff.added), ["P7dR8mSH"]);
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].id, "AANobbMI");
        assert_eq!(ids(&diff.removed), ["gvQqBUqZ"]);
        // Three entries left, less the one added and the one updated.
        assert_eq!(diff.unchanged, 1);
    }

    /// Reporting borrows, so `App::close` can save the cache and then read it.
    #[test]
    fn reporting_leaves_the_cache_readable() {
        let cache = as_loaded(populated());

        assert_eq!(cache.get_diff().unchanged, 3);
        assert_eq!(cache.get_diff().unchanged, 3);
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
        // A pin that names its release, so the snapshot covers a populated
        // `version_name`.
        cache.set_mod(
            modrinth_release("AANobbMI", "v1", "0.6.5"),
            sample_mod("AANobbMI", "Sodium"),
        );

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
    const EMPTY_PATHBUF: [PathBuf; 0] = [];

    fn sorted(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
        paths.sort_unstable();
        paths
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

        assert_eq!(Cache::previous_caches(dir.path()), EMPTY_PATHBUF);
    }

    #[test]
    fn every_old_cache_name_is_found() {
        let dir = TempDir::new("cache-previous-all");
        for name in PREVIOUS_CACHE_FILES {
            fs::write(dir.join(name), "{}").expect("write fixture");
        }

        let found = Cache::previous_caches(dir.path());

        let expected = PREVIOUS_CACHE_FILES
            .iter()
            .map(|name| dir.join(name))
            .collect();
        assert_eq!(sorted(found), sorted(expected));
    }

    #[test]
    fn only_the_old_caches_present_are_found() {
        let dir = TempDir::new("cache-previous-some");
        fs::write(dir.join(".packwizml.cache.json"), "{}").expect("write fixture");

        let found = Cache::previous_caches(dir.path());

        assert_eq!(found, [dir.join(".packwizml.cache.json")]);
    }

    #[test]
    fn the_current_cache_is_not_an_old_one() {
        let dir = TempDir::new("cache-previous-current");
        write_cache(&dir.join(CACHE_PATH), CACHE_VERSION, &populated().data);

        assert_eq!(Cache::previous_caches(dir.path()), EMPTY_PATHBUF);
    }

    #[test]
    fn a_directory_named_like_an_old_cache_is_not_one() {
        let dir = TempDir::new("cache-previous-directory");
        fs::create_dir(dir.join(".packwiz-modlist.cache.json")).expect("create fixture directory");
        let safe_wrap: [PathBuf; 0] = [];
        assert_eq!(Cache::previous_caches(dir.path()), EMPTY_PATHBUF);
    }

    /// The file is only looked for, so one that would not parse is left exactly
    /// as it was found.
    #[test]
    fn an_old_cache_is_found_without_being_rewritten() {
        let dir = TempDir::new("cache-previous-untouched");
        let old = dir.join(".packwiz-modlist.cache.json");
        fs::write(&old, r#"{"AANobbMI":{"cacheId":"v1"}}"#).expect("write fixture");

        Cache::preflight(dir.path());

        assert_eq!(
            Cache::previous_caches(dir.path()),
            std::slice::from_ref(&old)
        );
        assert_eq!(
            fs::read_to_string(&old).expect("read back"),
            r#"{"AANobbMI":{"cacheId":"v1"}}"#
        );
    }
}
