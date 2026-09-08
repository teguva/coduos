use std::path::{Component, Path, PathBuf};

use crate::error::ApiError;

pub fn resolve_in_root(root: &Path, rel: &str) -> Result<PathBuf, ApiError> {
    if rel.contains('\0') {
        return Err(ApiError::BadRequest("invalid path".into()));
    }
    let root = root
        .canonicalize()
        .map_err(|_| ApiError::BadRequest(format!("root not found: {}", root.display())))?;

    let mut joined = root.clone();
    let rel_path = Path::new(rel);
    for comp in rel_path.components() {
        match comp {
            Component::Normal(c) => joined.push(c),
            Component::CurDir => {}
            Component::ParentDir => {
                if !joined.pop() {
                    return Err(ApiError::Forbidden);
                }
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(ApiError::BadRequest("absolute paths are not allowed".into()));
            }
        }
    }

    if !joined.starts_with(&root) {
        return Err(ApiError::Forbidden);
    }

    let resolved = if joined.exists() {
        let canonical = joined
            .canonicalize()
            .map_err(|_| ApiError::BadRequest("cannot resolve path".into()))?;
        if !canonical.starts_with(&root) {
            return Err(ApiError::Forbidden);
        }
        canonical
    } else {
        let parent = joined
            .parent()
            .ok_or_else(|| ApiError::BadRequest("invalid path".into()))?;
        if !parent.exists() {
            return Err(ApiError::NotFound);
        }
        let parent = parent
            .canonicalize()
            .map_err(|_| ApiError::BadRequest("cannot resolve parent".into()))?;
        if !parent.starts_with(&root) {
            return Err(ApiError::Forbidden);
        }
        parent.join(joined.file_name().ok_or_else(|| {
            ApiError::BadRequest("invalid path".into())
        })?)
    };

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn jail_blocks_escape() {
        let dir = std::env::temp_dir().join(format!("coduos-jail-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let err = resolve_in_root(&dir, "../etc/passwd").unwrap_err();
        assert!(matches!(err, ApiError::Forbidden));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn jail_allows_nested() {
        let dir = std::env::temp_dir().join(format!("coduos-jail-ok-{}", std::process::id()));
        fs::create_dir_all(dir.join("a/b")).unwrap();
        let got = resolve_in_root(&dir, "a/b").unwrap();
        assert_eq!(got, dir.join("a/b").canonicalize().unwrap());
        fs::remove_dir_all(&dir).ok();
    }
}
