use std::{
    cell::{RefCell, RefMut},
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};

use colored::Colorize;

use crate::{
    Cache, Error, Mod,
    args::Cli,
    cache::CacheId,
    config::Config,
    env::Secret,
    format::Formatter,
    get_curseforge_mods, get_modrinth_mods,
    parser::{
        ParsedCurseForgeId, ParsedModrinthId, Parser, export, pack::Pack, packwiz::PackwizMod,
    },
    request::{CurseForgeId, ModrinthId},
};

/// Warns about ids that were requested but never came back.
///
/// A deleted or renamed project will just vanishes from the list, leaving
/// an incorrect mod count, compared to what packwiz found on disk.
fn warn_missing(source: &str, requested: &HashMap<String, CacheId>, returned: &HashSet<String>) {
    for id in requested.keys() {
        if !returned.contains(id) {
            log::warn!(
                "{source} returned no result for \"{id}\" -- it will be missing from the list"
            );
        }
    }
}

/// Where rendered output goes.
///
/// `--output` is *declined* rather than obeyed when the file is already there
/// and `--force` was not passed, which is the only case worth warning about.
/// Passing no `--output` at all is normal and must not read like a problem.
enum Destination<'a> {
    File(&'a Path),
    Stdout,
    /// `--output` was given, but the file already exists and `--force` was not.
    StdoutBlockedBy(&'a Path),
}

// So we don't have to tee data and modify bifurcating values...
// TODO: Need to see if we can "copy" or switch [Destination]'s between writes.
impl<'a> Destination<'a> {
    fn resolve(cli: &'a Cli) -> Self {
        match cli.output.as_deref() {
            // Treat `--output ""` the same as not passing it at all.
            None => Self::Stdout,
            Some(path) if path.as_os_str().is_empty() => Self::Stdout,
            Some(path) if cli.force || !path.exists() => Self::File(path),
            Some(path) => Self::StdoutBlockedBy(path),
        }
    }

    /// One locked, buffered handle: a few hundred mods would otherwise be a few
    /// hundred lock-and-flush cycles.
    fn writer(&self) -> Result<BufWriter<Box<dyn Write>>, Error> {
        Ok(match self {
            Self::File(path) => BufWriter::new(Box::new(File::create(path)?)),
            _ => BufWriter::new(Box::new(std::io::stdout().lock())),
        })
    }

    /// Reported once, after the payload is flushed.
    ///
    /// This goes to the log rather than into `out`, so a warning cannot land in
    /// the middle of a document being piped into another tool.
    fn report(&self, written: usize) {
        match self {
            Self::File(path) => log::info!("wrote {written} mod(s) to {}", path.display()),
            Self::Stdout => log::info!("listed {written} mod(s)"),
            Self::StdoutBlockedBy(path) => {
                log::warn!(
                    "{} already exists; listed {written} mod(s) on stdout instead. Pass --force to overwrite it.",
                    path.display()
                );
            }
        }
    }
}

pub struct App {
    cache: RefCell<Cache>,
    modrinth_mods: Vec<ParsedModrinthId>,
    curseforge_mods: Vec<ParsedCurseForgeId>,
    // TODO: `None` for cf_api_key is only a problem if the pack turns
    // out to hold CurseForge mods that are not already cached...
    /// Resolved once at startup.
    cf_api_key: Option<Secret>,
    /// [None] when the directory has no `pack.toml`. Only the export needs it,
    /// so a pack without one still lists fine.
    pack: Option<Pack>,
    /// The merged `.sculk` files, for the export to report and extort for rapport...
    config: Config,
    /// The `*.pw.toml` records. Empty for parsers that have no such thing.
    packwiz_mods: Vec<PackwizMod>,
}

impl App {
    pub fn new<P>(
        cache: Cache,
        parser: P,
        cf_api_key: Option<Secret>,
        pack: Option<Pack>,
        config: Config,
        packwiz_mods: Vec<PackwizMod>,
    ) -> Self
    where
        P: Parser,
    {
        let (modrinth_mods, curseforge_mods) = parser.get_mods_owned();

        Self {
            cache: RefCell::new(cache),
            modrinth_mods,
            curseforge_mods,
            cf_api_key,
            pack,
            config,
            packwiz_mods,
        }
    }

    fn get_mods(&self) -> Result<Vec<Mod>, Error> {
        let mut cache = self.cache.borrow_mut();
        let mut mods =
            Vec::<Mod>::with_capacity(self.modrinth_mods.len() + self.curseforge_mods.len());
        let mut mr_mods_ids = Vec::<ModrinthId>::with_capacity(self.modrinth_mods.len());
        let mut cf_mods_ids = Vec::<CurseForgeId>::with_capacity(self.curseforge_mods.len());
        let mut mr_id_map = HashMap::<String, CacheId>::with_capacity(mr_mods_ids.capacity());
        let mut cf_id_map = HashMap::<String, CacheId>::with_capacity(cf_mods_ids.capacity());

        for id in self.modrinth_mods.iter().cloned() {
            match cache.get_mod(id.clone()).cloned() {
                None => {
                    mr_id_map.insert(id.id.clone(), id.clone().into());
                    mr_mods_ids.push(id.into());
                }
                Some(m) => mods.push(m),
            }
        }

        for id in self.curseforge_mods.iter().cloned() {
            match cache.get_mod(id.clone()).cloned() {
                None => {
                    cf_id_map.insert(id.id.to_string(), id.clone().into());
                    cf_mods_ids.push(id.into());
                }
                Some(m) => mods.push(m),
            }
        }

        if !mr_mods_ids.is_empty() {
            let fetched = get_modrinth_mods(mr_mods_ids)?;
            let mut returned = HashSet::<String>::with_capacity(fetched.len());

            for m in fetched {
                returned.insert(m.id.clone());

                match mr_id_map.get(&m.id).cloned() {
                    Some(id) => {
                        cache.set_mod(id, m.clone());
                        mods.push(m);
                    }
                    // An id we never asked for should not take the whole run down.
                    None => {
                        log::warn!(
                            "Modrinth returned unrequested project \"{}\"; ignoring",
                            m.id
                        );
                    }
                }
            }

            warn_missing("Modrinth", &mr_id_map, &returned);
        }

        if !cf_mods_ids.is_empty() {
            let fetched = get_curseforge_mods(cf_mods_ids, self.cf_api_key.as_ref())?;
            let mut returned = HashSet::<String>::with_capacity(fetched.len());

            for m in fetched.into_iter().map(Mod::from) {
                returned.insert(m.id.clone());

                match cf_id_map.get(&m.id).cloned() {
                    Some(id) => {
                        cache.set_mod(id, m.clone());
                        mods.push(m);
                    }
                    None => {
                        log::warn!(
                            "CurseForge returned unrequested project \"{}\"; ignoring",
                            m.id
                        );
                    }
                }
            }

            warn_missing("CurseForge", &cf_id_map, &returned);
        }

        Ok(mods)
    }

    /// Every mod in the pack in alphabetical order by title, then id.
    pub fn sorted_mods(&self) -> Result<Vec<Mod>, Error> {
        let mut mods = self.get_mods()?;

        // Tie-break on id so mods sharing a title still land in a fixed order.
        mods.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });

        Ok(mods)
    }

    pub fn run(&self, cli: Cli) -> Result<(), Error> {
        let destination = Destination::resolve(&cli);

        if cli.json {
            if cli.format.is_some() {
                log::warn!("--format is ignored with --json, which emits a whole document");
            }

            return self.run_export(&destination);
        }

        // Parsed before anything is fetched, so a typo in the template costs a
        // message instead of a round of API calls.
        let formatter = Formatter::new(cli.format())?;
        let mods = self.sorted_mods()?;

        let mut out = destination.writer()?;

        formatter.write_all(&mut out, &mods)?;
        out.flush()?;

        destination.report(mods.len());

        Ok(())
    }

    /// The whole pack as one JSON document.
    fn run_export(&self, destination: &Destination<'_>) -> Result<(), Error> {
        let pack = self.pack.as_ref().ok_or_else(|| {
            Error::MissingPackToml(crate::parser::pack::PACK_FILE_NAME.to_owned())
        })?;

        let mods = self.sorted_mods()?;
        let document = export::generate_document(pack, &self.config, &self.packwiz_mods, &mods)?;

        let mut out = destination.writer()?;

        writeln!(out, "{document}")?;
        out.flush()?;

        destination.report(mods.len());

        Ok(())
    }

    /// Persists the cache, pruning anything no longer in the pack first.
    pub fn close(&self) -> Result<(), Error> {
        let mut cache = self.cache.borrow_mut();

        let installed: HashSet<String> = self
            .modrinth_mods
            .iter()
            .map(|m| m.id.clone())
            .chain(self.curseforge_mods.iter().map(|m| m.id.to_string()))
            .collect();

        let pruned = cache.retain_only(&installed);

        if pruned > 0 {
            log::debug!("pruned {pruned} cached mod(s) no longer in the pack");
        }

        cache.save()
    }
}
