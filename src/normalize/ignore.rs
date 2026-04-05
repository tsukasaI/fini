use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use super::ProblemKind;

const DIRECTIVE: &str = "fini:ignore";
const NEXT_LINE_DIRECTIVE: &str = "fini:ignore-next-line";

/// Tracks which lines have ignore directives and what kinds they suppress.
/// `None` = ignore all kinds, `Some(set)` = ignore only listed kinds.
pub(super) struct IgnoreMap {
    ignores: HashMap<usize, Option<HashSet<String>>>,
}

impl IgnoreMap {
    pub(super) fn is_empty(&self) -> bool {
        self.ignores.is_empty()
    }

    pub(super) fn is_ignored(&self, line: usize, kind: &ProblemKind) -> bool {
        match self.ignores.get(&line) {
            None => false,
            Some(None) => true,
            Some(Some(set)) => set.contains(kind.ignore_id()),
        }
    }

    fn insert(&mut self, line: usize, kinds: Option<HashSet<String>>) {
        match self.ignores.entry(line) {
            Entry::Vacant(e) => {
                e.insert(kinds);
            }
            Entry::Occupied(mut e) => match e.get_mut() {
                None => {}
                slot @ Some(_) => match kinds {
                    None => *slot = None,
                    Some(new_kinds) => slot.as_mut().unwrap().extend(new_kinds),
                },
            },
        }
    }
}

pub(super) fn parse_ignore_directives(content: &str) -> IgnoreMap {
    let mut map = IgnoreMap {
        ignores: HashMap::new(),
    };

    for (idx, line) in content.lines().enumerate() {
        let line_num = idx + 1;

        if let Some(pos) = line.find(NEXT_LINE_DIRECTIVE) {
            map.insert(line_num, None);
            let kinds = parse_kind_list(line, pos + NEXT_LINE_DIRECTIVE.len());
            map.insert(line_num + 1, kinds);
        } else if let Some(pos) = line.find(DIRECTIVE) {
            let kinds = parse_kind_list(line, pos + DIRECTIVE.len());
            map.insert(line_num, kinds);
        }
    }

    map
}

fn parse_kind_list(line: &str, offset: usize) -> Option<HashSet<String>> {
    let rest = line[offset..].trim();
    if rest.is_empty() || rest.starts_with("*/") || rest.starts_with("-->") {
        return None;
    }

    let kinds: HashSet<String> = rest
        .split(',')
        .filter_map(|s| {
            let s = s
                .trim()
                .trim_end_matches("*/")
                .trim_end_matches("-->")
                .trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_lowercase())
            }
        })
        .collect();

    if kinds.is_empty() {
        None
    } else {
        Some(kinds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignore_all_same_line() {
        let content = "// TODO: fix this fini:ignore\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
        assert!(map.is_ignored(1, &ProblemKind::DebugCode { pattern: "print(" }));
    }

    #[test]
    fn ignore_selective_same_line() {
        let content = "console.log('x'); // TODO: fix fini:ignore debug\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_ignored(
            1,
            &ProblemKind::DebugCode {
                pattern: "console.log("
            }
        ));
        assert!(!map.is_ignored(1, &ProblemKind::TodoComment));
    }

    #[test]
    fn ignore_multiple_kinds() {
        let content = "// fini:ignore todo,debug,secret\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
        assert!(map.is_ignored(1, &ProblemKind::DebugCode { pattern: "dbg!(" }));
        assert!(map.is_ignored(1, &ProblemKind::SecretPattern { hint: "key" }));
        assert!(!map.is_ignored(1, &ProblemKind::FixmeComment));
    }

    #[test]
    fn ignore_kinds_with_spaces() {
        let content = "// fini:ignore todo , debug\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
        assert!(map.is_ignored(1, &ProblemKind::DebugCode { pattern: "print(" }));
    }

    #[test]
    fn ignore_next_line_all() {
        let content = "// fini:ignore-next-line\n// TODO: fix this\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
        assert!(map.is_ignored(2, &ProblemKind::TodoComment));
        assert!(map.is_ignored(2, &ProblemKind::DebugCode { pattern: "dbg!(" }));
    }

    #[test]
    fn ignore_next_line_selective() {
        let content = "# fini:ignore-next-line debug,secret\nprint('hello')\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_ignored(2, &ProblemKind::DebugCode { pattern: "print(" }));
        assert!(map.is_ignored(2, &ProblemKind::SecretPattern { hint: "key" }));
        assert!(!map.is_ignored(2, &ProblemKind::TodoComment));
    }

    #[test]
    fn hash_comment_style() {
        let content = "# TODO: something fini:ignore\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
    }

    #[test]
    fn block_comment_style() {
        let content = "/* TODO: something fini:ignore */\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
    }

    #[test]
    fn html_comment_style() {
        let content = "<!-- TODO: something fini:ignore -->\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
    }

    #[test]
    fn block_comment_selective() {
        let content = "/* fini:ignore todo */\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
        assert!(!map.is_ignored(1, &ProblemKind::FixmeComment));
    }

    #[test]
    fn directive_line_self_suppresses() {
        let content = "// fini:ignore-next-line todo\nsome code\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
    }

    #[test]
    fn unknown_kind_silently_ignored() {
        let content = "// fini:ignore unknown,todo\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
    }

    #[test]
    fn ignore_next_line_at_end_of_file() {
        let content = "// fini:ignore-next-line\n";
        let map = parse_ignore_directives(content);
        // Should not panic; directive line self-suppresses
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
        // Line 2 entry exists but is harmless — no problems will reference it
        assert!(map.is_ignored(2, &ProblemKind::TodoComment));
    }

    #[test]
    fn multiple_directives_merge() {
        let content = "// fini:ignore-next-line todo\n// FIXME: broken fini:ignore fixme\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_ignored(2, &ProblemKind::TodoComment));
        assert!(map.is_ignored(2, &ProblemKind::FixmeComment));
        assert!(!map.is_ignored(2, &ProblemKind::DebugCode { pattern: "print(" }));
    }

    #[test]
    fn ignore_all_wins_over_selective() {
        let content = "// fini:ignore-next-line\n// TODO: fix fini:ignore todo\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_ignored(2, &ProblemKind::TodoComment));
        assert!(map.is_ignored(2, &ProblemKind::DebugCode { pattern: "print(" }));
    }

    #[test]
    fn no_directives_returns_empty() {
        let content = "// just a comment\nfn main() {}\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_empty());
    }

    #[test]
    fn line_length_ignore() {
        let content =
            "let x = \"a very long string that exceeds the limit\"; // fini:ignore line-length\n";
        let map = parse_ignore_directives(content);
        assert!(map.is_ignored(
            1,
            &ProblemKind::LongLine {
                length: 100,
                limit: 80
            }
        ));
    }
}
