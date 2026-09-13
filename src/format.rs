//! Renders a [`Mod`] through a user-supplied template.
//!
//! A template is a string literal with `{PLACEHOLDER}` holes in it, one per
//! field the cache holds. It is parsed once up front so a typo fails before
//! any network calls, and so rendering a few hundred mods is just appends.

use std::{
    borrow::Cow,
    cmp::Ordering,
    fmt,
    io::Write,
    iter::Peekable,
    str::{Chars, FromStr},
};

use serde_with::{DeserializeFromStr, formats::Format};

use crate::request::Mod;

/// The template used when `--format` is not given.
///
/// Escapes are resolved by [`Formatter::new`], so the `\n` here is still two
/// characters at this point -- which is what keeps `--help` readable.
pub const DEFAULT_FORMAT: &str = r"- [{NAME}]({URL}) - {DESCRIPTION}\n";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Field {
    Id,
    Slug,
    #[default]
    Name,
    Description,
    Url,
    IconUrl,
    SourceUrl,
    IssuesUrl,
    WikiUrl,
    License,
    LicenseId,
    LicenseUrl,
    Authors,
    AuthorUrls,
    AuthorsMd,
    Index,
}

const FIELDS: &[(&str, Field, &str)] = &[
    (
        "ID",
        Field::Id,
        "project id (Modrinth base62, CurseForge numeric)",
    ),
    ("SLUG", Field::Slug, "url slug, e.g. \"sodium\""),
    ("NAME", Field::Name, "project title"),
    ("TITLE", Field::Name, "alias of {NAME}"),
    (
        "DESCRIPTION",
        Field::Description,
        "short description / summary",
    ),
    ("DESC", Field::Description, "alias of {DESCRIPTION}"),
    ("URL", Field::Url, "project page on Modrinth/CurseForge"),
    ("ICON_URL", Field::IconUrl, "project icon image"),
    ("SOURCE_URL", Field::SourceUrl, "source repository"),
    ("ISSUES_URL", Field::IssuesUrl, "issue tracker"),
    ("WIKI_URL", Field::WikiUrl, "wiki / documentation"),
    ("LICENSE", Field::License, "license name (Modrinth only)"),
    ("LICENSE_ID", Field::LicenseId, "license id, e.g. \"MIT\""),
    ("LICENSE_URL", Field::LicenseUrl, "license text"),
    (
        "AUTHORS",
        Field::Authors,
        "author names, or the owning organization, comma separated",
    ),
    (
        "AUTHOR_URLS",
        Field::AuthorUrls,
        "author pages, comma separated",
    ),
    ("AUTHORS_MD", Field::AuthorsMd, "authors as markdown links"),
    (
        "INDEX",
        Field::Index,
        "this mod's position in the list, starting at 1",
    ),
];

impl Field {
    fn parse(name: &str) -> Option<Self> {
        let name = name.trim().to_uppercase();

        FIELDS
            .iter()
            .find(|(placeholder, ..)| *placeholder == name)
            .map(|(_, field, _)| *field)
    }

    /// The canonical placeholder name, the first row naming this field.
    fn name(self) -> &'static str {
        FIELDS
            .iter()
            .find(|(_, field, _)| *field == self)
            .map_or("", |(name, ..)| *name)
    }

    /// Fields the API leaves out render as an empty string rather than as some
    /// stand-in text, so a template stays in control of its own punctuation.
    fn render<'a>(self, m: &'a Mod, position: usize) -> Cow<'a, str> {
        let optional = |value: &'a Option<String>| {
            value.as_ref().map_or(Cow::Borrowed(""), |value: &String| {
                Cow::Borrowed(value.as_str())
            })
        };

        let join = |f: fn(&crate::request::Author) -> &str| {
            Cow::Owned(m.authors.iter().map(f).collect::<Vec<_>>().join(", "))
        };

        match self {
            Self::Id => Cow::Borrowed(m.id.as_str()),
            Self::Slug => Cow::Borrowed(m.slug.as_str()),
            Self::Name => Cow::Borrowed(m.title.as_str()),
            Self::Description => Cow::Borrowed(m.description.as_str()),
            Self::Url => Cow::Borrowed(m.mod_url.as_str()),
            Self::IconUrl => optional(&m.icon_url),
            Self::SourceUrl => optional(&m.source_url),
            Self::IssuesUrl => optional(&m.issues_url),
            Self::WikiUrl => optional(&m.wiki_url),
            Self::License => m.license.as_ref().map_or(Cow::Borrowed(""), |license| {
                Cow::Borrowed(license.name.as_str())
            }),
            Self::LicenseId => m.license.as_ref().map_or(Cow::Borrowed(""), |license| {
                Cow::Borrowed(license.id.as_str())
            }),
            Self::LicenseUrl => m
                .license
                .as_ref()
                .map_or(Cow::Borrowed(""), |license| optional(&license.url)),
            Self::Authors => join(|author| author.name.as_str()),
            Self::AuthorUrls => join(|author| author.url.as_str()),
            Self::AuthorsMd => Cow::Owned(
                m.authors
                    .iter()
                    .map(|author| format!("[{}]({})", author.name, author.url))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            Self::Index => Cow::Owned(position.to_string()),
        }
    }
}

/// The field a modlist is ordered by: any placeholder but `{INDEX}`.
///
/// Spelled the way the placeholder is, aliases and case included, so
/// `--sort-by title` and `{NAME}` name the same field.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, DeserializeFromStr)]
pub struct SortKey(Field);

impl FromStr for SortKey {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match Field::parse(name) {
            // An index is a mod's place in the sorted list, so it cannot also
            // be what decides that place.
            Some(Field::Index) => {
                Err("INDEX is a mod's position after sorting, so it cannot be sorted by".to_owned())
            }
            Some(field) => Ok(Self(field)),
            None => {
                let expected = FIELDS
                    .iter()
                    .filter(|(_, field, _)| *field != Field::Index)
                    .map(|(name, ..)| *name)
                    .collect::<Vec<_>>()
                    .join(", ");

                Err(format!(
                    "unknown sort field \"{name}\", expected one of: {expected}"
                ))
            }
        }
    }
}

impl fmt::Display for SortKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.name())
    }
}

impl SortKey {
    /// Orders `mods` A-Z by this field, or Z-A when `reverse` is set.
    ///
    /// Mods with no value for the field go last in both directions, since a
    /// block of blanks at the top of the list is not an order anyone wants.
    /// Ties fall back to the id, so the output never depends on the order the
    /// APIs happened to answer in.
    pub fn sort(self, mods: &mut Vec<Mod>, reverse: bool) {
        // Rendered once per mod rather than once per comparison, since the
        // author fields allocate.
        let mut keyed: Vec<(String, Mod)> = mods
            .drain(..)
            .map(|m| (self.0.render(&m, 0).into_owned(), m))
            .collect();

        keyed.sort_by(|(a_key, a), (b_key, b)| {
            let by_field = match (a_key.is_empty(), b_key.is_empty()) {
                (false, false) => {
                    let ordering = natural_cmp(a_key, b_key);
                    if reverse {
                        ordering.reverse()
                    } else {
                        ordering
                    }
                }
                // `false < true`, so the blank one sorts after.
                (a_blank, b_blank) => a_blank.cmp(&b_blank),
            };

            by_field.then_with(|| a.id.cmp(&b.id))
        });

        mods.extend(keyed.into_iter().map(|(_, m)| m));
    }
}

/// Compares the way a person reads a list: case aside, and with runs of digits
/// compared by value, so "Mod 2" comes before "Mod 10".
///
/// Strings that only differ in case or leading zeros still get a fixed order,
/// from a plain comparison as the last word.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut left = a.chars().peekable();
    let mut right = b.chars().peekable();

    loop {
        let ordering = match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(l), Some(r)) if l.is_ascii_digit() && r.is_ascii_digit() => {
                cmp_numbers(&take_digits(&mut left), &take_digits(&mut right))
            }
            (Some(l), Some(r)) => {
                left.next();
                right.next();
                l.to_lowercase().cmp(r.to_lowercase())
            }
        };

        if ordering.is_ne() {
            return ordering;
        }
    }
}

fn take_digits(chars: &mut Peekable<Chars<'_>>) -> String {
    let mut digits = String::new();

    while let Some(c) = chars.next_if(char::is_ascii_digit) {
        digits.push(c);
    }

    digits
}

/// Compares two digit runs by value without parsing them, so a `CurseForge` id
/// or a long version string cannot overflow anything.
fn cmp_numbers(a: &str, b: &str) -> Ordering {
    let a = a.trim_start_matches('0');
    let b = b.trim_start_matches('0');

    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// Flattens line breaks in a substituted value.
/// Only allocates when there is actually something to collapse.
fn single_line(value: Cow<'_, str>) -> Cow<'_, str> {
    if !value.contains(['\n', '\r']) {
        return value;
    }

    Cow::Owned(
        value
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// The placeholder table rendered as an aligned list, for `--help` and for the
/// "unknown placeholder" error.
///
/// Makes it easy to add a new placeholder: add a row to `FIELDS` and it is automatically
/// documented and tested.
#[allow(clippy::arithmetic_side_effects)]
pub fn placeholder_help() -> String {
    let width = FIELDS
        .iter()
        .map(|(name, ..)| name.len())
        .max()
        .unwrap_or_default();

    FIELDS
        .iter()
        .map(|(name, _, doc)| {
            format!(
                "  {{{name}}}{:width$}  {doc}",
                "",
                width = width - name.len()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(thiserror::Error, Debug)]
pub enum FormatError {
    #[error("unclosed \"{{\" at position {0} in the format string")]
    Unclosed(usize),
    #[error(
    "unknown placeholder \"{{{0}}}\" in the format string\nAvailable placeholders:\n{help}",
    help = placeholder_help()
  )]
    UnknownPlaceholder(String),
    #[error("unknown escape \"\\{0}\" in the format string; use \\\\ for a literal backslash")]
    UnknownEscape(char),
    #[error("format string ends with a dangling \"\\\"")]
    TrailingBackslash,
}

#[derive(Debug, Clone)]
enum Segment {
    Literal(String),
    Field(Field),
}

#[derive(Debug, Clone)]
pub struct Formatter {
    segments: Vec<Segment>,
}

impl Formatter {
    /// Parses `template` into segments, resolving `\n`-style escapes as it goes.
    pub fn new(template: &str) -> Result<Self, FormatError> {
        let mut segments = Vec::new();
        let mut literal = String::new();
        let mut chars = template.char_indices();

        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some((_, 'n')) => literal.push('\n'),
                    Some((_, 't')) => literal.push('\t'),
                    Some((_, 'r')) => literal.push('\r'),
                    Some((_, '0')) => literal.push('\0'),
                    Some((_, '\\')) => literal.push('\\'),
                    Some((_, '{')) => literal.push('{'),
                    Some((_, '}')) => literal.push('}'),
                    Some((_, c)) => return Err(FormatError::UnknownEscape(c)),
                    None => return Err(FormatError::TrailingBackslash),
                },
                '{' => {
                    let mut name = String::new();
                    let mut closed = false;

                    for (_, c) in chars.by_ref() {
                        if c == '}' {
                            closed = true;
                            break;
                        }

                        name.push(c);
                    }

                    if !closed {
                        return Err(FormatError::Unclosed(i));
                    }

                    let field = Field::parse(&name).ok_or(FormatError::UnknownPlaceholder(name))?;

                    if !literal.is_empty() {
                        segments.push(Segment::Literal(std::mem::take(&mut literal)));
                    }

                    segments.push(Segment::Field(field));
                }
                c => literal.push(c),
            }
        }

        if !literal.is_empty() {
            segments.push(Segment::Literal(literal));
        }

        Ok(Self { segments })
    }

    /// Renders one mod at `position`, a one-based place in the list.
    pub fn render(&self, m: &Mod, position: usize) -> String {
        let mut out = String::new();

        for segment in &self.segments {
            match segment {
                Segment::Literal(text) => out.push_str(text),
                Segment::Field(field) => out.push_str(&single_line(field.render(m, position))),
            }
        }

        out
    }

    /// Writes every mod, numbering them as it goes.
    ///
    /// The list is the unit rather than the entry, so `{COUNT}` is numbered in
    /// exactly one place and a caller cannot forget to advance it.
    pub fn write_all<W>(&self, out: &mut W, mods: &[Mod]) -> std::io::Result<()>
    where
        W: Write,
    {
        for (index, m) in mods.iter().enumerate() {
            for segment in &self.segments {
                match segment {
                    Segment::Literal(text) => out.write_all(text.as_bytes())?,
                    Segment::Field(field) => {
                        out.write_all(
                            single_line(field.render(m, index.saturating_add(1))).as_bytes(),
                        )?;
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{Author, License};

    // Just gonna fill out some of these fields...
    fn test_mod() -> Mod {
        Mod {
            id: "AANobbMI".into(),
            slug: "sodium".into(),
            title: "Sodium".into(),
            description: "A modern rendering engine".into(),
            mod_url: "https://modrinth.com/mod/sodium".into(),
            license: Some(License {
                id: "LGPL-3.0-only".into(),
                name: "GNU Lesser General Public License v3.0 only".into(),
                url: None,
            }),
            authors: vec![Author {
                name: "jellysquid3".into(),
                url: "https://modrinth.com/user/jellysquid3".into(),
            }],
            icon_url: Some("https://cdn.modrinth.com/icon.webp".into()),
            source_url: None,
            issues_url: None,
            wiki_url: None,
        }
    }

    #[test]
    fn default_format_renders_a_markdown_list_item() {
        let out = Formatter::new(DEFAULT_FORMAT)
            .unwrap()
            .render(&test_mod(), 1);

        assert_eq!(
            out,
            "- [Sodium](https://modrinth.com/mod/sodium) - A modern rendering engine\n"
        );
    }

    #[test]
    fn missing_fields_render_as_empty_rather_than_a_placeholder() {
        let out = Formatter::new("{NAME}|{SOURCE_URL}|{LICENSE_URL}")
            .unwrap()
            .render(&test_mod(), 1);

        assert_eq!(out, "Sodium||");
    }

    #[test]
    fn placeholder_names_are_case_insensitive_and_aliased() {
        let m = test_mod();
        let canonical = Formatter::new("{NAME}{DESCRIPTION}").unwrap().render(&m, 1);
        let aliased = Formatter::new("{title}{Desc}").unwrap().render(&m, 1);

        assert_eq!(canonical, aliased);
    }

    #[test]
    fn escapes_are_resolved_including_literal_braces() {
        let out = Formatter::new(r"\{{NAME}\}\t\\\n")
            .unwrap()
            .render(&test_mod(), 1);

        assert_eq!(out, "{Sodium}\t\\\n");
    }

    #[test]
    fn authors_render_joined_and_as_markdown() {
        let m = test_mod();

        assert_eq!(
            Formatter::new("{AUTHORS}").unwrap().render(&m, 1),
            "jellysquid3"
        );
        assert_eq!(
            Formatter::new("{AUTHORS_MD}").unwrap().render(&m, 1),
            "[jellysquid3](https://modrinth.com/user/jellysquid3)"
        );
    }

    #[test]
    fn a_line_break_in_the_data_cannot_break_the_template() {
        let mut m = test_mod();
        m.description = "A modern rendering engine\r\n\nCompatible with Iris.\n".into();

        let out = Formatter::new(DEFAULT_FORMAT).unwrap().render(&m, 1);

        // One entry, one line: the only newline is the template's own.
        assert_eq!(
            out,
            "- [Sodium](https://modrinth.com/mod/sodium) - A modern rendering engine Compatible with Iris.\n"
        );
        assert_eq!(out.matches('\n').count(), 1);
    }

    #[test]
    fn collapsing_borrows_when_there_is_nothing_to_collapse() {
        assert!(matches!(
            single_line(Cow::Borrowed("one line")),
            Cow::Borrowed(_)
        ));
        assert!(matches!(
            single_line(Cow::Borrowed("two\nlines")),
            Cow::Owned(_)
        ));
    }

    /// The bug this API shape exists to prevent: numbering that never advances
    /// because the caller rendered each mod on its own.
    #[test]
    fn index_advances_across_the_list() {
        let mods = vec![test_mod(), test_mod(), test_mod()];
        let mut out = Vec::new();

        Formatter::new(r"{INDEX}. {NAME}\n")
            .unwrap()
            .write_all(&mut out, &mods)
            .unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "1. Sodium\n2. Sodium\n3. Sodium\n"
        );
    }

    #[test]
    fn an_empty_list_writes_nothing() {
        let mut out = Vec::new();

        Formatter::new(DEFAULT_FORMAT)
            .unwrap()
            .write_all(&mut out, &[])
            .unwrap();

        assert_eq!(out, <[u8; 0]>::default());
    }

    #[test]
    fn a_bad_template_is_rejected_at_parse_time() {
        assert!(matches!(
          Formatter::new("{NOPE}"),
          Err(FormatError::UnknownPlaceholder(name)) if name == "NOPE"
        ));
        assert!(matches!(
            Formatter::new("{NAME"),
            Err(FormatError::Unclosed(0))
        ));
        assert!(matches!(
            Formatter::new(r"\q"),
            Err(FormatError::UnknownEscape('q'))
        ));
        assert!(matches!(
            Formatter::new(r"{NAME}\"),
            Err(FormatError::TrailingBackslash)
        ));
    }

    /// Every row must be reachable through the parser, or `--help` would be
    /// advertising a placeholder that will fail.
    #[test]
    fn every_documented_placeholder_parses() {
        for (name, field, _) in FIELDS {
            assert_eq!(Field::parse(name), Some(*field), "{name} did not parse");
        }
    }

    mod sorting {
        use super::*;

        fn named(id: &str, title: &str) -> Mod {
            Mod {
                id: id.into(),
                title: title.into(),
                ..test_mod()
            }
        }

        fn sorted(mut mods: Vec<Mod>, key: &str, reverse: bool) -> Vec<String> {
            key.parse::<SortKey>().unwrap().sort(&mut mods, reverse);
            mods.into_iter()
                .map(|m| format!("{}:{}", m.id, m.title))
                .collect()
        }

        #[test]
        fn numbers_compare_by_value_and_case_is_ignored() {
            assert_eq!(natural_cmp("Mod 2", "Mod 10"), Ordering::Less);
            assert_eq!(natural_cmp("apple", "Banana"), Ordering::Less);
            assert_eq!(natural_cmp("Create", "create: steam"), Ordering::Less);
            assert_eq!(natural_cmp("1.20.1", "1.9"), Ordering::Greater);
            // Equal by value, but still not a tie.
            assert_ne!(natural_cmp("007", "7"), Ordering::Equal);
        }

        #[test]
        fn the_default_is_name_a_to_z() {
            let mods = vec![
                named("1", "Sodium"),
                named("2", "iris"),
                named("3", "Mod 10"),
                named("4", "Mod 2"),
            ];

            assert_eq!(sorted(mods, &SortKey::default().to_string(), false), [
                "2:iris", "4:Mod 2", "3:Mod 10", "1:Sodium"
            ]);
        }

        #[test]
        fn reverse_is_z_to_a_but_keeps_ties_in_id_order() {
            let mods = vec![named("b", "Same"), named("c", "Other"), named("a", "Same")];

            assert_eq!(sorted(mods, "name", true), ["a:Same", "b:Same", "c:Other"]);
        }

        /// A field most mods leave blank would otherwise open the list with a
        /// wall of entries that have nothing to be sorted by.
        #[test]
        fn blanks_go_last_in_both_directions() {
            let with_source = |id: &str, source: Option<&str>| Mod {
                source_url: source.map(Into::into),
                ..named(id, id)
            };
            let mods = || {
                vec![
                    with_source("1", None),
                    with_source("2", Some("https://b.example")),
                    with_source("3", Some("https://a.example")),
                ]
            };

            assert_eq!(sorted(mods(), "SOURCE_URL", false), ["3:3", "2:2", "1:1"]);
            assert_eq!(sorted(mods(), "SOURCE_URL", true), ["2:2", "3:3", "1:1"]);
        }

        #[test]
        fn sort_fields_are_spelled_like_placeholders() {
            assert_eq!("title".parse::<SortKey>(), "NAME".parse::<SortKey>());
            assert_eq!(
                "desc".parse::<SortKey>().unwrap().to_string(),
                "DESCRIPTION"
            );
        }

        #[test]
        fn index_and_unknown_fields_are_rejected() {
            let index = "index".parse::<SortKey>().unwrap_err();
            let unknown = "nmae".parse::<SortKey>().unwrap_err();

            assert!(index.contains("INDEX"), "{index}");
            assert!(unknown.contains("\"nmae\""), "{unknown}");
            assert!(unknown.contains("NAME, TITLE"), "{unknown}");
            assert!(!unknown.contains("INDEX"), "{unknown}");
        }
    }
}
