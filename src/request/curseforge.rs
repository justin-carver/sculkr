use minreq::Request;
use serde::{Deserialize, Serialize};

use crate::{
    consts::CURSEFORGE_API,
    env::Secret,
    error::Error,
    request::{CurseForgeId, post},
};

pub type Mods = Vec<Mod>;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Mod {
    pub id: u32,
    pub name: String,
    pub slug: String,
    pub links: ModLinks,
    pub summary: String,
    pub authors: Vec<ModAuthor>,
    /// [None] for projects that never had a logo uploaded
    pub logo: Option<ModLogo>,
}

impl Mod {
    pub fn url(&self) -> String {
        self.links.website_url.clone().unwrap_or_else(|| {
            format!("https://www.curseforge.com/minecraft/mc-mods/{}", self.slug)
        })
    }
}

#[serde_with::serde_as]
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_field_names)]
pub struct ModLinks {
    #[serde_as(as = "serde_with::NoneAsEmptyString")]
    pub website_url: Option<String>,
    #[serde_as(as = "serde_with::NoneAsEmptyString")]
    pub wiki_url: Option<String>,
    #[serde_as(as = "serde_with::NoneAsEmptyString")]
    pub issues_url: Option<String>,
    #[serde_as(as = "serde_with::NoneAsEmptyString")]
    pub source_url: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModAuthor {
    pub id: u32,
    pub name: String,
    pub url: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModLogo {
    pub id: u32,
    pub mod_id: u32,
    pub title: String,
    pub description: String,
    pub thumbnail_url: String,
}

/// The key is handed in rather than looked up here, so the one place that
/// decides between the environment and a `.sculk` stays the one place.
pub fn post_curseforge(endpoint: &str, key: Option<&Secret>) -> Result<Request, Error> {
    let key = key.ok_or(Error::MissingEnv(crate::env::CF_API_KEY))?;

    Ok(post(format!("{CURSEFORGE_API}{endpoint}")).with_header("x-api-key", key.expose()))
}

pub fn get_curseforge_mods(ids: &[CurseForgeId], key: Option<&Secret>) -> Result<Mods, Error> {
    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(rename_all = "camelCase")]
    struct ResponseJson {
        data: Vec<Mod>,
    }

    let body = serde_json::json!({ "modIds": ids, "filterPcOnly": true });
    let response = post_curseforge("/mods", key)?.with_json(&body)?.send()?;

    if response.status_code == 200 {
        response
            .json::<ResponseJson>()
            .map_err(|err| {
                (match response.as_str() {
                    Ok(json) => (json, err).into(),
                    Err(err) => err.into(),
                })
            })
            .map(|m| m.data)
    } else {
        log::debug!(
            "request body sent to CurseForge:\n{}",
            serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string())
        );
        // A 403 from CurseForge carries a zero-byte body, so the headers are the
        // only place left with any signal about which hop rejected the request.
        log::debug!(
            "CurseForge returned {} {} with headers:\n{}",
            response.status_code,
            response.reason_phrase,
            crate::request::describe_headers(&response)
        );
        // The body rides along on the error itself, so it is visible without -vv.
        Err(response.into())
    }
}
