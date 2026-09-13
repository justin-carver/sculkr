use std::collections::{HashMap, HashSet};

use minreq::Request;
use serde::{Deserialize, Serialize};

use crate::{
    consts::{MODRINTH_API, MODRINTH_API_V3},
    error::Error,
    request::{Author, ModrinthId, get, json_or_error},
};

pub type Projects = Vec<Project>;
/// One inner [`Vec`] per requested team, in no particular order -- each member
/// carries its own `team_id`, which is what makes them matchable.
pub type Teams = Vec<Vec<TeamMember>>;

#[allow(clippy::struct_field_names)] // reducing this to `type` throws keyword errors
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Project {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub body: String,
    pub team: String,
    /// "mod", "modpack", "resourcepack", ... -- also the path segment the site
    /// uses, so it is what turns a slug into a project URL.
    pub project_type: Option<String>,
    /// Set when an organization owns the project instead of a person. Such a
    /// project has an empty team, and the site credits the organization.
    pub organization: Option<String>,
    pub icon_url: Option<String>,
    pub issues_url: Option<String>,
    pub source_url: Option<String>,
    pub wiki_url: Option<String>,
    pub license: ProjectLicense,
    pub versions: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProjectLicense {
    pub id: String,
    pub name: String,
    pub url: Option<String>,
}

impl Project {
    /// Modrinth sends no URL on the project payload; the site builds one out of
    /// the project type and slug (`https://modrinth.com/mod/sodium`).
    pub fn url(&self) -> String {
        let kind = self.project_type.as_deref().unwrap_or("mod");

        format!("https://modrinth.com/{kind}/{}", self.slug)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TeamMember {
    pub team_id: String,
    pub user: TeamUser,
    pub role: String,
    /// Absent for a member whose invite is still pending on some responses.
    pub accepted: Option<bool>,
    /// Modrinth's own display order for the team.
    pub ordering: Option<i64>,
}

/// Only the fields worth keeping: the `/teams` payload nests a full user
/// object, and serde drops the rest.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TeamUser {
    pub id: String,
    pub username: String,
}

impl TeamMember {
    /// Pending invitees sit on the team without being credited on the site.
    fn is_credited(&self) -> bool {
        self.accepted.unwrap_or(true)
    }

    fn into_author(self) -> Author {
        Author {
            url: format!("https://modrinth.com/user/{}", self.user.username),
            name: self.user.username,
        }
    }
}

/// Only the fields worth keeping off the v3 organization payload.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Organization {
    pub id: String,
    pub name: String,
    pub slug: String,
}

impl Organization {
    fn into_author(self) -> Author {
        Author {
            url: format!("https://modrinth.com/organization/{}", self.slug),
            name: self.name,
        }
    }
}

pub fn get_modrinth(endpoint: &str) -> Request {
    get(format!("{MODRINTH_API}{endpoint}"))
}

pub fn get_modrinth_v3(endpoint: &str) -> Request {
    get(format!("{MODRINTH_API_V3}{endpoint}"))
}

pub fn get_modrinth_projects(projects: &[ModrinthId]) -> Result<Projects, Error> {
    let json = serde_json::to_string(&projects)?;
    let response = get_modrinth("/projects").with_param("ids", json).send()?;

    json_or_error("Modrinth", response)
}

pub fn get_modrinth_teams(teams: &[String]) -> Result<Teams, Error> {
    let json = serde_json::to_string(&teams)?;
    let response = get_modrinth("/teams").with_param("ids", json).send()?;

    json_or_error("Modrinth", response)
}

/// Projects, with their team members resolved into [`Author`]s.
///
/// Modrinth credits a *team* rather than a list of users, so author names cost
/// a second bulk request keyed by the team ids the first one returned. That
/// request failing is downgraded to a warning.
pub fn get_modrinth_mods(ids: &[ModrinthId]) -> Result<Vec<crate::request::Mod>, Error> {
    let projects = get_modrinth_projects(ids)?;
    let by_team = authors_by_team(&projects);
    let by_organization = authors_by_organization(&projects, &by_team);

    Ok(projects
        .into_iter()
        .map(|project| {
            let authors = team_authors(&project, &by_team)
                .cloned()
                .or_else(|| {
                    let id = project.organization.as_ref()?;

                    Some(vec![by_organization.get(id)?.clone()])
                })
                .unwrap_or_default();

            let mut m = crate::request::Mod::from(project);

            m.authors = authors;
            m
        })
        .collect())
}

/// A project's team members, or [None] when the team credits nobody -- which
/// is what an organization-owned project looks like from `/teams`.
fn team_authors<'a>(
    project: &Project,
    by_team: &'a HashMap<String, Vec<Author>>,
) -> Option<&'a Vec<Author>> {
    by_team
        .get(&project.team)
        .filter(|authors| !authors.is_empty())
}

/// The organization behind each project whose team credits nobody.
///
/// Modrinth shows such a project as published by the organization, so that is
/// what gets credited.
fn authors_by_organization(
    projects: &[Project],
    by_team: &HashMap<String, Vec<Author>>,
) -> HashMap<String, Author> {
    let ids: Vec<String> = projects
        .iter()
        .filter(|project| team_authors(project, by_team).is_none())
        .filter_map(|project| project.organization.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    if ids.is_empty() {
        return HashMap::new();
    }

    match get_modrinth_organizations(&ids) {
        Ok(organizations) => organizations
            .into_iter()
            .map(|organization| (organization.id.clone(), organization.into_author()))
            .collect(),
        Err(err) => {
            log::warn!(
                "could not fetch Modrinth organizations ({err}); \
         organization-owned mods will have no author"
            );
            HashMap::new()
        }
    }
}

fn authors_by_team(projects: &[Project]) -> HashMap<String, Vec<Author>> {
    // Deduplicated: one team can own several projects, and the ids ride in the
    // query string, where length is the limit that bites first.
    let ids: Vec<String> = projects
        .iter()
        .map(|project| project.team.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    if ids.is_empty() {
        return HashMap::new();
    }

    let teams = match get_modrinth_teams(ids.as_slice()) {
        Ok(teams) => teams,
        Err(err) => {
            log::warn!("could not fetch Modrinth teams ({err}); author names will be missing");
            return HashMap::new();
        }
    };

    let mut by_team: HashMap<String, Vec<TeamMember>> = HashMap::new();

    for member in teams.into_iter().flatten() {
        if member.is_credited() {
            by_team
                .entry(member.team_id.clone())
                .or_default()
                .push(member);
        }
    }

    by_team
        .into_iter()
        .map(|(team, mut members)| {
            sort_members(&mut members);

            (
                team,
                members.into_iter().map(TeamMember::into_author).collect(),
            )
        })
        .collect()
}

pub fn get_modrinth_organizations(ids: &[String]) -> Result<Vec<Organization>, Error> {
    let json = serde_json::to_string(&ids)?;
    let response = get_modrinth_v3("/organizations")
        .with_param("ids", json)
        .send()?;

    json_or_error("Modrinth", response)
}

/// Modrinth's display order first, then owners, then alphabetically -- so the
/// credit line is stable no matter what order the API hands members back in.
fn sort_members(members: &mut [TeamMember]) {
    members.sort_by(|a, b| {
        a.ordering
            .unwrap_or_default()
            .cmp(&b.ordering.unwrap_or_default())
            .then_with(|| (a.role != "Owner").cmp(&(b.role != "Owner")))
            .then_with(|| {
                a.user
                    .username
                    .to_lowercase()
                    .cmp(&b.user.username.to_lowercase())
            })
    });
}
