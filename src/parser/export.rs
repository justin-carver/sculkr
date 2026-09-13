//! One exportable document describing a pack -- its `pack.toml` metadata, every
//! `*.pw.toml` record, the project data fetched from Modrinth and `CurseForge`,
//! and the settings the run resolved -- serialized as JSON.
//!
//! Keys are `snake_case` throughout and every struct here is written in the order
//! it should appear, because `serde_json` emits struct fields in declaration
//! order. Reordering a field reorders the output.
//!
//! [`Settings`] is a deliberate *view* rather than `Serialize` on [`Config`].
//! `Config` owns a [`Secret`](crate::env::Secret), which has no `Serialize` on
//! purpose; naming each exported field by hand is what keeps a `CurseForge` key
//! out of a file that tends to get committed. Widen it by adding a field here,
//! never by deriving `Serialize` on `Config`.
//!
//! There is deliberately no `generated_at` field. A timestamp would make the
//! output differ on every run, which breaks `insta` snapshots and makes the
//! file churn in git for no information.
#![allow(clippy::doc_markdown)]

use std::{collections::HashSet, path::Path};

use serde::Serialize;

use crate::{
    Error,
    config::Config,
    parser::{pack::Pack, packwiz::PackwizMod},
    request::{Author, License, Mod},
};

/// Bump when a field is removed or changes meaning, so consumers can branch.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Debug)]
pub struct Document<'a> {
    pub schema_version: u32,
    pub pack: &'a Pack,
    /// The pack's own records, straight from the `*.pw.toml` files.
    pub mods: &'a [PackwizMod],
    /// What the APIs returned for those mods, tagged by where it came from.
    pub projects: Vec<ProjectEntry<'a>>,
    pub settings: Settings<'a>,
}

/// Externally tagged, so a consumer can branch on the source without a
/// discriminant field: `{"Modrinth": {...}}`.
#[derive(Serialize, Debug)]
pub enum ProjectEntry<'a> {
    Modrinth(ProjectView<'a>),
    CurseForge(ProjectView<'a>),
}

/// Everything the cache holds for one project, in a fixed key order.
///
/// Borrowed from [`Mod`] rather than reusing it directly, because `Mod` is
/// camelCase for the cache file and this document is `snake_case`.
#[derive(Serialize, Debug)]
pub struct ProjectView<'a> {
    pub id: &'a str,
    pub slug: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub mod_url: &'a str,
    pub license: Option<&'a License>,
    pub authors: &'a [Author],
    pub icon_url: Option<&'a str>,
    pub source_url: Option<&'a str>,
    pub issues_url: Option<&'a str>,
    pub wiki_url: Option<&'a str>,
}

impl<'a> From<&'a Mod> for ProjectView<'a> {
    fn from(m: &'a Mod) -> Self {
        Self {
            id: &m.id,
            slug: &m.slug,
            title: &m.title,
            description: &m.description,
            mod_url: &m.mod_url,
            license: m.license.as_ref(),
            authors: &m.authors,
            icon_url: m.icon_url.as_deref(),
            source_url: m.source_url.as_deref(),
            issues_url: m.issues_url.as_deref(),
            wiki_url: m.wiki_url.as_deref(),
        }
    }
}

/// The resolved global flags. Nothing from `[secrets]` belongs here.
#[derive(Serialize, Debug)]
pub struct Settings<'a> {
    pub format: Option<&'a str>,
    pub output: Option<&'a Path>,
    pub path: Option<&'a Path>,
}

impl<'a> Document<'a> {
    pub fn new(
        pack: &'a Pack,
        config: &'a Config,
        mods: &'a [PackwizMod],
        projects: &'a [Mod],
    ) -> Self {
        // The pack's own records say which service each id belongs to, so the
        // source is derived here rather than tracked on Mod, which would change
        // the on-disk cache shape.
        let modrinth_ids: HashSet<&str> = mods
            .iter()
            .filter_map(|m| m.update.modrinth.as_ref())
            .map(|update| update.mod_id.as_str())
            .collect();

        let projects = projects
            .iter()
            .map(|m| {
                if modrinth_ids.contains(m.id.as_str()) {
                    ProjectEntry::Modrinth(m.into())
                } else {
                    ProjectEntry::CurseForge(m.into())
                }
            })
            .collect();

        Self {
            schema_version: SCHEMA_VERSION,
            pack,
            mods,
            projects,
            settings: Settings {
                // config.secrets is intentionally absent. See the module docs.
                format: config.format.as_deref(),
                output: config.output.as_deref(),
                path: config.path.as_deref(),
            },
        }
    }

    pub fn to_json_pretty(&self) -> Result<String, Error> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

/// Renders the whole pack as one JSON document, for piping into another tool.
pub fn generate_document(
    pack: &Pack,
    config: &Config,
    mods: &[PackwizMod],
    projects: &[Mod],
) -> Result<String, Error> {
    Document::new(pack, config, mods, projects).to_json_pretty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::packwiz::{
        PackwizDownload, PackwizMod, PackwizModUpdate, PackwizModUpdateCurseforge,
        PackwizModUpdateModrinth,
    };

    fn sample_pack() -> Pack {
        toml::from_str(
            r#"
name = "Create Prime"
author = "minecraft_steve"
version = "1.0.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash = "deadbeef"

[versions]
minecraft = "1.21.1"
neoforge = "21.1.249"
"#,
        )
        .expect("sample should parse")
    }

    /// Shaped after a real Modrinth `*.pw.toml`.
    fn modrinth_pw_mod() -> PackwizMod {
        PackwizMod {
            name: "Sodium".into(),
            filename: "sodium-neoforge-0.8.13.jar".into(),
            side: Some("client".into()),
            download: PackwizDownload {
                url: Some("https://cdn.modrinth.com/data/AANobbMI/x.jar".into()),
                hash: "4f5376".into(),
                hash_format: "sha512".into(),
                mode: None,
            },
            update: PackwizModUpdate {
                modrinth: Some(PackwizModUpdateModrinth {
                    mod_id: "AANobbMI".into(),
                    version: "uMOpc5uV".into(),
                }),
                curseforge: None,
            },
            category: Some("mods".into()),
            path: "mods/sodium.pw.toml".into(),
        }
    }

    /// CurseForge entries carry a mode and no url.
    fn curseforge_pw_mod() -> PackwizMod {
        PackwizMod {
            name: "FTB Teams".into(),
            filename: "ftb-teams-neoforge.jar".into(),
            side: Some("both".into()),
            download: PackwizDownload {
                url: None,
                hash: "62b65f".into(),
                hash_format: "sha1".into(),
                mode: Some("metadata:curseforge".into()),
            },
            update: PackwizModUpdate {
                modrinth: None,
                curseforge: Some(PackwizModUpdateCurseforge {
                    file_id: 8_724_782,
                    project_id: 404_468,
                }),
            },
            category: Some("mods".into()),
            path: "mods/ftb-teams.pw.toml".into(),
        }
    }

    fn project(id: &str, slug: &str, title: &str) -> Mod {
        Mod {
            id: id.into(),
            slug: slug.into(),
            title: title.into(),
            description: "A mod.".into(),
            mod_url: format!("https://example.invalid/{slug}"),
            license: None,
            authors: vec![Author {
                name: "someone".into(),
                url: "https://example.invalid/u".into(),
            }],
            icon_url: None,
            source_url: None,
            issues_url: None,
            wiki_url: None,
        }
    }

    fn render(mods: &[PackwizMod], projects: &[Mod]) -> serde_json::Value {
        let json = generate_document(&sample_pack(), &Config::default(), mods, projects)
            .expect("should serialize");
        serde_json::from_str(&json).expect("the export should be valid json")
    }

    /// Parsed back rather than matched as a string, so pretty-printing
    /// whitespace cannot break the test.
    #[test]
    fn renders_the_document_shape() {
        let doc = render(&[modrinth_pw_mod()], &[project(
            "AANobbMI", "sodium", "Sodium",
        )]);

        assert_eq!(doc["schema_version"], 1);

        assert_eq!(doc["pack"]["name"], "Create Prime");
        assert_eq!(doc["pack"]["pack-format"], "packwiz:1.1.0");
        assert_eq!(doc["pack"]["versions"]["minecraft"], "1.21.1");
        assert_eq!(doc["pack"]["versions"]["neoforge"], "21.1.249");

        // A .pw.toml record, kebab-case on disk and snake_case here.
        assert_eq!(doc["mods"][0]["filename"], "sodium-neoforge-0.8.13.jar");
        assert_eq!(doc["mods"][0]["side"], "client");
        assert_eq!(doc["mods"][0]["download"]["hash_format"], "sha512");
        assert_eq!(doc["mods"][0]["update"]["modrinth"]["mod_id"], "AANobbMI");

        assert_eq!(doc["projects"][0]["Modrinth"]["slug"], "sodium");
        assert_eq!(
            doc["projects"][0]["Modrinth"]["mod_url"],
            "https://example.invalid/sodium"
        );
    }

    /// Every loader key is present on every export, so a consumer can read
    /// `versions.fabric` without checking whether it exists first.
    #[test]
    fn unused_loaders_are_null_rather_than_missing() {
        let doc = render(&[], &[]);
        let versions = doc["pack"]["versions"]
            .as_object()
            .expect("versions should be an object");

        assert!(versions["fabric"].is_null());
        assert!(versions["forge"].is_null());
        assert!(versions["quilt"].is_null());
        assert_eq!(versions["neoforge"], "21.1.249");

        let keys: Vec<&str> = versions.keys().map(String::as_str).collect();
        assert_eq!(keys, ["fabric", "forge", "minecraft", "neoforge", "quilt"]);
    }

    /// The pack's own records are what say which service an id came from.
    #[test]
    fn projects_are_tagged_by_source() {
        let doc = render(&[modrinth_pw_mod(), curseforge_pw_mod()], &[
            project("AANobbMI", "sodium", "Sodium"),
            project("404468", "ftb-teams", "FTB Teams"),
        ]);

        assert_eq!(doc["projects"][0]["Modrinth"]["slug"], "sodium");
        assert!(doc["projects"][0]["CurseForge"].is_null());

        assert_eq!(doc["projects"][1]["CurseForge"]["slug"], "ftb-teams");
        assert!(doc["projects"][1]["Modrinth"].is_null());
    }

    /// The whole reason [`Settings`] exists rather than `Serialize` on `Config`.
    #[test]
    fn a_configured_api_key_never_reaches_the_export() {
        let config: Config = toml::from_str(
            r#"
format = "{NAME}"

[secrets]
cf-api-key = "super-secret-value"
"#,
        )
        .expect("config should parse");

        // Without this the test would pass just as happily on a config that
        // never loaded a key at all, which proves nothing.
        let key = config
            .secrets
            .cf_api_key
            .as_ref()
            .expect("the sample key should have been loaded");

        let json = generate_document(&sample_pack(), &config, &[], &[]).expect("should serialize");

        assert!(
            !json.contains(key.expose()),
            "the raw key reached the export"
        );
        assert!(
            !json.contains(&key.fingerprint()),
            "a fingerprint of the key reached the export"
        );

        // Settings is an allowlist. A new field has to be added deliberately,
        // and this fails until it is added here too.
        let doc: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        let fields: Vec<&str> = doc["settings"]
            .as_object()
            .expect("settings should be an object")
            .keys()
            .map(String::as_str)
            .collect();

        assert_eq!(fields, ["format", "output", "path"]);
    }
}
