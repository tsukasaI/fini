use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
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
    include_hidden: bool,
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
        // A root path argument that is itself a symlink to a directory is
        // followed by WalkBuilder before any per-entry policy (hidden files,
        // never-follow-symlink) ever runs, so its target's contents would be
        // walked and, in fix mode, rewritten, even though that target can be
        // entirely outside the tree the user meant to scan (issue #91). A
        // symlink-to-*file* root is unaffected: `ignore` follows a root that
        // resolves to a file, so it's yielded as a regular file entry and
        // process_file's own symlink_metadata check reports it as a skipped
        // symlink, same as any in-tree one.
        //
        // A trailing separator (or `/.`) makes lstat resolve through the
        // symlink - POSIX strips it before the syscall - so `path` itself
        // isn't a reliable probe; normalize via Components first (this
        // collapses "link/", "link//" and "link/." to "link", while leaving
        // a leading "./" alone). The walk and the error message below still
        // use the as-typed `path`, so reported entries keep their original
        // shape. Out of scope here: a root that *traverses* a symlink
        // component ("link/sub", "link/..") rather than naming one directly
        // - Components doesn't collapse `..`, and this check only guards
        // the root argument itself.
        let probe = Path::new(path).components().as_path();
        let is_symlinked_dir_root = fs::symlink_metadata(probe)
            .map(|m| m.is_symlink())
            .unwrap_or(false)
            && fs::metadata(probe).map(|m| m.is_dir()).unwrap_or(false);
        if is_symlinked_dir_root {
            all_files.push(Err(io::Error::other(format!(
                "{path}: refusing to walk a symlinked directory root (pass its target directly if that's intended)"
            ))));
            continue;
        }

        let mut builder = WalkBuilder::new(path);
        builder
            // Hidden files (dotfiles) are where secrets land most often
            // (.env, .github/workflows/*.yml) - --hidden opts into scanning
            // them too (issue #101). .git/ stays excluded either way: it's
            // not something users want scanned regardless of that flag.
            // git_ignore/git_global/git_exclude below still apply on top of
            // --hidden: inside a real git repo, a user's own .gitignore or
            // global excludes (many developers' global gitignore lists
            // ".env" itself) can still hide a file --hidden would otherwise
            // include - --hidden widens fini's own default, not the user's
            // explicit ignore rules.
            .hidden(!include_hidden)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true);

        let mut patterns: Vec<&str> = exclude_patterns.iter().map(String::as_str).collect();
        if include_hidden {
            // No trailing slash: a worktree or submodule checkout has
            // ".git" as a *file* (containing "gitdir: ..."), not a
            // directory, and a directory-only pattern wouldn't match it.
            patterns.push(".git");
        }

        let overrides_built = if patterns.is_empty() {
            None
        } else {
            let mut overrides = OverrideBuilder::new(path);
            for pattern in &patterns {
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
        // walk excluded *only because of .gitignore*. This replicates the
        // primary walk's other filtering (hidden files unless --hidden,
        // --exclude/config overrides, never following a symlink) as closely
        // as a second, non-walking pass reasonably can; known carve-out: a
        // `.ignore` file (as opposed to `.gitignore`) is not consulted here,
        // so a tracked file it excludes is still rescued.
        let root = Path::new(path);
        if root.is_dir() {
            for rel in git_tracked_files_relative(root) {
                // `git ls-files` prints index entries verbatim — git
                // validates a path when it's added, not when the index is
                // read back, so a hand-crafted .git/index (a shipped tarball,
                // not a clone) could contain an absolute path or a `..`
                // component. root.join() on such a path would discard root
                // entirely, so reject anything but plain path components
                // before it's ever joined.
                if !is_plain_relative_path(&rel) {
                    continue;
                }

                // Mirrors builder.hidden(!include_hidden): skip any path
                // with a dotfile/dotdir component, unless --hidden opted
                // into scanning those too (issue #101) - a tracked-and-
                // gitignored secret under a dotfile/dotdir (.env,
                // .github/workflows/*.yml) is exactly the #101 scenario
                // this rescue must not silently exempt.
                if !include_hidden
                    && rel
                        .components()
                        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
                {
                    continue;
                }

                let abs = root.join(&rel);

                // Cheap dedup check first: the vast majority of tracked
                // files were already found by the primary walk, so skip the
                // lstat-per-ancestor work below for those.
                let key = dedup_key(&abs, &mut canon_parent_cache);
                if seen.contains(&key) {
                    continue;
                }

                // A directory-level exclude (e.g. "vendor/") prunes the
                // directory in the primary walk before any file under it is
                // ever tested; a symlinked intermediate directory is never
                // descended into by the primary walk either (issue #35). Both
                // are properties of the path itself, so check every ancestor
                // between the file and root — not just the file path — for
                // either.
                let ancestor_blocked =
                    abs.ancestors().skip(1).take_while(|a| *a != root).any(|a| {
                        fs::symlink_metadata(a)
                            .map(|m| m.is_symlink())
                            .unwrap_or(true)
                            || overrides_built
                                .as_ref()
                                .is_some_and(|o| o.matched(a, true).is_ignore())
                    });
                if ancestor_blocked {
                    continue;
                }
                if overrides_built
                    .as_ref()
                    .is_some_and(|o| o.matched(&abs, false).is_ignore())
                {
                    continue;
                }

                // symlink_metadata (not metadata): the primary walk never
                // follows symlinks into content outside the tree (issue
                // #35), so this rescue must not either.
                let is_file = fs::symlink_metadata(&abs)
                    .map(|m| m.is_file())
                    .unwrap_or(false);
                if is_file {
                    seen.insert(key);
                    all_files.push(Ok(abs));
                }
            }
        }
    }

    Ok(all_files.into_iter())
}

/// Lists files tracked by git under `root`, as paths relative to `root`.
/// `git ls-files` ignores .gitignore entirely for already-tracked paths, so
/// this surfaces exactly the set the ignore-respecting walk above may have
/// hidden for that reason. Doesn't recurse into submodules.
///
/// Silent no-op (empty vec) both when `root` isn't inside a git work tree
/// (expected; nothing to rescue) and when git itself fails there (missing
/// binary, permission, `safe.directory`) — the caller is responsible for
/// telling those two cases apart and warning on the latter, since this is a
/// security-relevant fallback that must fail loud, not silently disable
/// itself (see security.md).
fn git_tracked_files_relative(root: &Path) -> Vec<PathBuf> {
    let output = Command::new("git")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        // A repo-local .git/config could otherwise run an arbitrary
        // core.fsmonitor hook, or ls-files could block on an index lock;
        // this call is a read-only supplement to the walk, not something
        // that should ever shell out further or wait on a lock.
        .args(["-c", "core.fsmonitor=", "-c", "core.useBuiltinFSMonitor="])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            if has_git_ancestor(root) {
                eprintln!(
                    "Warning: could not run git ({e}); tracked-but-gitignored files under {} were not rescanned",
                    crate::output::safe_path_display(root)
                );
            }
            return Vec::new();
        }
    };
    if !output.status.success() {
        if has_git_ancestor(root) {
            eprintln!(
                "Warning: `git ls-files` failed in {}: {} (tracked-but-gitignored files there were not rescanned)",
                crate::output::safe_path_display(root),
                crate::output::escape_line_breaks(String::from_utf8_lossy(&output.stderr).trim())
            );
        }
        return Vec::new();
    }

    output
        .stdout
        .split(|&b| b == 0)
        .filter(|chunk| !chunk.is_empty())
        .map(bytes_to_path)
        .collect()
}

/// True if every component of `rel` is a plain path segment — no root
/// prefix, no `.`/`..`. `git ls-files` output isn't guaranteed to satisfy
/// this (git validates a path on `add`, not on reading the index back), and
/// `root.join(rel)` on a rejected path would silently discard `root`.
fn is_plain_relative_path(rel: &Path) -> bool {
    rel.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Filesystem-only check for a `.git` entry (directory, or the file a
/// worktree/submodule uses) in `root` or any ancestor — used to decide
/// whether a git failure deserves a warning, without depending on git
/// itself being available to answer that question.
fn has_git_ancestor(root: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    root.ancestors().any(|dir| {
        // An empty `.git` directory (as a test fixture might create without
        // actually running `git init`) isn't a real repo either; check for
        // HEAD (present for both a normal repo and a worktree's `.git` file
        // pointing elsewhere) rather than bare existence.
        let git_entry = dir.join(".git");
        git_entry.join("HEAD").exists() || git_entry.is_file()
    })
}

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
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
        let files: Vec<_> = walk_paths(&paths, &[], false).unwrap().collect();

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
        let files: Vec<_> = walk_paths(&paths, &[], false)
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
        let files: Vec<_> = walk_paths(&paths, &[], false)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(files.len(), 1);
        assert!(files[0].to_string_lossy().contains("visible.txt"));
    }

    #[test]
    fn test_issue_101() {
        // --hidden (include_hidden=true) must include dotfiles like .env,
        // where secrets land most often - the very thing hidden-by-default
        // scanning was silently excluding from a `--check .` CI gate.
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("visible.txt"), "visible").unwrap();
        fs::write(dir.path().join(".env"), "SECRET=1").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[], true)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(files.len(), 2, "{files:?}");
        assert!(files.iter().any(|f| f.to_string_lossy().contains(".env")));
        assert!(files
            .iter()
            .any(|f| f.to_string_lossy().contains("visible.txt")));
    }

    #[test]
    fn test_issue_101_git_dir_still_excluded_with_hidden() {
        // .git/ must stay excluded even with --hidden - it's not something
        // users want scanned, regardless of that flag.
        //
        // A real .git directory makes the walker's git_global(true) setting
        // consult the machine's actual global gitignore - many developers'
        // global gitignore excludes ".env" itself (a common convention), so
        // that filename would make this test's outcome depend on the
        // machine it runs on. Use an obscure name instead.
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".git/config"), "git config").unwrap();
        fs::write(dir.path().join(".fini_test_hidden_marker"), "SECRET=1").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[], true)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(files
            .iter()
            .any(|f| f.to_string_lossy().contains(".fini_test_hidden_marker")));
        assert!(
            !files.iter().any(|f| f.to_string_lossy().contains(".git")),
            "{files:?}"
        );
    }

    #[test]
    fn test_issue_101_gitignore_rescue_reaches_hidden_files_with_hidden() {
        // The issue #85 tracked-but-gitignored rescue must not silently
        // exempt dotfiles/dotdirs from --hidden: a secret committed under
        // .env or .github/workflows/*.yml, then added to .gitignore, is
        // exactly the #101 scenario this rescue must not fall through on.
        // Uses an obscure filename (not .env) - a real .git directory makes
        // git_global(true) consult the machine's actual global gitignore,
        // which commonly lists .env.
        let dir = TempDir::new().unwrap();
        assert!(Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(dir.path())
            .status()
            .unwrap()
            .success());

        let tracked = dir.path().join(".fini_test_hidden_tracked");
        fs::write(&tracked, "SECRET=hunter2\n").unwrap();
        assert!(Command::new("git")
            .args(["-C"])
            .arg(dir.path())
            .args(["add", "-f", ".fini_test_hidden_tracked"])
            .status()
            .unwrap()
            .success());
        fs::write(dir.path().join(".gitignore"), ".fini_test_hidden_tracked\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];

        let without_hidden: Vec<_> = walk_paths(&paths, &[], false)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(
            !without_hidden
                .iter()
                .any(|f| f.to_string_lossy().contains(".fini_test_hidden_tracked")),
            "without --hidden, a hidden tracked file stays hidden: {without_hidden:?}"
        );

        let with_hidden: Vec<_> = walk_paths(&paths, &[], true)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(
            with_hidden
                .iter()
                .any(|f| f.to_string_lossy().contains(".fini_test_hidden_tracked")),
            "--hidden must let the gitignore rescue reach a tracked hidden file too: {with_hidden:?}"
        );
    }

    #[test]
    fn test_skip_git_directory() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("file.txt"), "content").unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".git/config"), "git config").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[], false)
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
        let files: Vec<_> = walk_paths(&paths, &[], false)
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
        let files: Vec<_> = walk_paths(&paths, &exclude, false)
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
        let files: Vec<_> = walk_paths(&paths, &exclude, false)
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
        let files: Vec<_> = walk_paths(&paths, &[], false)
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
        let files: Vec<_> = walk_paths(&paths, &[], false)
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
        let files: Vec<_> = walk_paths(&paths, &[], false)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(
            files.len(),
            2,
            "a symlink root and its target are distinct paths, not duplicates: {files:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_issue_91() {
        // A root path argument that is itself a symlink to a directory must
        // not be followed: WalkBuilder dereferences the root's own type
        // directly (unlike a nested symlink, which the walk already skips),
        // so an unguarded symlink root would have its target's files walked
        // and, in fix mode, rewritten - even though that target can be
        // entirely outside the tree the user meant to scan.
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("secret.txt"), "hello").unwrap();

        let container = TempDir::new().unwrap();
        let link = container.path().join("link");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();

        let paths = vec![link.to_string_lossy().to_string()];
        let results: Vec<_> = walk_paths(&paths, &[], false).unwrap().collect();

        assert!(
            results.iter().all(|r| r.is_err()),
            "a symlinked directory root must be refused, not walked: {results:?}"
        );
        assert!(
            results
                .iter()
                .any(|r| matches!(r, Err(e) if e.to_string().contains("symlink"))),
            "the refusal should say why: {results:?}"
        );

        // A trailing separator (or "/.") makes lstat resolve through the
        // symlink instead of reporting it - the fix must probe a normalized
        // path, not the literal argument string, or this form bypasses the
        // refusal entirely.
        for suffix in ["/", "//", "/."] {
            let path_with_suffix = format!("{}{suffix}", link.to_string_lossy());
            let paths = vec![path_with_suffix.clone()];
            let results: Vec<_> = walk_paths(&paths, &[], false).unwrap().collect();
            assert!(
                results.iter().all(|r| r.is_err()),
                "{path_with_suffix:?} must also be refused, not walked: {results:?}"
            );
            assert!(
                results
                    .iter()
                    .any(|r| matches!(r, Err(e) if e.to_string().contains("symlink"))),
                "{path_with_suffix:?}: refusal should say why, not just any walk error: {results:?}"
            );
        }
    }

    #[test]
    fn test_invalid_exclude_pattern_returns_err() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let exclude = vec!["[invalid".to_string()];
        let result = walk_paths(&paths, &exclude, false);

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
        let entries: Vec<_> = walk_paths(&paths, &[], false).unwrap().collect();

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
            .args(["add", "-f", "tracked.env"])
            .status()
            .unwrap()
            .success());

        // Now ignore it — git keeps tracking it regardless.
        fs::write(dir.path().join(".gitignore"), "tracked.env\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[], false)
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
        let files: Vec<_> = walk_paths(&paths, &[], false)
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
            .args(["add", "-f", "tracked.env"])
            .status()
            .unwrap()
            .success());
        fs::write(dir.path().join(".gitignore"), "tracked.env\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let exclude = vec!["tracked.env".to_string()];
        let files: Vec<_> = walk_paths(&paths, &exclude, false)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(files.iter().all(|f| !f.ends_with("tracked.env")));
    }

    #[test]
    fn test_issue_85_directory_exclude_still_wins_over_tracked_file() {
        // A directory-form exclude ("vendor/") must still prune every
        // tracked file under that directory, not just a tracked file whose
        // own path happens to match the pattern.
        let dir = TempDir::new().unwrap();
        assert!(Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        fs::create_dir(dir.path().join("vendor")).unwrap();
        let tracked = dir.path().join("vendor/lib.rs");
        fs::write(&tracked, "SECRET=hunter2\n").unwrap();
        assert!(Command::new("git")
            .args(["-C"])
            .arg(dir.path())
            .args(["add", "-f", "vendor/lib.rs"])
            .status()
            .unwrap()
            .success());
        fs::write(dir.path().join(".gitignore"), "vendor/\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let exclude = vec!["vendor/".to_string()];
        let files: Vec<_> = walk_paths(&paths, &exclude, false)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(
            files
                .iter()
                .all(|f| !f.to_string_lossy().contains("vendor")),
            "a directory-form exclude must still prune a tracked file under it: {files:?}"
        );
    }

    #[test]
    fn test_issue_85_hidden_tracked_file_stays_excluded() {
        // The walk hides dotfiles by default (README: "Hidden files (.foo)"
        // are skipped) — the gitignore rescue must not override that
        // unrelated rule.
        let dir = TempDir::new().unwrap();
        assert!(Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        let tracked = dir.path().join(".env");
        fs::write(&tracked, "SECRET=hunter2\n").unwrap();
        // -f: the user's global gitignore commonly excludes .env, which
        // would otherwise make `git add` refuse it here regardless of this
        // repo's own (not-yet-written) .gitignore.
        assert!(Command::new("git")
            .args(["-C"])
            .arg(dir.path())
            .args(["add", "-f", ".env"])
            .status()
            .unwrap()
            .success());
        fs::write(dir.path().join(".gitignore"), ".env\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[], false)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(
            files.iter().all(|f| !f.to_string_lossy().contains(".env")),
            "a tracked dotfile must stay hidden like every other dotfile: {files:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_issue_85_tracked_symlink_not_followed() {
        // The primary walk never follows symlinks (issue #35), so the
        // gitignore rescue must use symlink_metadata, not metadata, or it
        // would stat through a tracked symlink to content outside the tree.
        let dir = TempDir::new().unwrap();
        assert!(Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        let target = dir.path().join("outside_target.txt");
        fs::write(&target, "hello").unwrap();
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(Command::new("git")
            .args(["-C"])
            .arg(dir.path())
            .args(["add", "link.txt"])
            .status()
            .unwrap()
            .success());
        fs::write(dir.path().join(".gitignore"), "link.txt\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[], false)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(
            files.iter().all(|f| !f.ends_with("link.txt")),
            "a tracked symlink must not be rescued via a metadata() call that follows it: {files:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_issue_85_symlinked_intermediate_dir_not_descended_into() {
        // A tracked file's *parent* directory being a symlink is the same
        // "never follow a symlink" boundary as a symlinked leaf file, and
        // must be checked for every ancestor between the file and the walk
        // root, not just the leaf.
        let outer = TempDir::new().unwrap();
        let outside_target = outer.path().join("outside_target");
        fs::create_dir(&outside_target).unwrap();
        fs::write(outside_target.join("tracked.txt"), "hello").unwrap();

        let dir = TempDir::new().unwrap();
        assert!(Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(dir.path())
            .status()
            .unwrap()
            .success());
        // Track "sub/tracked.txt" as a real directory first, so the git
        // index still references that path once "sub" is replaced with a
        // symlink below — this is how a tracked path ends up "inside" what
        // is, on disk, a symlinked directory.
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("tracked.txt"), "hello").unwrap();
        assert!(Command::new("git")
            .args(["-C"])
            .arg(dir.path())
            .args(["add", "sub/tracked.txt"])
            .status()
            .unwrap()
            .success());
        fs::remove_dir_all(&sub).unwrap();
        std::os::unix::fs::symlink(&outside_target, &sub).unwrap();
        fs::write(dir.path().join(".gitignore"), "sub/\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[], false)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(
            files
                .iter()
                .all(|f| !f.to_string_lossy().contains("tracked.txt")),
            "must never descend through a symlinked intermediate directory: {files:?}"
        );
    }

    #[test]
    fn test_issue_85_rejects_absolute_or_dotdot_tracked_paths() {
        // `git ls-files` prints whatever's in the index verbatim, and git
        // only validates a path when it's added, not when the index is read
        // back — a hand-crafted .git/index (a shipped tarball, not a real
        // clone) could contain an absolute path or a `..` component.
        // root.join() on such a path would discard root entirely, letting
        // the rescue write outside the walked tree.
        assert!(is_plain_relative_path(Path::new("src/main.rs")));
        assert!(!is_plain_relative_path(Path::new("/etc/passwd")));
        assert!(!is_plain_relative_path(Path::new("../outside.txt")));
        assert!(!is_plain_relative_path(Path::new("a/../../outside.txt")));
        assert!(!is_plain_relative_path(Path::new("./a.txt")));
    }
}
