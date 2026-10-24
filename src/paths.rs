use crate::{Error, Result};
use std::path::{Component, Path, PathBuf};

pub fn default_data_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or(Error::HomeDirUnavailable)?;
    Ok(home
        .join("Library")
        .join("Application Support")
        .join("katok"))
}

/// Resolves the configured semantic dir against `data_dir` (a relative path is
/// taken inside it). `wipe-index` deletes this directory recursively, so one that
/// is the data dir or an ancestor of it (`""`, `"."`, `".."`, ...) is refused
/// rather than letting a wipe take the archive with it.
pub fn resolve_semantic_dir(data_dir: &Path, configured: &Path) -> Result<PathBuf> {
    let semantic_dir = if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        data_dir.join(configured)
    };
    if lexically_normal(data_dir).starts_with(lexically_normal(&semantic_dir)) {
        return Err(Error::InvalidSemanticPath(configured.to_path_buf()));
    }
    Ok(semantic_dir)
}

/// Drops `.` and folds `..` into the preceding component without touching the
/// filesystem; `..` at the root stays at the root.
fn lexically_normal(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if normal.file_name().is_some() {
                    normal.pop();
                } else if !normal.has_root() {
                    normal.push(component);
                }
            }
            other => normal.push(other),
        }
    }
    normal
}

pub fn ensure_private_dir(path: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(Error::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).map_err(Error::Io)?.permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(path, permissions).map_err(Error::Io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_semantic_dir;
    use std::path::Path;

    #[test]
    fn a_relative_semantic_dir_resolves_inside_the_data_dir() {
        let data_dir = Path::new("/data/katok");

        assert_eq!(
            resolve_semantic_dir(data_dir, Path::new("semantic")).expect("resolve"),
            data_dir.join("semantic")
        );
        assert_eq!(
            resolve_semantic_dir(data_dir, Path::new("/elsewhere/semantic")).expect("resolve"),
            Path::new("/elsewhere/semantic")
        );
    }

    #[test]
    fn a_semantic_dir_holding_the_data_dir_is_refused() {
        let data_dir = Path::new("/data/katok");

        for configured in [
            "",
            ".",
            "..",
            "nested/../..",
            "/data",
            "/data/katok/",
            "/../..",
        ] {
            assert!(
                resolve_semantic_dir(data_dir, Path::new(configured)).is_err(),
                "{configured:?}"
            );
        }
    }
}
