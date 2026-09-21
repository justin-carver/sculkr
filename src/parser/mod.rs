pub mod export;
pub mod index;
pub mod pack;
pub mod packwiz;
pub mod text;

#[derive(Debug, Clone)]
pub struct ParsedModrinthId {
    pub cache_id: String,
    pub id: String,
    /// The release this pin resolves to, separate from the `cache_id` it is
    /// keyed on. `None` for a source that names no release.
    pub version_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedCurseForgeId {
    pub cache_id: String,
    pub id: i32,
    /// See [`ParsedModrinthId::version_name`].
    pub version_name: Option<String>,
}

pub trait Parser: Sized {
    fn get_mods_owned(self) -> (Vec<ParsedModrinthId>, Vec<ParsedCurseForgeId>);
    fn get_modrinth_mods(&self) -> Vec<ParsedModrinthId>;
    fn get_curseforge_mods(&self) -> Vec<ParsedCurseForgeId>;
}
