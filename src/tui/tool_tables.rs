//! Tables in tool output, laid out at the transcript's width.
//!
//! A table a command prints (`docker ps`, `kubectl get`, `psql`, a box-drawn
//! report) is raw text sized for some other terminal. Wrapped at the
//! transcript's width its rows fold into each other and the columns are lost.
//! So a table run that does not fit is turned into a Markdown table and goes
//! through the same layout as tables in answers (`markdown::render_markdown_at`),
//! which fits it to the width on every draw: a grid, a grid with wrapped
//! cells, or records. Output that fits is left exactly as printed, colours
//! included.
//!
//! Two shapes are recognised:
//! - separated: every row splits on `|` / `│` / `║` into the same number of
//!   cells; border rows (`+--+`, `├──┼`, `----+----`) are dropped;
//! - aligned: columns separated by runs of two or more spaces that line up
//!   in every row, with the first row (the header) naming each column.

/// A stretch of tool output: a raw line, or a table as Markdown source.
#[derive(Debug, PartialEq)]
pub enum Block<'a> {
    Raw(&'a str),
    Table(String),
}

/// Fewest rows (header included) an aligned table needs before it is
/// believed; two-line coincidences are common in logs and code.
const MIN_ALIGNED_ROWS: usize = 3;

/// Split `lines` into raw lines and the tables among them that are wider
/// than `avail` columns.
pub fn segment<'a>(lines: &[&'a str], avail: usize) -> Vec<Block<'a>> {
    let plain: Vec<Vec<char>> = lines
        .iter()
        .map(|l| super::ansi::strip(l).trim_end().chars().collect())
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let found = separated_run(&plain, i).or_else(|| aligned_run(&plain, i));
        match found {
            Some((end, rows)) if plain[i..end].iter().any(|l| l.len() > avail) => {
                out.push(Block::Table(markdown_table(&rows)));
                i = end;
            }
            Some((end, _)) => {
                out.extend(lines[i..end].iter().map(|l| Block::Raw(l)));
                i = end;
            }
            None => {
                out.push(Block::Raw(lines[i]));
                i += 1;
            }
        }
    }
    out
}

fn is_separator(c: char) -> bool {
    matches!(c, '|' | '│' | '║' | '┃')
}

/// A border row: nothing but rule characters.
fn is_border(line: &[char]) -> bool {
    line.iter().any(|c| !c.is_whitespace())
        && line.iter().all(|&c| {
            c.is_whitespace()
                || is_separator(c)
                || "-=+:─━═┌┐└┘├┤┬┴┼╭╮╰╯╔╗╚╝╠╣╦╩╬╞╡╪╟╢╫".contains(c)
        })
}

fn separated_cells(line: &[char]) -> Option<Vec<String>> {
    if !line.iter().any(|&c| is_separator(c)) {
        return None;
    }
    // `   12|code` is a numbered source line (read_file, hashline excerpts).
    let indent = line.iter().take_while(|c| **c == ' ').count();
    let digits = line[indent..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count();
    if digits > 0 && line.get(indent + digits) == Some(&'|') {
        return None;
    }
    let text: String = line.iter().collect();
    let mut cells: Vec<String> = text
        .split(is_separator)
        .map(|cell| cell.trim().to_string())
        .collect();
    // Outer borders leave an empty cell at either end.
    let trimmed = text.trim();
    if trimmed.starts_with(is_separator) {
        cells.remove(0);
    }
    if trimmed.ends_with(is_separator) && !cells.is_empty() {
        cells.pop();
    }
    (cells.len() >= 2).then_some(cells)
}

/// Rows from `start` that split into the same number of cells, borders
/// skipped: a header and two rows, or one row under a border (two lines that
/// merely contain a `|` are too easy to mistake).
fn separated_run(lines: &[Vec<char>], start: usize) -> Option<(usize, Vec<Vec<String>>)> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut bordered = false;
    let mut end = start;
    let mut i = start;
    while i < lines.len() {
        if is_border(&lines[i]) {
            bordered = true;
            i += 1;
            if !rows.is_empty() {
                end = i;
            }
            continue;
        }
        let Some(cells) = separated_cells(&lines[i]) else {
            break;
        };
        if rows.first().is_some_and(|first| first.len() != cells.len()) {
            break;
        }
        rows.push(cells);
        i += 1;
        end = i;
    }
    // A leading border belongs to the table too.
    let start_border = lines[start..].iter().take_while(|l| is_border(l)).count();
    let enough = rows.len() >= 3 || (bordered && rows.len() >= 2);
    (enough && end > start + start_border).then_some((end, rows))
}

/// The column starts a header and its rows share: positions after a run of
/// two or more columns that are blank in every row (`blank`), where the
/// header has a word.
fn column_starts(header: &[char], blank: &[bool]) -> Vec<usize> {
    let indent = header.iter().take_while(|c| **c == ' ').count();
    let mut starts = vec![indent];
    let mut run = 0;
    for (p, is_blank) in blank.iter().enumerate().skip(indent) {
        if *is_blank {
            run += 1;
            continue;
        }
        if run >= 2 && header.get(p).is_some_and(|c| *c != ' ') {
            starts.push(p);
        }
        run = 0;
    }
    starts
}

/// The longest run from `start` whose columns line up, if it is a table.
fn aligned_run(lines: &[Vec<char>], start: usize) -> Option<(usize, Vec<Vec<String>>)> {
    let header = &lines[start];
    let mut blank: Vec<bool> = Vec::new();
    let mut end = start;
    let mut starts = Vec::new();
    for (i, line) in lines.iter().enumerate().skip(start) {
        if line.is_empty() {
            break;
        }
        // A column is blank while every row so far has a space (or nothing)
        // there; past the end of the shorter rows counts as blank.
        let mut next = blank.clone();
        if next.len() < line.len() {
            next.resize(line.len(), true);
        }
        for (p, c) in line.iter().enumerate() {
            if *c != ' ' {
                next[p] = false;
            }
        }
        let cols = column_starts(header, &next);
        // Two columns at least, and every row has a word in the first column
        // and in one more (a trailing `done` is not a row).
        let filled = cols
            .iter()
            .enumerate()
            .filter(|(n, &from)| {
                let to = cols
                    .get(n + 1)
                    .copied()
                    .unwrap_or(line.len())
                    .min(line.len());
                line.get(from.min(to)..to)
                    .is_some_and(|cell| cell.iter().any(|c| *c != ' '))
            })
            .count();
        if cols.len() < 2 || line.get(cols[0]).is_none_or(|c| *c == ' ') || filled < 2 {
            break;
        }
        blank = next;
        end = i + 1;
        starts = cols;
    }
    if end - start < MIN_ALIGNED_ROWS {
        return None;
    }
    let rows = lines[start..end]
        .iter()
        .map(|line| {
            starts
                .iter()
                .enumerate()
                .map(|(n, &from)| {
                    let to = starts.get(n + 1).copied().unwrap_or(line.len());
                    line.get(from.min(line.len())..to.min(line.len()))
                        .map(|cell| cell.iter().collect::<String>().trim().to_string())
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect();
    Some((end, rows))
}

/// `rows` (the first is the header) as a Markdown table with every cell's
/// punctuation escaped, so `*`, `_`, `|` and friends stay literal text.
fn markdown_table(rows: &[Vec<String>]) -> String {
    let escape = |cell: &str| {
        let mut out = String::with_capacity(cell.len());
        for c in cell.chars() {
            if c.is_ascii_punctuation() {
                out.push('\\');
            }
            out.push(c);
        }
        out
    };
    let line = |cells: &[String]| {
        let inner: Vec<String> = cells.iter().map(|c| escape(c)).collect();
        format!("| {} |", inner.join(" | "))
    };
    let cols = rows.first().map(Vec::len).unwrap_or(0);
    let mut text = line(&rows[0]);
    text.push('\n');
    text.push_str(&format!("|{}\n", "---|".repeat(cols)));
    for row in &rows[1..] {
        let mut cells = row.clone();
        cells.resize(cols, String::new());
        text.push_str(&line(&cells));
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables(text: &str, avail: usize) -> Vec<String> {
        let lines: Vec<&str> = text.lines().collect();
        segment(&lines, avail)
            .into_iter()
            .filter_map(|b| match b {
                Block::Table(t) => Some(t),
                Block::Raw(_) => None,
            })
            .collect()
    }

    const DOCKER: &str = "CONTAINER ID   IMAGE          STATUS          NAMES\n\
                          3f2a1b9c8d7e   postgres:16    Up 2 hours      db\n\
                          9e8d7c6b5a4f   redis:7        Up 2 hours      cache";

    /// Aligned output that is too wide becomes one table with the right
    /// cells, spaces inside a column (`Up 2 hours`) included.
    #[test]
    fn aligned_columns_become_a_table_when_too_wide() {
        let found = tables(DOCKER, 40);
        assert_eq!(found.len(), 1, "{found:?}");
        let table = &found[0];
        assert!(table.starts_with("| CONTAINER ID | IMAGE | STATUS | NAMES |\n|---|---|---|---|\n"));
        assert!(
            table.contains("| 3f2a1b9c8d7e | postgres\\:16 | Up 2 hours | db |"),
            "{table}"
        );
    }

    /// Ways a table can be invented or broken: output that fits is left as
    /// printed, prose and code with single spaces are not tables, a two-line
    /// coincidence is not believed, and surrounding lines stay raw.
    #[test]
    fn only_real_tables_that_do_not_fit_are_rewritten() {
        assert!(tables(DOCKER, 200).is_empty(), "fits: leave it alone");
        let prose = "error: could not compile `nur` due to 2 previous errors\n\
                     warning: unused variable `x` in this function body here\n\
                     note: run with RUST_BACKTRACE=1 for a backtrace now";
        assert!(tables(prose, 20).is_empty());
        assert!(
            tables("a  b\nc  d", 2).is_empty(),
            "two rows are not enough"
        );
        let mixed = format!("$ docker ps\n{DOCKER}\ndone");
        let lines: Vec<&str> = mixed.lines().collect();
        let blocks = segment(&lines, 40);
        assert_eq!(blocks.first(), Some(&Block::Raw("$ docker ps")));
        assert_eq!(blocks.last(), Some(&Block::Raw("done")));
        assert_eq!(blocks.len(), 3);
    }

    /// psql and box-drawn tables split on their separators, borders dropped,
    /// and Markdown punctuation inside a cell stays literal.
    #[test]
    fn separated_tables_drop_borders_and_escape_cells() {
        let psql = " id | name       | note\n----+------------+---------------------------\n  1 | a_b        | *not* bold, see #4\n  2 | c          | plain";
        let found = tables(psql, 30);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].contains("| 1 | a\\_b | \\*not\\* bold\\, see \\#4 |"),
            "{}",
            found[0]
        );
        let boxed = "┌────┬──────┐\n│ id │ name │\n├────┼──────┤\n│ 1  │ alpha│\n└────┴──────┘";
        let found = tables(boxed, 5);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], "| id | name |\n|---|---|\n| 1 | alpha |\n");
        let numbered = "     1|let a = b | c;\n     2|let d = e | f;\n     3|let g = h | i;";
        assert!(
            tables(numbered, 5).is_empty(),
            "numbered source is not a table"
        );
    }
}
