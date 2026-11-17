use ignore::overrides::{Override, OverrideBuilder};
use ignore::WalkBuilder;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// What `walk_paths` found for its arguments.
pub struct Walked {
    /// Files to process; a per-entry walk error stays in place.
    pub entries: Vec<io::Result<PathBuf>>,
    /// Arguments, as typed, that an exclude pattern skipped without
    /// walking them.
    pub excluded_arguments: Vec<PathBuf>,
}

/// Walk paths and return file paths, respecting gitignore and custom exclude patterns.
///
/// Exclude patterns use gitignore-style glob syntax (e.g., "vendor/", "*.min.js")
/// and are matched relative to the current directory for every argument
/// inside it; an argument outside it anchors them at itself (a file
/// argument, at its directory).
///
/// Returns `Err` immediately for fatal configuration errors (e.g. an invalid exclude
/// pattern) so callers can fail closed instead of silently walking with no excludes.
/// Once walking starts, per-entry errors (e.g. permission-denied on a subdirectory)
/// are returned as `Err` elements rather than aborting the whole walk.
pub fn walk_paths(
    paths: &[String],
    exclude_patterns: &[String],
    include_hidden: bool,
) -> io::Result<Walked> {
    let canonical_cwd = std::env::current_dir().and_then(fs::canonicalize).ok();
    walk_paths_from(
        canonical_cwd.as_deref(),
        paths,
        exclude_patterns,
        include_hidden,
    )
}

/// `walk_paths` with the directory that anchors exclude patterns passed in.
/// `None` (the cwd could not be resolved) anchors every pattern at the
/// argument it is applied under.
fn walk_paths_from(
    canonical_cwd: Option<&Path>,
    paths: &[String],
    exclude_patterns: &[String],
    include_hidden: bool,
) -> io::Result<Walked> {
    let mut all_files = vec![];
    let mut excluded_arguments = vec![];
    let mut seen_excluded = HashSet::new();
    // Overlapping path arguments (e.g. `fini src src/a.rs`) can walk the same
    // file more than once; dedup so it's only ever processed once (issue
    // #81). See `dedup_key` for the key used.
    let mut seen = HashSet::new();
    // Caches each parent directory's canonicalize() result so a directory
    // with many files pays that syscall once, not once per file.
    let mut canon_parent_cache = HashMap::new();

    let user_patterns: Vec<&str> = exclude_patterns.iter().map(String::as_str).collect();
    let argument_excludes = build_excludes(&user_patterns)?;
    let mut walk_patterns = user_patterns;
    if include_hidden {
        // No trailing slash: a worktree or submodule checkout has
        // ".git" as a *file* (containing "gitdir: ..."), not a
        // directory, and a directory-only pattern wouldn't match it.
        walk_patterns.push(".git");
    }
    let walk_excludes = build_excludes(&walk_patterns)?;

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

        let root = Path::new(path);
        let anchored_root = canonical_cwd
            .and_then(|cwd| cwd_relative(root, cwd, &mut canon_parent_cache))
            .unwrap_or_default();
        if is_excluded_argument(root, &anchored_root, argument_excludes.as_ref()) {
            // Not keyed on the typed text: `vendor`, `./vendor` and `vendor/`
            // are one argument given three times, as for files (issue #81).
            let location = canonical_location(root, &mut canon_parent_cache)
                .unwrap_or_else(|| root.to_path_buf());
            if seen_excluded.insert(location) {
                excluded_arguments.push(root.to_path_buf());
            }
            continue;
        }
        let excludes = AnchoredExcludes {
            matcher: walk_excludes.clone(),
            typed_root: root.to_path_buf(),
            anchored_root,
        };

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
            // These three only take effect inside a git repository:
            // WalkBuilder's `require_git` defaults to true, so a directory
            // with no `.git` ancestor (a `git archive` export, an extracted
            // tarball) has its .gitignore files ignored entirely - deliberate
            // (issue #86, issue #105).
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true);

        if excludes.matcher.is_some() {
            // Not `builder.overrides`: `ignore` matches an override against
            // the entry path as typed, which is only relative to the cwd for
            // some spellings of the argument.
            let filter = excludes.clone();
            builder.filter_entry(move |entry| {
                let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
                !filter.matches(entry.path(), is_dir)
            });
        }

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
        // regardless of tracked status, so such a file (and any secret in
        // it) was silently skipped. Re-add tracked files under this path
        // that the walk excluded *only because of .gitignore*. This replicates the
        // primary walk's other filtering (hidden files unless --hidden,
        // --exclude/config overrides, never following a symlink) as closely
        // as a second, non-walking pass reasonably can; known carve-out: a
        // `.ignore` file (as opposed to `.gitignore`) is not consulted here,
        // so a tracked file it excludes is still rescued.
        if root.is_dir() {
            rescue_tracked_files(
                root,
                include_hidden,
                &excludes,
                &mut seen,
                &mut canon_parent_cache,
                &mut all_files,
            );
        }
    }

    Ok(Walked {
        entries: all_files,
        excluded_arguments,
    })
}

fn build_excludes(patterns: &[&str]) -> io::Result<Option<Override>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    // "." disables `ignore`'s root-prefix stripping, so a pattern applies to
    // exactly the anchor-relative path `AnchoredExcludes` and
    // `is_excluded_argument` hand over.
    let mut overrides = OverrideBuilder::new(".");
    for pattern in patterns {
        // OverrideBuilder uses inverted ! semantics:
        // !pattern = exclude, pattern = whitelist
        overrides
            .add(&format!("!{pattern}"))
            .map_err(|e| io::Error::other(format!("invalid exclude pattern '{pattern}': {e}")))?;
    }
    overrides
        .build()
        .map(Some)
        .map_err(|e| io::Error::other(format!("failed to build exclude patterns: {e}")))
}

/// `argument` relative to `canonical_cwd`, or `None` when it is outside the
/// cwd or can't be resolved.
///
/// Not a lexical `strip_prefix` on the path as typed: `/tmp/x` and
/// `/private/tmp/x`, a symlinked checkout, and `vendor/../src` each name a
/// place the typed text doesn't show, and matching that text let a
/// directory above the project exclude a file (issue #273).
///
/// The final component stays unresolved (`dedup_key`): a symlink is matched
/// by where the link sits, not by where it points.
///
/// The typed path is the fallback only when resolving leads out of the cwd
/// (see `typed_below_cwd`).
fn cwd_relative(
    argument: &Path,
    canonical_cwd: &Path,
    canon_parent_cache: &mut HashMap<PathBuf, Option<PathBuf>>,
) -> Option<PathBuf> {
    canonical_location(argument, canon_parent_cache)?
        .strip_prefix(canonical_cwd)
        .ok()
        .map(Path::to_path_buf)
        .or_else(|| typed_below_cwd(argument, canonical_cwd))
}

/// Where `argument` is, with every component but the last resolved.
fn canonical_location(
    argument: &Path,
    canon_parent_cache: &mut HashMap<PathBuf, Option<PathBuf>>,
) -> Option<PathBuf> {
    if argument.file_name().is_some() {
        Some(dedup_key(argument, canon_parent_cache))
    } else {
        // Not `dedup_key`: for `.`, `..` or `src/..` it has no final
        // component to keep and would return the parent it resolved.
        fs::canonicalize(argument).ok()
    }
}

/// `argument` as typed, relative to the cwd, when its text alone places it
/// below the cwd: no `..`, and either relative or starting with
/// `canonical_cwd`.
///
/// For a path through a symlinked directory that leads out of the cwd
/// (`data -> /mnt/data`, then `data/x.csv`). A walk never descends such a
/// link, so an exclude written against the typed path (`data/`) is all that
/// keeps the file from being rewritten. Without `..`, no directory above
/// the cwd can take part in the match, which is what #273 was about.
fn typed_below_cwd(argument: &Path, canonical_cwd: &Path) -> Option<PathBuf> {
    let relative = if argument.is_absolute() {
        argument.strip_prefix(canonical_cwd).ok()?
    } else {
        argument
    };
    let mut below_cwd = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(name) => below_cwd.push(name),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(below_cwd)
}

/// True if a user exclude pattern matches an existing argument.
/// `anchored_root` is `cwd_relative`'s answer: for an argument below the
/// cwd, the argument and each directory between it and the cwd are tested.
///
/// The walk can't answer this: `ignore` yields a root argument without
/// consulting any filter, so a file passed by name (pre-commit, an editor
/// hook) would otherwise bypass `--exclude`/config excludes (issue #264).
fn is_excluded_argument(
    argument: &Path,
    anchored_root: &Path,
    excludes: Option<&Override>,
) -> bool {
    let Some(excludes) = excludes else {
        return false;
    };
    // Not skipped when missing: a path that doesn't exist must still reach
    // the walk and be reported as an error, excluded pattern or not.
    let Ok(metadata) = fs::metadata(argument) else {
        return false;
    };
    let mut is_dir = metadata.is_dir();
    if anchored_root.as_os_str().is_empty() {
        // The cwd itself, or an argument outside it. Not its directories:
        // nothing above it is part of the scan (issue #273). A file still
        // has its own name, which is what a walk of its directory would
        // match it by, so `*.snap` applies to it and `vendor/` cannot.
        return !is_dir
            && argument
                .file_name()
                .is_some_and(|name| excludes.matched(Path::new(name), false).is_ignore());
    }
    for path in anchored_root.ancestors() {
        if path.as_os_str().is_empty() {
            break;
        }
        if excludes.matched(path, is_dir).is_ignore() {
            return true;
        }
        is_dir = true;
    }
    false
}

/// Exclude patterns for the walk under one argument.
#[derive(Clone)]
struct AnchoredExcludes {
    matcher: Option<Override>,
    /// The argument as typed; every entry found under it starts with this.
    typed_root: PathBuf,
    /// The argument relative to the cwd, which anchors the patterns there.
    /// Empty for an argument outside the cwd: patterns are then anchored
    /// at the argument itself.
    anchored_root: PathBuf,
}

impl AnchoredExcludes {
    /// Whether a pattern matches `entry`, a path below the argument. The
    /// argument itself is `is_excluded_argument`'s to judge.
    fn matches(&self, entry: &Path, is_dir: bool) -> bool {
        let Some(matcher) = &self.matcher else {
            return false;
        };
        let Ok(below_root) = entry.strip_prefix(&self.typed_root) else {
            return false;
        };
        if below_root.as_os_str().is_empty() {
            return false;
        }
        matcher
            .matched(self.anchored_root.join(below_root), is_dir)
            .is_ignore()
    }
}

/// Mirrors `builder.hidden(!include_hidden)`: any dotfile/dotdir component.
fn has_hidden_component(rel: &Path) -> bool {
    rel.components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
}

fn rescue_tracked_files(
    root: &Path,
    include_hidden: bool,
    excludes: &AnchoredExcludes,
    seen: &mut HashSet<PathBuf>,
    canon_parent_cache: &mut HashMap<PathBuf, Option<PathBuf>>,
    all_files: &mut Vec<io::Result<PathBuf>>,
) {
    for rel in git_tracked_files_relative(root) {
        // `git ls-files` prints index entries verbatim; git validates a
        // path when it's added, not when the index is read back, so a
        // hand-crafted .git/index (a shipped tarball, not a clone) could
        // hold an absolute path or a `..` component. root.join() on such a
        // path would discard root entirely, so reject anything but plain
        // components before it's ever joined.
        if !is_plain_relative_path(&rel) {
            continue;
        }

        // Hidden paths are skipped unless --hidden: a tracked-and-gitignored
        // dotfile (.env) or file under a dotdir (.github/workflows/) is
        // exactly the #101 case.
        if !include_hidden && has_hidden_component(&rel) {
            continue;
        }

        let abs = root.join(&rel);

        // Cheap dedup first: most tracked files were already found by the
        // primary walk, so skip the lstat-per-ancestor work below for those.
        let key = dedup_key(&abs, canon_parent_cache);
        if seen.contains(&key) {
            continue;
        }

        // The primary walk prunes an excluded directory before testing any
        // file under it, and never descends a symlinked directory (issue
        // #35). Both are properties of the path, so check every ancestor
        // between the file and root, not just the file path.
        let ancestor_blocked = abs.ancestors().skip(1).take_while(|a| *a != root).any(|a| {
            fs::symlink_metadata(a)
                .map(|m| m.is_symlink())
                .unwrap_or(true)
                || excludes.matches(a, true)
        });
        if ancestor_blocked {
            continue;
        }
        if excludes.matches(&abs, false) {
            continue;
        }

        // symlink_metadata (not metadata): the primary walk never follows
        // symlinks into content outside the tree (issue #35), so neither
        // may this rescue.
        let is_file = fs::symlink_metadata(&abs)
            .map(|m| m.is_file())
            .unwrap_or(false);
        if is_file {
            seen.insert(key);
            all_files.push(Ok(abs));
        }
    }
}

/// Inherited git env vars that would redirect `git ls-files` away from the
/// repo at `root` (repo, work tree, index, or object store); the single list
/// shared by production and the test helper.
///
/// `GIT_CEILING_DIRECTORIES` is deliberately absent: it only bounds upward
/// repo discovery (an explicit boundary the user chose) and never redirects
/// git to a different repo, so stripping it would widen discovery.
const GIT_ENV_TO_STRIP: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

/// Removes inherited git env vars (see `GIT_ENV_TO_STRIP`) from `cmd` so git
/// resolves the repo from its `-C` directory instead of the caller's
/// environment (e.g. when fini runs inside a git hook).
fn strip_inherited_git_env(cmd: &mut Command) {
    for var in GIT_ENV_TO_STRIP {
        cmd.env_remove(var);
    }
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
    let mut cmd = Command::new("git");
    strip_inherited_git_env(&mut cmd);
    let output = cmd
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
            if has_real_git_repo(root) {
                eprintln!(
                    "Warning: could not run git ({e}); tracked-but-gitignored files under {} were not rescanned",
                    crate::output::safe_path_display(root)
                );
            }
            return Vec::new();
        }
    };
    if !output.status.success() {
        if has_real_git_repo(root) {
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

/// Filesystem-only check for a git repo (`.git/HEAD`, or the `.git` file a
/// worktree/submodule uses) in `root` or any ancestor — used to decide
/// whether a git failure deserves a warning, without depending on git
/// itself being available to answer that question.
///
/// Not unified with config::file::has_git_marker; see the comment there.
/// An empty `.git` directory (e.g. a test fixture's) must not count: git
/// itself fails there, and warning about that would be noise.
fn has_real_git_repo(root: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    root.ancestors().any(|dir| {
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

    /// The entries alone, for the tests that don't look at excluded
    /// arguments. Shadows the glob-imported `walk_paths`.
    fn walk_paths(
        paths: &[String],
        exclude_patterns: &[String],
        include_hidden: bool,
    ) -> io::Result<Vec<io::Result<PathBuf>>> {
        super::walk_paths(paths, exclude_patterns, include_hidden).map(|walked| walked.entries)
    }

    fn git_command() -> Command {
        let mut cmd = Command::new("git");
        strip_inherited_git_env(&mut cmd);
        cmd
    }

    fn git_init(dir: &Path) {
        assert!(git_command()
            .args(["init", "-q"])
            .arg(dir)
            .status()
            .unwrap()
            .success());
    }

    fn git_in<const N: usize>(dir: &Path, args: [&str; N]) {
        assert!(git_command()
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap()
            .success());
    }

    // Literal list on purpose (not GIT_ENV_TO_STRIP): deriving the expectations
    // from the constant would stay green if a var were dropped from it.
    #[test]
    fn test_issue_164_strip_inherited_git_env_removes_redirecting_vars() {
        let must_strip = [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        ];
        let mut cmd = Command::new("git");
        for var in must_strip {
            cmd.env(var, "/nonexistent/fini-bogus");
        }
        strip_inherited_git_env(&mut cmd);
        for var in must_strip {
            let entry = cmd.get_envs().find(|(k, _)| *k == var).unwrap();
            assert_eq!(entry.1, None, "{var} must be removed, not passed through");
        }

        let dir = TempDir::new().unwrap();
        git_init(dir.path());
        fs::write(dir.path().join("a.txt"), "x\n").unwrap();
        git_in(dir.path(), ["add", "a.txt"]);
        let out = cmd
            .arg("-C")
            .arg(dir.path())
            .args(["ls-files"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git ls-files failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "a.txt");
    }

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
        let files: Vec<_> = walk_paths(&paths, &[], false).unwrap();

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
            .into_iter()
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
            .into_iter()
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
            .into_iter()
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
            .into_iter()
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
    fn test_issue_101_git_file_still_excluded_with_hidden() {
        // A worktree or submodule checkout has ".git" as a *file* (a
        // "gitdir: ..." pointer), not a directory - the always-added
        // exclude pattern must match both forms.
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(".git"),
            "gitdir: /elsewhere/.git/worktrees/x\n",
        )
        .unwrap();
        fs::write(dir.path().join(".fini_test_hidden_marker"), "SECRET=1").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[], true)
            .unwrap()
            .into_iter()
            .filter_map(|r| r.ok())
            .collect();

        assert!(files
            .iter()
            .any(|f| f.to_string_lossy().contains(".fini_test_hidden_marker")));
        assert!(
            !files.iter().any(|f| f.to_string_lossy().ends_with(".git")),
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
        git_init(dir.path());

        let tracked = dir.path().join(".fini_test_hidden_tracked");
        fs::write(&tracked, "SECRET=hunter2\n").unwrap();
        git_in(dir.path(), ["add", "-f", ".fini_test_hidden_tracked"]);
        fs::write(dir.path().join(".gitignore"), ".fini_test_hidden_tracked\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];

        let without_hidden: Vec<_> = walk_paths(&paths, &[], false)
            .unwrap()
            .into_iter()
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
            .into_iter()
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
            .into_iter()
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
            .into_iter()
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
    fn test_issue_105() {
        // .gitignore is only honoured inside a git repository - WalkBuilder's
        // require_git defaults to true. Outside one (no .git ancestor: a
        // `git archive` export, an extracted tarball, a plain directory),
        // .gitignore files are not consulted at all, and everything they'd
        // otherwise exclude is scanned.
        let dir = TempDir::new().unwrap();
        // Deliberately no .git directory here - but guard against TMPDIR
        // itself living inside a git repo (see test_issue_86 in
        // config/file.rs for the same concern), which would make .gitignore
        // apply and fail this test with a misleading assertion message.
        assert!(
            !dir.path().ancestors().any(|a| a.join(".git").exists()),
            "TMPDIR is inside a git repo; this test cannot run here"
        );
        fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.path().join("kept.txt"), "kept").unwrap();
        fs::write(dir.path().join("ignored.txt"), "ignored").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[], false)
            .unwrap()
            .into_iter()
            .filter_map(|r| r.ok())
            .collect();

        assert!(
            files
                .iter()
                .any(|f| f.to_string_lossy().contains("ignored.txt")),
            "outside a git repo, .gitignore must not be consulted: {files:?}"
        );
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
            .into_iter()
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
            .into_iter()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(files.len(), 1);
        assert!(files[0].to_string_lossy().contains("app.js"));
    }

    /// Walks absolute `paths` as if fini had been started in `cwd`.
    fn walk_from(cwd: &Path, paths: &[&Path], exclude: &[&str]) -> Walked {
        let cwd = cwd.canonicalize().unwrap();
        let paths: Vec<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let exclude: Vec<String> = exclude.iter().map(|p| (*p).to_string()).collect();
        walk_paths_from(Some(&cwd), &paths, &exclude, false).unwrap()
    }

    fn walk_files_from(cwd: &Path, paths: &[&Path], exclude: &[&str]) -> Vec<PathBuf> {
        walk_from(cwd, paths, exclude)
            .entries
            .into_iter()
            .map(|entry| entry.unwrap())
            .collect()
    }

    fn write_file(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "content").unwrap();
    }

    #[test]
    fn test_exclude_applies_to_explicit_file() {
        let dir = TempDir::new().unwrap();
        let kept = dir.path().join("main.rs");
        let minified = dir.path().join("style.min.js");
        let vendored = dir.path().join("vendor/lib.js");
        // A file, so the directory-only pattern `vendor/` must not match it.
        let file_named_like_dir = dir.path().join("sub/vendor");
        for file in [&kept, &minified, &vendored, &file_named_like_dir] {
            write_file(file);
        }

        let files = walk_files_from(
            dir.path(),
            &[&kept, &minified, &vendored, &file_named_like_dir],
            &["*.min.js", "vendor/"],
        );

        assert_eq!(files, vec![kept, file_named_like_dir]);
    }

    #[test]
    fn test_excluded_missing_file_is_still_an_error() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("gone.min.js");

        let results = walk_from(dir.path(), &[&missing], &["*.min.js"]).entries;

        assert_eq!(results.len(), 1);
        assert!(
            results[0].is_err(),
            "a path that does not exist must be reported, not hidden by an exclude: {results:?}"
        );
    }

    #[test]
    fn test_issue_273_file_argument_outside_cwd_is_matched_by_name_only() {
        let base = TempDir::new().unwrap();
        let cwd = base.path().join("proj");
        fs::create_dir(&cwd).unwrap();
        let outside = base.path().join("vendor/lib.js");
        write_file(&outside);

        let files = walk_files_from(&cwd, &[&outside], &["vendor/", "/vendor/lib.js"]);
        assert_eq!(
            files,
            vec![outside.clone()],
            "no directory above an argument outside the cwd is part of the scan"
        );

        let files = walk_files_from(&cwd, &[&outside], &["*.js"]);
        assert!(
            files.is_empty(),
            "a file-name pattern applies wherever the file is, as it would under a directory argument: {files:?}"
        );
    }

    #[test]
    fn test_unresolved_cwd_anchors_patterns_at_the_argument() {
        let dir = TempDir::new().unwrap();
        let vendored = dir.path().join("vendor/lib.js");
        let kept = dir.path().join("src/a.rs");
        for file in [&vendored, &kept, &dir.path().join("src/gen/x.rs")] {
            write_file(file);
        }
        let walk = |argument: &Path, exclude: &str| -> Vec<PathBuf> {
            let paths = [argument.to_string_lossy().to_string()];
            walk_paths_from(None, &paths, &[exclude.to_string()], false)
                .unwrap()
                .entries
                .into_iter()
                .map(|entry| entry.unwrap())
                .collect()
        };

        assert_eq!(walk(&vendored, "vendor/"), vec![vendored.clone()]);
        assert_eq!(walk(&dir.path().join("src"), "/gen/"), vec![kept]);
    }

    #[cfg(unix)]
    #[test]
    fn test_issue_273_symlinked_directory_component_is_resolved() {
        let base = TempDir::new().unwrap();
        let cwd = base.path().join("proj");
        write_file(&cwd.join("tests/a.txt"));
        write_file(&cwd.join("tests/fixtures/x.txt"));
        let link = base.path().join("link");
        std::os::unix::fs::symlink(&cwd, &link).unwrap();

        let files = walk_files_from(
            &cwd,
            &[&link.join("tests/fixtures/x.txt"), &link.join("tests")],
            &["tests/fixtures/"],
        );

        assert_eq!(
            files,
            vec![link.join("tests/a.txt")],
            "link/tests is <cwd>/tests, so tests/fixtures/ must apply under it"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_argument_is_matched_where_the_link_sits() {
        let dir = TempDir::new().unwrap();
        write_file(&dir.path().join("vendor/lib.txt"));
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(dir.path().join("vendor/lib.txt"), &link).unwrap();

        let files = walk_files_from(dir.path(), &[&link], &["vendor/"]);

        assert_eq!(
            files,
            vec![link],
            "the link is not under vendor/; only its target is"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_file_under_symlinked_directory_leaving_cwd_is_matched_as_typed() {
        let base = TempDir::new().unwrap();
        // Canonical, so the absolute arguments below start with the cwd as
        // fini resolves it (macOS reaches the temp dir through a symlink).
        let cwd = base.path().canonicalize().unwrap().join("proj");
        fs::create_dir(&cwd).unwrap();
        write_file(&base.path().join("big/x.csv"));
        std::os::unix::fs::symlink(base.path().join("big"), cwd.join("data")).unwrap();
        let through_link = cwd.join("data/x.csv");

        let files = walk_files_from(&cwd, &[&through_link], &["data/"]);
        assert!(
            files.is_empty(),
            "data/ is the only thing keeping a file behind the link from being rewritten: {files:?}"
        );

        let files = walk_files_from(&cwd, &[&through_link], &["big/"]);
        assert_eq!(
            files,
            vec![through_link],
            "big/ is outside the cwd and not in the typed path"
        );
    }

    #[test]
    fn test_issue_273_directory_above_cwd_does_not_exclude() {
        let base = TempDir::new().unwrap();
        let cwd = base.path().join("vendor/proj");
        let file = cwd.join("main.rs");
        write_file(&file);

        let files = walk_files_from(&cwd, &[&file], &["vendor/"]);

        assert_eq!(files, vec![file]);
    }

    #[test]
    fn test_issue_273_parent_component_is_resolved_before_matching() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("vendor")).unwrap();
        write_file(&dir.path().join("src/main.rs"));
        let through_vendor = dir.path().join("vendor/../src/main.rs");

        let files = walk_files_from(dir.path(), &[&through_vendor], &["vendor/"]);

        assert_eq!(
            files,
            vec![through_vendor],
            "src/main.rs is not under vendor/, however the path was spelled"
        );
    }

    #[test]
    fn test_issue_273_argument_ending_in_parent_component_names_the_parent() {
        let dir = TempDir::new().unwrap();
        write_file(&dir.path().join("main.rs"));
        write_file(&dir.path().join("src/a.rs"));
        let cwd_through_src = dir.path().join("src/..");

        let files = walk_files_from(dir.path(), &[&cwd_through_src], &["src/"]);

        assert_eq!(
            files,
            vec![cwd_through_src.join("main.rs")],
            "src/.. is the cwd, not src: only what is under src/ may be skipped"
        );
    }

    // issue #274
    #[test]
    fn test_excluded_arguments_are_reported_as_typed() {
        let dir = TempDir::new().unwrap();
        let kept = dir.path().join("main.rs");
        let excluded_file = dir.path().join("style.min.js");
        let excluded_dir = dir.path().join("vendor");
        for file in [&kept, &excluded_file, &excluded_dir.join("lib.js")] {
            write_file(file);
        }

        let same_dir_with_slash = dir.path().join("vendor/");
        let walked = walk_from(
            dir.path(),
            &[&kept, &excluded_file, &excluded_dir, &same_dir_with_slash],
            &["*.min.js", "vendor/"],
        );

        assert_eq!(
            walked.excluded_arguments,
            vec![excluded_file, excluded_dir],
            "an argument given twice in different spellings is reported once"
        );
        assert_eq!(walked.entries.len(), 1, "{:?}", walked.entries);
    }

    #[test]
    fn test_entry_excluded_inside_a_walk_is_not_an_excluded_argument() {
        let dir = TempDir::new().unwrap();
        write_file(&dir.path().join("vendor/lib.js"));

        let walked = walk_from(dir.path(), &[dir.path()], &["vendor/"]);

        assert!(walked.entries.is_empty(), "{:?}", walked.entries);
        assert!(
            walked.excluded_arguments.is_empty(),
            "only arguments are reported, not what a walk filters: {:?}",
            walked.excluded_arguments
        );
    }

    #[test]
    fn test_issue_275_excluded_directory_argument_is_skipped() {
        let dir = TempDir::new().unwrap();
        write_file(&dir.path().join("vendor/sub/lib.js"));

        for argument in ["vendor", "vendor/sub"] {
            let files = walk_files_from(dir.path(), &[&dir.path().join(argument)], &["vendor/"]);
            assert!(
                files.is_empty(),
                "{argument} is excluded, so nothing under it may be walked: {files:?}"
            );
        }
    }

    #[test]
    fn test_issue_275_pattern_is_anchored_at_cwd_under_a_directory_argument() {
        let dir = TempDir::new().unwrap();
        let kept = dir.path().join("src/a.rs");
        let elsewhere = dir.path().join("other/src/gen/y.rs");
        for file in [&kept, &elsewhere, &dir.path().join("src/gen/x.rs")] {
            write_file(file);
        }

        let files = walk_files_from(dir.path(), &[&dir.path().join("src")], &["src/gen/"]);
        assert_eq!(files, vec![kept]);

        let files = walk_files_from(dir.path(), &[&dir.path().join("other")], &["src/gen/"]);
        assert_eq!(
            files,
            vec![elsewhere],
            "src/gen/ names <cwd>/src/gen, not other/src/gen"
        );
    }

    #[test]
    fn test_issue_275_anchored_exclude_wins_over_tracked_file_under_directory_argument() {
        let dir = TempDir::new().unwrap();
        git_init(dir.path());
        write_file(&dir.path().join("conf/tracked.env"));
        git_in(dir.path(), ["add", "-f", "conf/tracked.env"]);
        fs::write(dir.path().join(".gitignore"), "tracked.env\n").unwrap();

        let files = walk_files_from(
            dir.path(),
            &[&dir.path().join("conf")],
            &["conf/tracked.env"],
        );

        assert!(
            files.is_empty(),
            "the tracked-file rescue must anchor the pattern at the cwd too: {files:?}"
        );
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
            .into_iter()
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
            .into_iter()
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
            .into_iter()
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
        let results: Vec<_> = walk_paths(&paths, &[], false).unwrap();

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
            let results: Vec<_> = walk_paths(&paths, &[], false).unwrap();
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

        let err = result.expect_err("invalid glob pattern should error");
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

        // Probe the real effect of mode 000 instead of checking the uid: root
        // (and CAP_DAC_OVERRIDE) can still read the directory, leaving nothing
        // to exercise. Outside CI, skip with a reason; under CI, fall through
        // and fail so a permissive runner cannot silently drop this check.
        if fs::read_dir(&blocked_dir).is_ok() && std::env::var_os("CI").is_none() {
            perms.set_mode(0o755);
            fs::set_permissions(&blocked_dir, perms).unwrap();
            eprintln!(
                "skipping test_per_entry_walk_error_does_not_abort_walk: \
                 a mode-000 directory is still readable (running as root?); \
                 set CI to run it anyway"
            );
            return;
        }

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let entries: Vec<_> = walk_paths(&paths, &[], false).unwrap();

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
        git_init(dir.path());

        let tracked = dir.path().join("tracked.env");
        fs::write(&tracked, "SECRET=hunter2\n").unwrap();
        git_in(dir.path(), ["add", "-f", "tracked.env"]);

        // Now ignore it — git keeps tracking it regardless.
        fs::write(dir.path().join(".gitignore"), "tracked.env\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[], false)
            .unwrap()
            .into_iter()
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
        git_init(dir.path());
        fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.path().join("ignored.txt"), "never tracked").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[], false)
            .unwrap()
            .into_iter()
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
        git_init(dir.path());
        let tracked = dir.path().join("tracked.env");
        fs::write(&tracked, "SECRET=hunter2\n").unwrap();
        git_in(dir.path(), ["add", "-f", "tracked.env"]);
        fs::write(dir.path().join(".gitignore"), "tracked.env\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let exclude = vec!["tracked.env".to_string()];
        let files: Vec<_> = walk_paths(&paths, &exclude, false)
            .unwrap()
            .into_iter()
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
        git_init(dir.path());
        fs::create_dir(dir.path().join("vendor")).unwrap();
        let tracked = dir.path().join("vendor/lib.rs");
        fs::write(&tracked, "SECRET=hunter2\n").unwrap();
        git_in(dir.path(), ["add", "-f", "vendor/lib.rs"]);
        fs::write(dir.path().join(".gitignore"), "vendor/\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let exclude = vec!["vendor/".to_string()];
        let files: Vec<_> = walk_paths(&paths, &exclude, false)
            .unwrap()
            .into_iter()
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
        git_init(dir.path());
        let tracked = dir.path().join(".env");
        fs::write(&tracked, "SECRET=hunter2\n").unwrap();
        // -f: the user's global gitignore commonly excludes .env, which
        // would otherwise make `git add` refuse it here regardless of this
        // repo's own (not-yet-written) .gitignore.
        git_in(dir.path(), ["add", "-f", ".env"]);
        fs::write(dir.path().join(".gitignore"), ".env\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[], false)
            .unwrap()
            .into_iter()
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
        git_init(dir.path());
        let target = dir.path().join("outside_target.txt");
        fs::write(&target, "hello").unwrap();
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        git_in(dir.path(), ["add", "link.txt"]);
        fs::write(dir.path().join(".gitignore"), "link.txt\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[], false)
            .unwrap()
            .into_iter()
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
        git_init(dir.path());
        // Track "sub/tracked.txt" as a real directory first, so the git
        // index still references that path once "sub" is replaced with a
        // symlink below — this is how a tracked path ends up "inside" what
        // is, on disk, a symlinked directory.
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("tracked.txt"), "hello").unwrap();
        git_in(dir.path(), ["add", "sub/tracked.txt"]);
        fs::remove_dir_all(&sub).unwrap();
        std::os::unix::fs::symlink(&outside_target, &sub).unwrap();
        fs::write(dir.path().join(".gitignore"), "sub/\n").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths, &[], false)
            .unwrap()
            .into_iter()
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
