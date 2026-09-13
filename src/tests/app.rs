//! Tests for [`App`].

use super::*;

/// [`App::close`]: what survives in the cache file once a run is over.
///
/// `close` never fetches, so these build the cache through its public API
/// in a scratch directory, close, and read the file back.
mod close {
    use std::fs;

    use super::*;
    use crate::tests::support::{TempDir, curseforge, modrinth, sample_mod};

    /// An app over the given pack mods. `close` reads nothing else, so the
    /// rest stays empty rather than going through a [`Parser`].
    fn app(
        cache: Cache,
        modrinth_mods: Vec<ParsedModrinthId>,
        curseforge_mods: Vec<ParsedCurseForgeId>,
    ) -> App {
        App {
            cache: RefCell::new(cache),
            modrinth_mods,
            curseforge_mods,
            cf_api_key: None,
            pack: None,
            config: Config::default(),
            packwiz_mods: Vec::new(),
        }
    }

    /// A dirty cache backed by `file`, holding each id at version `v1`.
    fn cache_at(file: &Path, modrinth_ids: &[&str], curseforge_ids: &[i32]) -> Cache {
        let mut cache = Cache::load(file).expect("an absent file is not an error");

        for &id in modrinth_ids {
            cache.set_mod(modrinth(id, "v1"), sample_mod(id, id));
        }
        for &id in curseforge_ids {
            cache.set_mod(curseforge(id, "v1"), sample_mod(&id.to_string(), "cf"));
        }

        cache
    }

    fn saved_ids(file: &Path) -> Vec<String> {
        let cache = Cache::load(file).expect("load what close saved");
        let mut ids: Vec<String> = cache.get_data().keys().cloned().collect();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn mods_no_longer_in_the_pack_are_pruned_from_the_file() {
        let dir = TempDir::new("app-close-prunes");
        let cache = cache_at(&dir.cache_file(), &["AANobbMI", "gvQqBUqZ"], &[
            238_222, 306_612,
        ]);

        // One mod from each host has left the pack since it was cached.
        let app = app(cache, vec![modrinth("AANobbMI", "v1")], vec![curseforge(
            238_222, "v1",
        )]);
        app.close().expect("close");

        assert_eq!(saved_ids(&dir.cache_file()), ["238222", "AANobbMI"]);
    }

    /// Pruning goes by project, not pinned version: a mod whose version
    /// moved is still in the pack, and its entry is replaced on the next
    /// fetch rather than dropped here.
    #[test]
    fn a_mod_whose_pinned_version_moved_is_kept() {
        let dir = TempDir::new("app-close-version-moved");
        let cache = cache_at(&dir.cache_file(), &["AANobbMI"], &[238_222]);

        let app = app(cache, vec![modrinth("AANobbMI", "v2")], vec![curseforge(
            238_222, "v2",
        )]);
        app.close().expect("close");

        assert_eq!(saved_ids(&dir.cache_file()), ["238222", "AANobbMI"]);
    }

    #[test]
    fn an_unchanged_pack_does_not_rewrite_the_file() {
        let dir = TempDir::new("app-close-unchanged");
        cache_at(&dir.cache_file(), &["AANobbMI"], &[238_222])
            .save()
            .expect("seed the cache file");
        let cache = Cache::load(dir.cache_file()).expect("load the seeded file");

        // Swap the file out from under the loaded cache, so a write would show.
        fs::write(dir.cache_file(), "sentinel").expect("overwrite fixture");
        let app = app(cache, vec![modrinth("AANobbMI", "v1")], vec![curseforge(
            238_222, "v1",
        )]);
        app.close().expect("close");

        assert_eq!(
            fs::read_to_string(dir.cache_file()).expect("read back"),
            "sentinel"
        );
    }
}
