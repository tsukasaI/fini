use std::collections::{HashMap, HashSet};

use super::{is_fix_kind_id, ProblemKind};

const DIRECTIVE: &str = "fini:ignore";
const NEXT_LINE_DIRECTIVE: &str = "fini:ignore-next-line";

/// Byte offset of a fini:ignore(-next-line) directive marker in `line`, if
/// any - the one place that knows what counts as "directive text" on a
/// line, shared by directive parsing and by detectors that must not scan
/// past it (issue #95).
pub(super) fn directive_start(line: &str) -> Option<usize> {
    line.find(DIRECTIVE)
}

/// Tracks which lines have ignore directives and what kinds they suppress.
/// `None` = ignore all kinds, `Some(set)` = ignore only listed kinds.
pub(super) struct IgnoreMap {
    ignores: HashMap<usize, Option<HashSet<String>>>,
    /// Some directive named only fix-type kinds, which can never suppress
    /// anything (see `is_ignored`).
    fix_kinds_only_directive: bool,
}

impl IgnoreMap {
    pub(super) fn is_empty(&self) -> bool {
        self.ignores.is_empty()
    }

    pub(super) fn has_fix_kinds_only_directive(&self) -> bool {
        self.fix_kinds_only_directive
    }

    pub(super) fn is_ignored(&self, line: usize, kind: &ProblemKind) -> bool {
        // Fix-type problems change the content regardless, so a directive
        // hiding them would leave `--check` failing with no explanation (#165).
        let Some(id) = kind.ignore_id() else {
            return false;
        };
        match self.ignores.get(&line) {
            None => false,
            Some(None) => true,
            Some(Some(set)) => set.contains(id),
        }
    }

    fn insert(&mut self, line: usize, kinds: Option<HashSet<String>>) {
        // Vacant starts from an empty kind set, so the merge below yields exactly `kinds`
        let slot = self
            .ignores
            .entry(line)
            .or_insert_with(|| Some(HashSet::new()));
        match (slot, kinds) {
            (Some(existing), Some(new)) => existing.extend(new),
            (slot, None) => *slot = None, // ignore-all absorbs any kind list
            (None, Some(_)) => {}         // already ignore-all
        }
    }
}

/// `line_map[i]` gives the original file line number for line `i` (0-indexed)
/// of `content`, mirroring the same map threaded through the fix pipeline —
/// `Problem.line` is remapped to original line numbers by the time this runs,
/// so directives must key on original line numbers too (issue #76).
pub(super) fn parse_ignore_directives(content: &str, line_map: &[usize]) -> IgnoreMap {
    let mut map = IgnoreMap {
        ignores: HashMap::new(),
        fix_kinds_only_directive: false,
    };

    for (idx, line) in content.lines().enumerate() {
        let line_num = line_map[idx];
        // The user's literal next line, per the README ("suppress all
        // detections on the next line") — not the next surviving line in
        // `content`. If a fix removed that line (e.g. a code fence), the
        // directive simply matches no problem, rather than reaching past it
        // to whatever line took its place.
        let next_line_num = line_num + 1;

        let Some(pos) = directive_start(line) else {
            continue;
        };
        let is_next_line = line[pos..].starts_with(NEXT_LINE_DIRECTIVE);
        let kinds = parse_kind_list(
            line,
            pos + if is_next_line {
                NEXT_LINE_DIRECTIVE.len()
            } else {
                DIRECTIVE.len()
            },
        );
        if kinds
            .as_ref()
            .is_some_and(|k| k.iter().all(|id| is_fix_kind_id(id)))
        {
            map.fix_kinds_only_directive = true;
        }
        if is_next_line {
            // Same kind restriction on the directive's own line as on the
            // next line — not ignore-all (issue #95).
            map.insert(line_num, kinds.clone());
            map.insert(next_line_num, kinds);
        } else {
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

    (!kinds.is_empty()).then_some(kinds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::identity_line_map;

    #[test]
    fn ignore_all_same_line() {
        let content = "// TODO: fix this fini:ignore\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
        assert!(map.is_ignored(1, &ProblemKind::DebugCode { pattern: "print(" }));
    }

    #[test]
    fn ignore_selective_same_line() {
        let content = "console.log('x'); // TODO: fix fini:ignore debug\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
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
        let map = parse_ignore_directives(content, &identity_line_map(content));
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
        assert!(map.is_ignored(1, &ProblemKind::DebugCode { pattern: "dbg!(" }));
        assert!(map.is_ignored(1, &ProblemKind::SecretPattern { hint: "key" }));
        assert!(!map.is_ignored(1, &ProblemKind::FixmeComment));
    }

    #[test]
    fn ignore_kinds_with_spaces() {
        let content = "// fini:ignore todo , debug\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
        assert!(map.is_ignored(1, &ProblemKind::DebugCode { pattern: "print(" }));
    }

    #[test]
    fn ignore_next_line_all() {
        let content = "// fini:ignore-next-line\n// TODO: fix this\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
        assert!(map.is_ignored(2, &ProblemKind::TodoComment));
        assert!(map.is_ignored(2, &ProblemKind::DebugCode { pattern: "dbg!(" }));
    }

    #[test]
    fn ignore_next_line_selective() {
        let content = "# fini:ignore-next-line debug,secret\nprint('hello')\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        assert!(map.is_ignored(2, &ProblemKind::DebugCode { pattern: "print(" }));
        assert!(map.is_ignored(2, &ProblemKind::SecretPattern { hint: "key" }));
        assert!(!map.is_ignored(2, &ProblemKind::TodoComment));
    }

    #[test]
    fn hash_comment_style() {
        let content = "# TODO: something fini:ignore\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
    }

    #[test]
    fn block_comment_style() {
        let content = "/* TODO: something fini:ignore */\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
    }

    #[test]
    fn html_comment_style() {
        let content = "<!-- TODO: something fini:ignore -->\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
    }

    #[test]
    fn block_comment_selective() {
        let content = "/* fini:ignore todo */\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
        assert!(!map.is_ignored(1, &ProblemKind::FixmeComment));
    }

    #[test]
    fn directive_line_self_suppresses() {
        let content = "// fini:ignore-next-line todo\nsome code\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
        // issue #95: the directive's own line must only suppress the kinds
        // it actually names, not every kind.
        assert!(!map.is_ignored(1, &ProblemKind::SecretPattern { hint: "key" }));
    }

    #[test]
    fn unknown_kind_silently_ignored() {
        let content = "// fini:ignore unknown,todo\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
    }

    #[test]
    fn ignore_next_line_at_end_of_file() {
        let content = "// fini:ignore-next-line\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        // Should not panic; directive line self-suppresses
        assert!(map.is_ignored(1, &ProblemKind::TodoComment));
        // Line 2 entry exists but is harmless — no problems will reference it
        assert!(map.is_ignored(2, &ProblemKind::TodoComment));
    }

    #[test]
    fn multiple_directives_merge() {
        let content = "// fini:ignore-next-line todo\n// FIXME: broken fini:ignore fixme\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        assert!(map.is_ignored(2, &ProblemKind::TodoComment));
        assert!(map.is_ignored(2, &ProblemKind::FixmeComment));
        assert!(!map.is_ignored(2, &ProblemKind::DebugCode { pattern: "print(" }));
    }

    #[test]
    fn ignore_all_wins_over_selective() {
        let content = "// fini:ignore-next-line\n// TODO: fix fini:ignore todo\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        assert!(map.is_ignored(2, &ProblemKind::TodoComment));
        assert!(map.is_ignored(2, &ProblemKind::DebugCode { pattern: "print(" }));
    }

    #[test]
    fn no_directives_returns_empty() {
        let content = "// just a comment\nfn main() {}\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        assert!(map.is_empty());
    }

    #[test]
    fn fix_kinds_never_suppressed() {
        let kinds = [
            ProblemKind::FullWidthSpace,
            ProblemKind::ZeroWidthCharacter,
            ProblemKind::CodeBlockRemnant,
            ProblemKind::LeadingBlankLines { count: 1 },
            ProblemKind::ExcessiveBlankLines { found: 5, limit: 2 },
        ];
        let content = "x fini:ignore fullwidth,zero-width,code-block,leading-blanks,blank-lines\ny fini:ignore\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        for kind in &kinds {
            assert!(!map.is_ignored(1, kind), "selective: {kind:?}");
            assert!(!map.is_ignored(2, kind), "ignore-all: {kind:?}");
        }
        assert!(map.is_ignored(2, &ProblemKind::TodoComment));
    }

    #[test]
    fn fix_kinds_only_directive_flag() {
        let flagged = |content: &str| {
            parse_ignore_directives(content, &identity_line_map(content))
                .has_fix_kinds_only_directive()
        };
        assert!(flagged("x fini:ignore fullwidth\n"));
        assert!(flagged("# fini:ignore-next-line zero-width, blank-lines\n"));
        assert!(!flagged("x fini:ignore\n"));
        assert!(!flagged("x fini:ignore todo\n"));
        assert!(!flagged("x fini:ignore fullwidth,todo\n"));
        assert!(!flagged("x fini:ignore bogus\n"));
    }

    #[test]
    fn insert_merges_all_kind_combinations() {
        let set = |ids: &[&str]| Some(ids.iter().map(|s| s.to_string()).collect::<HashSet<_>>());
        let merged = |first: Option<HashSet<String>>, second: Option<HashSet<String>>| {
            let mut map = IgnoreMap {
                ignores: HashMap::new(),
                fix_kinds_only_directive: false,
            };
            map.insert(1, first);
            map.insert(1, second);
            map.ignores.remove(&1).unwrap()
        };
        assert_eq!(
            merged(set(&["todo"]), set(&["fixme"])),
            set(&["todo", "fixme"])
        );
        assert_eq!(merged(set(&["todo"]), None), None);
        assert_eq!(merged(None, set(&["todo"])), None);
        assert_eq!(merged(None, None), None);
    }

    #[test]
    fn line_length_ignore() {
        let content =
            "let x = \"a very long string that exceeds the limit\"; // fini:ignore line-length\n";
        let map = parse_ignore_directives(content, &identity_line_map(content));
        assert!(map.is_ignored(
            1,
            &ProblemKind::LongLine {
                length: 100,
                limit: 80
            }
        ));
    }
}
