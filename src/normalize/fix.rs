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

pub(super) fn fix_fullwidth_spaces(content: &str) -> (String, Vec<Problem>) {
    let problems: Vec<Problem> = content
        .lines()
        .enumerate()
        .flat_map(|(line_idx, line)| {
            let count = line.chars().filter(|&c| c == FULLWIDTH_SPACE).count();
            std::iter::repeat_n(
                Problem {
                    line: line_idx + 1,
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

pub(super) fn normalize_eof_newline(content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }
    let trimmed = content.trim_end_matches('\n');
    format!("{trimmed}\n")
}

pub(super) fn remove_leading_blank_lines(content: &str) -> (String, Vec<Problem>) {
    let lines: Vec<&str> = content.lines().collect();
    let first_non_blank = lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(lines.len());

    let problems = if first_non_blank > 0 {
        vec![Problem {
            line: 1,
            kind: ProblemKind::LeadingBlankLines {
                count: first_non_blank,
            },
        }]
    } else {
        vec![]
    };

    let result = lines
        .get(first_non_blank..)
        .map_or(String::new(), |rest| rest.join("\n"));

    (result, problems)
}

pub(super) fn limit_consecutive_blank_lines(content: &str, max: usize) -> (String, Vec<Problem>) {
    let mut problems = vec![];
    let mut result_lines = vec![];
    let mut blank_count = 0;
    let mut problem_start_line = 0;

    for (line_idx, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            blank_count += 1;
            if blank_count <= max {
                result_lines.push(line);
            } else if blank_count == max + 1 {
                problem_start_line = line_idx + 1;
            }
        } else {
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
            result_lines.push(line);
        }
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

    (result_lines.join("\n"), problems)
}

pub(super) fn remove_code_block_remnants(content: &str) -> (String, Vec<Problem>) {
    let mut problems = vec![];
    let mut result_lines = vec![];

    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if let Some(after_backticks) = trimmed.strip_prefix("```") {
            let is_valid_fence = after_backticks.is_empty()
                || after_backticks
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '+' || c.is_whitespace());

            if is_valid_fence {
                problems.push(Problem {
                    line: line_idx + 1,
                    kind: ProblemKind::CodeBlockRemnant,
                });
                continue;
            }
        }

        result_lines.push(line);
    }

    (result_lines.join("\n"), problems)
}

pub(super) fn remove_zero_width_chars(content: &str) -> (String, Vec<Problem>) {
    let mut problems = vec![];
    let mut result = String::with_capacity(content.len());
    let mut char_idx = 0;

    for (line_idx, line) in content.lines().enumerate() {
        for ch in line.chars() {
            let is_zero_width = ZERO_WIDTH_CHARS.contains(&ch);
            let is_bom_at_start = ch == '\u{FEFF}' && char_idx == 0;

            if is_zero_width && !is_bom_at_start {
                problems.push(Problem {
                    line: line_idx + 1,
                    kind: ProblemKind::ZeroWidthCharacter,
                });
            } else {
                result.push(ch);
            }
            char_idx += 1;
        }
        result.push('\n');
        char_idx += 1;
    }

    // Remove the trailing newline we added (EOF normalization handles this)
    if result.ends_with('\n') && !content.ends_with('\n') {
        result.pop();
    }

    (result, problems)
}
