use ignore::WalkBuilder;
use std::io;
use std::path::PathBuf;

/// Walk paths and yield file paths, respecting gitignore
pub fn walk_paths(paths: &[String]) -> impl Iterator<Item = io::Result<PathBuf>> {
    let mut all_files = vec![];

    for path in paths {
        let walker = WalkBuilder::new(path)
            .hidden(true) // Skip hidden files
            .git_ignore(true) // Respect .gitignore
            .git_global(true)
            .git_exclude(true)
            .build();

        for entry in walker {
            match entry {
                Ok(entry) => {
                    if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                        all_files.push(Ok(entry.into_path()));
                    }
                }
                Err(e) => {
                    all_files.push(Err(io::Error::other(e.to_string())));
                }
            }
        }
    }

    all_files.into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ===========================================
    // Phase 2: File Walker Tests
    // ===========================================

    #[test]
    fn test_walk_single_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello").unwrap();

        let paths = vec![file_path.to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths).collect();

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
        let files: Vec<_> = walk_paths(&paths).filter_map(|r| r.ok()).collect();

        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_skip_hidden_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("visible.txt"), "visible").unwrap();
        fs::write(dir.path().join(".hidden"), "hidden").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths).filter_map(|r| r.ok()).collect();

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
        let files: Vec<_> = walk_paths(&paths).filter_map(|r| r.ok()).collect();

        assert_eq!(files.len(), 1);
        assert!(!files[0].to_string_lossy().contains(".git"));
    }

    #[test]
    fn test_respect_gitignore() {
        let dir = TempDir::new().unwrap();

        // Create a .git directory so ignore crate respects .gitignore
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(dir.path().join("kept.txt"), "kept").unwrap();
        fs::write(dir.path().join("ignored.txt"), "ignored").unwrap();

        let paths = vec![dir.path().to_string_lossy().to_string()];
        let files: Vec<_> = walk_paths(&paths).filter_map(|r| r.ok()).collect();

        // ignored.txt should be excluded by .gitignore rules
        assert!(files
            .iter()
            .all(|f| !f.to_string_lossy().contains("ignored.txt")));
        // kept.txt should be present
        assert!(files
            .iter()
            .any(|f| f.to_string_lossy().contains("kept.txt")));
    }
}
