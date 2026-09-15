use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    // Caches each parent directory's canonicalize() result so a directory
    // with many files pays that syscall once, not once per file.
    let mut canon_parent_cache = HashMap::new();

    for path in paths {
        let mut builder = WalkBuilder::new(path);
        builder
            .hidden(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true);

        let overrides_built = if exclude_patterns.is_empty() {
            None
        } else {
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
            builder.overrides(built.clone());
            Some(built)
        };

        for entry in builder.build() {
            match entry {
                Ok(entry) => {
                    if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                        let entry_path = entry.into_path();
                        if seen.insert(dedup_key(&entry_path, &mut canon_parent_cache)) {
                            all_files.push(Ok(entry_path));
                        }
                    }
                }
                Err(e) => {
                    all_files.push(Err(io::Error::other(e.to_string())));
                }
            }
        }

        // issue #85: a file already tracked by git stays tracked even after
        // it's added to .gitignore (git itself ignores .gitignore for paths
        // it already tracks) — but the walk above honors .gitignore
        // unconditionally, so such a file (and any secret in it) was
        // silently skipped. Re-add tracked files under this path that the
        // walk excluded, still honoring explicit --exclude/config excludes
        // (those are a deliberate opt-out, unlike an incidental .gitignore
        // match).
        if Path::new(path).is_dir() {
            for tracked in git_tracked_files_under(Path::new(path)) {
                if overrides_built
                    .as_ref()
                    .is_some_and(|o| o.matched(&tracked, false).is_ignore())
                {
                    continue;
                }
                if fs::metadata(&tracked).map(|m| m.is_file()).unwrap_or(false)
                    && seen.insert(dedup_key(&tracked, &mut canon_parent_cache))
                {
                    all_files.push(Ok(tracked));
                }
            }
        }
    }

    Ok(all_files.into_iter())
}

/// Lists files tracked by git under `root`, as absolute paths. `git ls-files`
/// ignores .gitignore entirely for already-tracked paths, so this surfaces
/// exactly the set the ignore-respecting walk above may have hidden.
/// Returns an empty vec (not an error) when `root` isn't inside a git
/// work tree, or `git` isn't installed — this is a best-effort supplement to
/// the primary walk, not something a missing git should fail the scan over.
fn git_tracked_files_under(root: &Path) -> Vec<PathBuf> {
    let output = match Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    output
        .stdout
        .split(|&b| b == 0)
        .filter(|chunk| !chunk.is_empty())
        .map(|chunk| root.join(String::from_utf8_lossy(chunk).as_ref()))
        .collect()
}

/// Dedup key for a walked file: the canonicalized parent directory joined
/// with the entry's own (non-canonicalized) file name, so the final path
/// component is never resolved through a symlink. Falls back to the literal
/// path when the parent can't be canonicalized (e.g. it vanished mid-walk) —
/// the worst case is then a duplicate entry (pre-fix behavior), never a
/// dropped file, so failing this way is safe.
///
/// `cache` memoizes each literal parent's canonicalize() result (`None` on
/// failure) so a directory with many files pays that syscall once rather
/// than once per file.
///
/// Known limitation: the file-name component is compared literally, so on a
/// case-insensitive filesystem (default macOS/Windows) two args differing
/// only in case (`A.rs` vs `a.rs`) that name the same file are not deduped.
fn dedup_key(path: &Path, cache: &mut HashMap<PathBuf, Option<PathBuf>>) -> PathBuf {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let canon_parent = cache
        .entry(parent.to_path_buf())
        .or_insert_with(|| parent.canonicalize().ok())
        .clone();
    match canon_parent {
        Some(canon_parent) => match path.file_name() {
            Some(name) => canon_parent.join(name),
            None => canon_parent,
        },
        None => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_dedup_key_falls_back_to_literal_path_when_parent_missing() {
        let mut cache = HashMap::new();
        let path = Path::new("/definitely/nonexistent/dir/f.txt");
        assert_eq!(dedup_key(path, &mut cache), path.to_path_buf());
    }

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

    #[test]
    fn test_issue_85() {
        // A file that's already tracked by git stays tracked even after
        // .gitignore is updated to match it (git only consults .gitignore
        // for previously-untracked paths) — so the walk must still surface
        // it, not silently drop it (and any secret it contains) from
        // scanning.
        let dir = TempDir::new().unwrap();
        assert!(Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(dir.path())
            .status()
            .unwrap()
            .success());

        let tracked = dir.path().join("tracked.env");
        fs::write(&tracked, "SECRET=hunter2\n").unwrap();
        assert!(Command::new("git")
            .args(["-C"])
            .arg(dir.path())
            .args(["add", "tracked.env"])
            .status()
            .unwrap()
            .success());

        // Now ignore it — git keeps tracking it regardless.
        fs::write(dir.path().join(".gitignore"), "tracked.env\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[])
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(
            files.iter().any(|f| f.ends_with("tracked.env")),
            "a tracked-but-now-gitignored file must still be walked: {files:?}"
        );
    }

    #[test]
    fn test_issue_85_untracked_gitignored_file_still_excluded() {
        // A file that was never tracked and matches .gitignore must remain
        // excluded — the fix only rescues already-tracked files.
        let dir = TempDir::new().unwrap();
        assert!(Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.path().join("ignored.txt"), "never tracked").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[])
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(files
            .iter()
            .all(|f| !f.to_string_lossy().contains("ignored.txt")));
    }

    #[test]
    fn test_issue_85_explicit_exclude_still_wins_over_tracked_file() {
        // Explicit --exclude/config excludes are a deliberate opt-out, unlike
        // an incidental .gitignore match, so they must still suppress an
        // already-tracked file.
        let dir = TempDir::new().unwrap();
        assert!(Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        let tracked = dir.path().join("tracked.env");
        fs::write(&tracked, "SECRET=hunter2\n").unwrap();
        assert!(Command::new("git")
            .args(["-C"])
            .arg(dir.path())
            .args(["add", "tracked.env"])
            .status()
            .unwrap()
            .success());
        fs::write(dir.path().join(".gitignore"), "tracked.env\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let exclude = vec!["tracked.env".to_string()];
        let files: Vec<_> = walk_paths(&paths, &exclude)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(files.iter().all(|f| !f.ends_with("tracked.env")));
    }
}
