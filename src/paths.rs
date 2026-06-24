use std::path::{Component, Path, PathBuf};

pub const DELETE_DENIED_PREFIXES: &[&str] = &[
    "/System",
    "/usr",
    "/bin",
    "/sbin",
    "/etc",
    "/Library",
    "/Applications",
];

/// Return canonical absolute path.
pub fn normalize_path(path: &Path) -> PathBuf {
    let expanded = expand_user(path);
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(expanded)
    };
    std::fs::canonicalize(&absolute).unwrap_or(absolute)
}

/// Cheap, **syscall-free** comparison key for an already-absolute path.
///
/// Expands a leading `~`, makes the path absolute, and lexically removes `.`
/// and `..` components — but, unlike [`normalize_path`], performs **no**
/// `realpath()`/`canonicalize()` syscall and does not resolve symlinks.
///
/// Scan-tree node paths are built by joining child names onto the canonicalized
/// scan root, so they are already canonical by construction. Comparing them with
/// this key is equivalent to comparing the canonicalized forms, but avoids a
/// `realpath()` per node — which, on the live-merge hot path, is the difference
/// between O(1) string work and a filesystem syscall for every node visited.
pub fn lexical_key(path: &Path) -> PathBuf {
    let expanded = expand_user(path);
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(expanded)
    };
    lexical_clean(&absolute)
}

/// Resolve `.`/`..` and redundant separators purely lexically (no FS access),
/// and collapse the macOS firmlink prefix so `/private/var` and `/var` (also
/// `/tmp`, `/etc`) compare equal — matching what `canonicalize` would yield for
/// these well-known links, but without the syscall.
fn lexical_clean(path: &Path) -> PathBuf {
    // Fast path: the overwhelming majority of scan-tree paths are already clean
    // (no `.`/`..`) and are not the `/private` firmlink, so a single copy beats
    // re-collecting every component. This keeps the live-merge hot path cheap.
    let mut needs_work = false;
    for (i, comp) in path.components().enumerate() {
        match comp {
            Component::CurDir | Component::ParentDir => {
                needs_work = true;
                break;
            }
            Component::Normal(s) if i == 1 && s == "private" => {
                needs_work = true;
                break;
            }
            _ => {}
        }
    }
    if !needs_work {
        return path.to_path_buf();
    }

    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    let mut absolute = false;
    for comp in path.components() {
        match comp {
            Component::RootDir => absolute = true,
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            other => parts.push(other.as_os_str().to_os_string()),
        }
    }
    if parts.first().map(|p| p == "private").unwrap_or(false) {
        parts.remove(0);
    }
    let mut out = PathBuf::new();
    if absolute {
        out.push("/");
    }
    for p in parts {
        out.push(p);
    }
    out
}

fn expand_user(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();
    if path_str == "~" {
        return dirs_home();
    }
    if let Some(rest) = path_str.strip_prefix("~/") {
        return dirs_home().join(rest);
    }
    path.to_path_buf()
}

pub fn expand_user_path(path: &Path) -> PathBuf {
    let expanded = expand_user(path);
    if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(expanded)
    }
}

pub fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn is_under_scan_root(path: &Path, scan_root: &Path) -> bool {
    let real = normalize_path(path);
    let root = normalize_path(scan_root);
    is_path_under(&real, &root)
}

/// Lexical containment: is `path` equal to or under `root`, comparing path
/// components directly with **no** filesystem access?
///
/// Both arguments must already be absolute (and normally canonical) for the
/// answer to be meaningful. Unlike [`is_under_scan_root`], this performs no
/// `realpath()` syscalls, so it is safe to call on the per-directory scan hot
/// path where the child path is a descendant of a canonical root by
/// construction.
pub fn is_under_root_lexical(path: &Path, root: &Path) -> bool {
    is_path_under(path, root)
}

fn is_path_under(path: &Path, root: &Path) -> bool {
    let path_components: Vec<_> = path.components().collect();
    let root_components: Vec<_> = root.components().collect();
    if root_components.is_empty() {
        return true;
    }
    if path_components.len() < root_components.len() {
        return false;
    }
    for (i, rc) in root_components.iter().enumerate() {
        if path_components.get(i) != Some(rc) {
            return false;
        }
    }
    true
}

pub fn is_ancestor(ancestor: &Path, descendant: &Path) -> bool {
    let a = normalize_path(ancestor);
    let d = normalize_path(descendant);
    a != d && is_path_under(&d, &a)
}

pub fn is_delete_protected(path: &Path, scan_root: &Path) -> (bool, String) {
    let real = normalize_path(path);
    let home = dirs_home();
    let root = PathBuf::from("/");
    let scan = normalize_path(scan_root);

    if real == root {
        return (true, "Cannot delete filesystem root".to_string());
    }
    if real == home {
        return (true, "Cannot delete home directory".to_string());
    }
    if real == scan {
        return (true, "Cannot delete the scan root".to_string());
    }
    if is_ancestor(&real, &scan) {
        return (
            true,
            "Cannot delete an ancestor of the scan root".to_string(),
        );
    }

    if !is_under_scan_root(&real, &scan) {
        for prefix in DELETE_DENIED_PREFIXES {
            let p = normalize_path(Path::new(prefix));
            if real == p || is_path_under(&real, &p) {
                return (
                    true,
                    format!("Cannot delete protected system path: {prefix}"),
                );
            }
        }
        return (true, "Path is outside the scan root".to_string());
    }

    (false, String::new())
}

pub fn safe_delete_target(
    path: &Path,
    scan_root: &Path,
    expect_dir: bool,
) -> (PathBuf, Option<String>) {
    if !path.exists() && path.symlink_metadata().is_err() {
        return (
            path.to_path_buf(),
            Some("Path no longer exists".to_string()),
        );
    }

    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => return (path.to_path_buf(), Some(e.to_string())),
    };

    if meta.file_type().is_symlink() {
        return (
            path.to_path_buf(),
            Some("Cannot delete symlinks (refuse to follow)".to_string()),
        );
    }

    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let (protected, reason) = is_delete_protected(&real, scan_root);
    if protected {
        return (real, Some(reason));
    }

    if expect_dir && !meta.is_dir() {
        return (
            real,
            Some("Expected directory but path is no longer a directory".to_string()),
        );
    }
    if !expect_dir && meta.is_dir() {
        return (
            real,
            Some("Expected file but path is no longer a file".to_string()),
        );
    }

    (real, None)
}

/// Strip /private prefix for display comparisons (macOS).
pub fn strip_private_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("/private") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ancestor_self_not_ancestor() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("dir");
        std::fs::create_dir(&path).unwrap();
        assert!(!is_ancestor(&path, &path));
    }
}
