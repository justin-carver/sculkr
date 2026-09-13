#![allow(unused)]

use std::path::PathBuf;

use clap::Parser;

use crate::{
    app::App,
    args::{Cli, Command},
    cache::Cache,
    error::Error,
    format::Formatter,
    parser::{packwiz::PackwizParser, text::TextParser},
    request::{Mod, curseforge::get_curseforge_mods, modrinth::get_modrinth_mods},
};

mod app;
mod args;
mod cache;
mod config;
mod consts;
mod env;
mod error;
mod format;
mod parser;
mod request;
mod util;

fn setup_logging(verbosity: args::Verbosity) {
    let level = verbosity.to_level_filter();

    simple_logger::SimpleLogger::new()
        // Dependencies (rustls) dump a wall of TLS handshake traffic at
        // debug/trace, which buries output. Cap them at warn and let the
        // flags raise only this crate's level.
        .with_level(log::LevelFilter::Warn.min(level))
        .with_module_level(env!("CARGO_CRATE_NAME"), level)
        .without_timestamps()
        .env()
        .init()
        .unwrap_or_default();

    colored::control::set_override(true);

    // TODO: I'm actually unsure if this works the way it should on Windows...
    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).unwrap_or_default();
}

const CACHE_PATH: &str = ".packwiz-modlist.cache.json";

fn run(cli: &Cli, loaded: &config::Loaded, pack_root: &PathBuf) -> Result<(), Error> {
    match std::env::current_dir() {
        Ok(cwd) => log::debug!("working directory: \"{}\"", cwd.display()),
        Err(err) => log::debug!("could not determine working directory: {err}"),
    }

    log::debug!("pack root: \"{}\"", pack_root.display());

    let cache = Cache::load(CACHE_PATH)?;

    // Absent is not fatal: only --json needs the metadata, and a bare directory
    // of *.pw.toml files still lists fine.
    let pack = parser::pack::Pack::read(pack_root.join(parser::pack::PACK_FILE_NAME)).ok();

    let index_file = pack
        .as_ref()
        .and_then(|pack| pack.index.as_ref())
        .map(|index| index.file.as_str());

    let pw_parser = PackwizParser::load_from(&pack_root, index_file)?;
    let packwiz_mods = pw_parser.mods.clone();

    // Resolved here rather than at the request, so the environment-over-file
    // precedence is decided once and the same key is reported by `config`.
    let cf_api_key = loaded.curseforge_api_key().map(|(key, source)| {
        log::debug!("using the {} from {source}", crate::env::CF_API_KEY);
        key
    });

    let app = App::new(
        cache,
        pw_parser,
        cf_api_key,
        pack,
        loaded.config.clone(),
        packwiz_mods,
    );

    if let Err(err) = app.run(&cli) {
        log::error!("{err}");
    }

    if let Err(err) = app.close() {
        log::error!("{err}");
    }

    Ok(())
}

fn main() {
    // Setting up arg parsing outside of app.rs for now, as these are core commands.
    let mut cli = args::Cli::parse();

    // The config file can set the log level, so it has to be read before there
    // is any logging to read it with. Whatever it had to say is replayed below.
    // Everything resolves against the pack root, which is wherever pack.toml
    // is, not wherever --path happens to point. A pack's own `.sculk` sits
    // beside pack.toml, so the root has to be settled before the config is read.
    let start = cli.path.clone().unwrap_or_else(|| PathBuf::from("."));
    let pack_root = parser::pack::find_root(&start).unwrap_or(start);

    let loaded = config::Loaded::discover(&pack_root);
    cli.apply(&loaded.config);

    // Flags
    let verbosity = args::Verbosity::resolve(cli.verbose, cli.quiet);
    // Logging needs to be run after verbosity is resolved, but before any other code that may log.
    setup_logging(verbosity);

    for source in &loaded.sources {
        log::debug!("loaded config from \"{}\"", source.display());
    }

    for warning in &loaded.warnings {
        log::warn!("{warning}");
    }

    crate::env::load_dotenv();

    // Commands / Subcommands
    // Result of the most recently run subcommand
    let result: anyhow::Result<()> = match cli.command {
        Some(Command::Config) => args::config(&mut std::io::stdout().lock(), &cli, &loaded),
        Some(Command::About) => args::about(&mut std::io::stdout().lock()),
        None => run(&cli, &loaded, &pack_root).map_err(anyhow::Error::from),
    };

    if let Err(err) = result {
        log::error!("{err}");
    }
}
