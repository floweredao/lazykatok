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

/// Prepare a directory named by the caller for output that holds message bodies.
///
/// A directory this has to create is owner-only, like [`ensure_private_dir`]. One that already
/// exists keeps the permissions it came with: it belongs to whoever made it, and tightening it
/// would change how every other user and process sees a path lazykatok was only asked to write
/// into. What protects the content there is the file mode, which every writer sets to 0600.
pub fn ensure_output_dir(path: &Path) -> Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    ensure_private_dir(path)
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
    use super::{ensure_output_dir, resolve_semantic_dir};
    use std::path::Path;

    #[cfg(unix)]
    #[test]
    fn an_output_dir_is_created_owner_only_and_an_existing_one_is_left_alone() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("tempdir");
        let mode = |path: &Path| {
            std::fs::metadata(path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777
        };

        let created = root.path().join("created");
        ensure_output_dir(&created).expect("create output dir");
        assert_eq!(mode(&created), 0o700);

        let existing = root.path().join("existing");
        std::fs::create_dir(&existing).expect("create dir");
        std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o755))
            .expect("set mode");
        ensure_output_dir(&existing).expect("accept existing output dir");
        assert_eq!(mode(&existing), 0o755);
    }

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
