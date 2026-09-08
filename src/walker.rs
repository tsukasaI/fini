use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

/// Walk paths and yield file paths, respecting gitignore and custom exclude patterns.
///
/// Exclude patterns use gitignore-style glob syntax (e.g., "vendor/", "*.min.js").
///
/// Returns `Err` immediately for fatal configuration errors (e.g. an invalid exclude
/// pattern) so callers can fail closed instead of silently walking with no excludes.
/// Once walking starts, per-entry errors (e.g. permission-denied on a subdirectory)
/// are yielded as `Err` items in the iterator rather than aborting the whole walk.
pub fn walk_paths(
    paths: &[String],
    exclude_patterns: &[String],
) -> io::Result<impl Iterator<Item = io::Result<PathBuf>>> {
    let mut all_files = vec![];
    // Overlapping path arguments (e.g. `fini src src/a.rs`) can walk the same
    // file more than once; dedup so it's only ever processed once (issue
    // #81). See `dedup_key` for the key used.
    let mut seen = HashSet::new();

    for path in paths {
        let mut builder = WalkBuilder::new(path);
        builder
            .hidden(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true);

        if !exclude_patterns.is_empty() {
            let mut overrides = OverrideBuilder::new(path);
            for pattern in exclude_patterns {
                // OverrideBuilder uses inverted ! semantics:
                // !pattern = exclude, pattern = whitelist
                overrides.add(&format!("!{pattern}")).map_err(|e| {
                    io::Error::other(format!("invalid exclude pattern '{pattern}': {e}"))
                })?;
            }
            let built = overrides
                .build()
                .map_err(|e| io::Error::other(format!("failed to build exclude patterns: {e}")))?;
            builder.overrides(built);
        }

        for entry in builder.build() {
            match entry {
                Ok(entry) => {
                    if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                        let entry_path = entry.into_path();
                        if seen.insert(dedup_key(&entry_path)) {
                            all_files.push(Ok(entry_path));
                        }
                    }
                }
                Err(e) => {
                    all_files.push(Err(io::Error::other(e.to_string())));
                }
            }
        }
    }

    Ok(all_files.into_iter())
}

/// Dedup key for a walked file: the canonicalized parent directory joined
/// with the entry's own (non-canonicalized) file name, so the final path
/// component is never resolved through a symlink. Falls back to the literal
/// path when the parent can't be canonicalized (e.g. it vanished mid-walk).
fn dedup_key(path: &Path) -> PathBuf {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    match parent.canonicalize() {
        Ok(canon_parent) => match path.file_name() {
            Some(name) => canon_parent.join(name),
            None => canon_parent,
        },
        Err(_) => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_walk_single_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello").unwrap();

        let paths = vec![file_path.to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[]).unwrap().collect();

        assert_eq!(files.len(), 1);
        assert!(files[0].is_ok());
    }

    #[test]
    fn test_recursive_directory_traversal() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("file1.txt"), "content1").unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();
        fs::write(dir.path().join("subdir/file2.txt"), "content2").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[])
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_skip_hidden_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("visible.txt"), "visible").unwrap();
        fs::write(dir.path().join(".hidden"), "hidden").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[])
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(files.len(), 1);
        assert!(files[0].to_string_lossy().contains("visible.txt"));
    }

    #[test]
    fn test_skip_git_directory() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("file.txt"), "content").unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".git/config"), "git config").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[])
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(files.len(), 1);
        assert!(!files[0].to_string_lossy().contains(".git"));
    }

    #[test]
    fn test_respect_gitignore() {
        let dir = TempDir::new().unwrap();

        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.path().join("kept.txt"), "kept").unwrap();
        fs::write(dir.path().join("ignored.txt"), "ignored").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[])
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(files
            .iter()
            .all(|f| !f.to_string_lossy().contains("ignored.txt")));
        assert!(files
            .iter()
            .any(|f| f.to_string_lossy().contains("kept.txt")));
    }

    #[test]
    fn test_exclude_patterns() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("style.min.js"), "minified").unwrap();
        fs::create_dir(dir.path().join("vendor")).unwrap();
        fs::write(dir.path().join("vendor/lib.js"), "vendor code").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let exclude = vec!["*.min.js".to_string(), "vendor/".to_string()];
        let files: Vec<_> = walk_paths(&paths, &exclude)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(files.len(), 1);
        assert!(files[0].to_string_lossy().contains("main.rs"));
    }

    #[test]
    fn test_exclude_specific_directory() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("app.js"), "app").unwrap();
        fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        fs::write(dir.path().join("node_modules/pkg/index.js"), "pkg").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let exclude = vec!["node_modules/".to_string()];
        let files: Vec<_> = walk_paths(&paths, &exclude)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(files.len(), 1);
        assert!(files[0].to_string_lossy().contains("app.js"));
    }

    #[test]
    fn test_overlapping_dir_and_file_args_dedup_to_one() {
        // Regression test for issue #81.
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("dupdir");
        fs::create_dir(&subdir).unwrap();
        let file_path = subdir.join("f.txt");
        fs::write(&file_path, "hello").unwrap();

        let paths = vec![
            subdir.to_string_lossy().to_string(),
            file_path.to_string_lossy().to_string(),
        ];
        let files: Vec<_> = walk_paths(&paths, &[])
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(
            files.len(),
            1,
            "duplicate path args must collapse: {files:?}"
        );
    }

    #[test]
    fn test_dotdot_path_and_plain_path_dedup_to_one() {
        // "dupdir" and "dupdir/../dupdir" name the same directory but aren't
        // `==` as `PathBuf`s (unlike an interior "/./", "/../" is not
        // normalized away by `Path`'s component comparison), so this only
        // passes if the dedup key actually canonicalizes.
        let dir = TempDir::new().unwrap();
        let subdir = dir.path().join("dupdir");
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("f.txt"), "hello").unwrap();

        let paths = vec![
            subdir.to_string_lossy().to_string(),
            format!("{}/../dupdir", subdir.to_string_lossy()),
        ];
        let files: Vec<_> = walk_paths(&paths, &[])
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(
            files.len(),
            1,
            "equivalent dir args must collapse: {files:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_and_target_are_not_deduped_together() {
        // A dedup key built from a full canonicalize() of the entry would
        // resolve link.txt to target.txt and silently drop one of them. The
        // dedup key must only canonicalize the parent directory, never the
        // final path component, so a symlink and its target remain distinct
        // walk results.
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("target.txt");
        let link = dir.path().join("link.txt");
        fs::write(&target, "hello").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        // Link listed first, matching the order a shell glob would produce.
        let paths = vec![
            link.to_string_lossy().to_string(),
            target.to_string_lossy().to_string(),
        ];
        let files: Vec<_> = walk_paths(&paths, &[])
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(
            files.len(),
            2,
            "a symlink root and its target are distinct paths, not duplicates: {files:?}"
        );
    }

    #[test]
    fn test_invalid_exclude_pattern_returns_err() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let exclude = vec!["[invalid".to_string()];
        let result = walk_paths(&paths, &exclude);

        let err = result.err().expect("invalid glob pattern should error");
        assert!(err.to_string().contains("invalid exclude pattern"));
    }

    #[cfg(unix)]
    #[test]
    fn test_per_entry_walk_error_does_not_abort_walk() {
        // Regression test for issue #32.
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("good.txt"), "content").unwrap();
        let blocked_dir = dir.path().join("blocked");
        fs::create_dir(&blocked_dir).unwrap();
        fs::write(blocked_dir.join("inner.txt"), "content").unwrap();

        let mut perms = fs::metadata(&blocked_dir).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&blocked_dir, perms.clone()).unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let entries: Vec<_> = walk_paths(&paths, &[]).unwrap().collect();

        // Restore permissions so TempDir cleanup can remove the directory.
        perms.set_mode(0o755);
        fs::set_permissions(&blocked_dir, perms).unwrap();

        assert!(
            entries
                .iter()
                .filter_map(|r| r.as_ref().ok())
                .any(|p| p.to_string_lossy().contains("good.txt")),
            "sibling files must still be walked after a per-entry error: {entries:?}"
        );
        assert!(
            entries.iter().any(|r| r.is_err()),
            "permission-denied subdirectory should surface as an Err entry"
        );
    }
}
