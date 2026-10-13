use super::{Problem, ProblemKind};

const FULLWIDTH_SPACE: char = '\u{3000}';

const ZERO_WIDTH_CHARS: &[char] = &[
    '\u{200B}', // Zero Width Space (ZWSP)
    '\u{200C}', // Zero Width Non-Joiner (ZWNJ)
    '\u{200D}', // Zero Width Joiner (ZWJ)
    '\u{200E}', // Left-to-Right Mark
    '\u{200F}', // Right-to-Left Mark
    '\u{2060}', // Word Joiner
    '\u{FEFF}', // Byte Order Mark (BOM) - removed except at file start
];

pub(super) fn normalize_line_endings(content: &str) -> String {
    content.replace("\r\n", "\n").replace('\r', "\n")
}

pub(super) fn fix_fullwidth_spaces(content: &str, line_map: &[usize]) -> (String, Vec<Problem>) {
    let problems: Vec<Problem> = content
        .lines()
        .enumerate()
        .flat_map(|(line_idx, line)| {
            let count = line.chars().filter(|&c| c == FULLWIDTH_SPACE).count();
            std::iter::repeat_n(
                Problem {
                    line: line_map[line_idx],
                    kind: ProblemKind::FullWidthSpace,
                },
                count,
            )
        })
        .collect();

    let result = content.replace(FULLWIDTH_SPACE, " ");
    (result, problems)
}

pub(super) fn remove_trailing_whitespace(content: &str) -> String {
    content
        .lines()
        .map(|line| line.trim_end_matches([' ', '\t']))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Reproduces the fixer transforms that run *before* trailing-whitespace
/// removal in the pipeline (zero-width-character removal, fullwidth-space
/// conversion) on a single line, so a caller deciding "would trailing-
/// whitespace removal change this line" sees what the fixer sees rather
/// than the raw line (issue #94). `remove_zero_width` must match the
/// config's own setting - zero-width removal is optional (`--keep-zero-width`
/// / `remove_zero_width = false`), unlike fullwidth-space conversion, which
/// always runs.
///
/// Steps that can drop a line entirely (code-fence removal, leading/
/// consecutive-blank-line limits) are deliberately not reproduced here: this
/// only answers "is this line's own trailing whitespace trimmable", not
/// "does this line survive at all". A leading BOM is a line *prefix* and
/// never affects a trailing-whitespace check, so it needs no special case.
pub(crate) fn line_after_pre_trim_fixes(line: &str, remove_zero_width: bool) -> String {
    let without_zero_width: String = if remove_zero_width {
        line.chars()
            .filter(|c| !ZERO_WIDTH_CHARS.contains(c))
            .collect()
    } else {
        line.to_string()
    };
    without_zero_width.replace(FULLWIDTH_SPACE, " ")
}

// Callers must re-attach the BOM via `reattach_bom`.
fn split_bom(content: &str) -> (&str, &str) {
    content.split_at(if content.starts_with('\u{FEFF}') {
        '\u{FEFF}'.len_utf8()
    } else {
        0
    })
}

// A bare BOM still occupies a line of the output, so keep the map entry of
// the original first line when every body line was dropped.
fn reattach_bom(
    bom: &str,
    kept_lines: &[&str],
    mut kept_map: Vec<usize>,
    line_map: &[usize],
) -> (String, Vec<usize>) {
    if !bom.is_empty() && kept_map.is_empty() {
        kept_map.push(line_map[0]);
    }
    (format!("{bom}{}", kept_lines.join("\n")), kept_map)
}

pub(super) fn normalize_eof_newline(content: &str) -> String {
    let trimmed = content.trim_end_matches('\n');
    if trimmed.is_empty() {
        return String::new();
    }
    format!("{trimmed}\n")
}

/// `line_map[i]` gives the original file line number for line `i` (0-indexed)
/// of `content`. Fixes that drop or reorder lines return an updated map
/// alongside their result so downstream problems (from later fixes or
/// detections) can report the real line the user has open, not the
/// post-fix line (issue #76).
pub(super) fn remove_leading_blank_lines(
    content: &str,
    line_map: &[usize],
) -> (String, Vec<usize>, Vec<Problem>) {
    let (bom, body) = split_bom(content);
    let lines: Vec<&str> = body.lines().collect();
    let first_non_blank = lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(lines.len());

    // With a BOM, the first body "line" is the rest of the BOM line, not a
    // real blank line; reporting it would recur on every run.
    if !bom.is_empty() && first_non_blank == lines.len() {
        return (content.to_string(), line_map.to_vec(), vec![]);
    }

    let problems = if first_non_blank > 0 {
        vec![Problem {
            line: line_map[0],
            kind: ProblemKind::LeadingBlankLines {
                count: first_non_blank,
            },
        }]
    } else {
        vec![]
    };

    let (result, new_line_map) = reattach_bom(
        bom,
        &lines[first_non_blank..],
        line_map[first_non_blank..].to_vec(),
        line_map,
    );

    (result, new_line_map, problems)
}

pub(super) fn limit_consecutive_blank_lines(
    content: &str,
    max: usize,
    line_map: &[usize],
) -> (String, Vec<usize>, Vec<Problem>) {
    let mut problems = vec![];
    let mut result_lines = vec![];
    let mut new_line_map = vec![];
    let mut blank_count = 0;
    let mut problem_start_line = 0;

    // The trailing `None` flushes a run of blanks that reaches end of input.
    let lines = content.lines().map(Some).chain(std::iter::once(None));
    for (line_idx, line) in lines.enumerate() {
        if let Some(blank) = line.filter(|l| l.trim().is_empty()) {
            blank_count += 1;
            if blank_count <= max {
                result_lines.push(blank);
                new_line_map.push(line_map[line_idx]);
            } else if blank_count == max + 1 {
                problem_start_line = line_map[line_idx];
            }
            continue;
        }
        if blank_count > max {
            problems.push(Problem {
                line: problem_start_line,
                kind: ProblemKind::ExcessiveBlankLines {
                    found: blank_count,
                    limit: max,
                },
            });
        }
        blank_count = 0;
        if let Some(line) = line {
            result_lines.push(line);
            new_line_map.push(line_map[line_idx]);
        }
    }

    (result_lines.join("\n"), new_line_map, problems)
}

pub(super) fn remove_code_block_remnants(
    content: &str,
    line_map: &[usize],
) -> (String, Vec<usize>, Vec<Problem>) {
    let mut problems = vec![];
    let mut result_lines = vec![];
    let mut new_line_map = vec![];
    let (bom, body) = split_bom(content);

    for (line_idx, line) in body.lines().enumerate() {
        let trimmed = line.trim();

        if let Some(after_backticks) = trimmed.strip_prefix("```") {
            let is_valid_fence = after_backticks.is_empty()
                || after_backticks
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '+' || c.is_whitespace());

            if is_valid_fence {
                problems.push(Problem {
                    line: line_map[line_idx],
                    kind: ProblemKind::CodeBlockRemnant,
                });
                continue;
            }
        }

        result_lines.push(line);
        new_line_map.push(line_map[line_idx]);
    }

    let (result, new_line_map) = reattach_bom(bom, &result_lines, new_line_map, line_map);

    (result, new_line_map, problems)
}

pub(super) fn remove_zero_width_chars(content: &str, line_map: &[usize]) -> (String, Vec<Problem>) {
    let mut problems = vec![];
    let mut result = String::with_capacity(content.len());

    for (line_idx, line) in content.lines().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            let is_zero_width = ZERO_WIDTH_CHARS.contains(&ch);
            let is_bom_at_start = ch == '\u{FEFF}' && line_idx == 0 && col == 0;

            if is_zero_width && !is_bom_at_start {
                problems.push(Problem {
                    line: line_map[line_idx],
                    kind: ProblemKind::ZeroWidthCharacter,
                });
            } else {
                result.push(ch);
            }
        }
        result.push('\n');
    }

    // Remove the trailing newline we added (EOF normalization handles this)
    if result.ends_with('\n') && !content.ends_with('\n') {
        result.pop();
    }

    (result, problems)
}
