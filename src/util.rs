use std::path::{Component, Path, PathBuf};

/// Resolves `path` to an absolute, normalized form for diagnostics.
/// This is a more robust version of `std::path::absolute` that does not
/// require the path to exist, and does not resolve symlinks.
pub fn resolve_for_display<P>(path: P) -> PathBuf
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();

    for component in absolute.components() {
        match component {
            // Only pop a real directory name; popping past a root would silently
            // rewrite the path into something that was never requested.
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push(component);
                }
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }

    out
}

// #[cfg(test)]
// mod test {
//     #[test]
//     fn path_resolves_to_correct_display() {
//         let path = path_resolves_to_correct_display("");
//     }
// }
