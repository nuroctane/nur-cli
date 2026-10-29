//! Hash-anchored edits ("hashline", after oh-my-pi's edit mode).
//!
//! `read_file` heads every read with `[path#TAG]`: a 4-hex snapshot of the
//! whole file. `hashline_edit` then changes lines by their number in that
//! snapshot instead of by retyping old text, so a model never loses an edit to
//! whitespace drift or a "string not found" loop, and spends far fewer output
//! tokens. If the file changed since the read, the tag no longer matches and
//! the edit is refused before anything is written. OMP measured the format
//! beating unified-diff patching on 14 of 16 models (Grok Code Fast 1 went from
//! 6.7% to 68.3%, Grok 4 Fast spent 61% fewer output tokens).
//!
//! The language is OMP's line subset:
//!
//! ```text
//! PUT 4.=6:      replace lines 4-6 with the + rows below
//! +new line
//! PUT <3:        insert before line 3 (<1 = file head)
//! PUT >3:        insert after line 3 (>$ = file tail)
//! CUT 8.=9 @fn   delete lines 8-9, keeping them in register @fn
//! PUT >12 @fn    paste register @fn after line 12 (no body)
//! PUT 20*:       replace the block that line 20 opens
//! PUT >20*:      insert after that block
//! ```
//!
//! Every number refers to the original snapshot, never to a line shifted by an
//! earlier hunk of the same call. A block anchor `N*` stands for line N through
//! the end of the block it opens: brackets opened on N (or a `{` alone on the
//! next line) until they close, an indented body under a line ending in `:`,
//! or a Markdown heading's section. OMP resolves blocks with tree-sitter; this
//! resolver reads only brackets, indentation and headings, so it needs no
//! grammar per language, and a line that opens none of them is refused rather
//! than guessed. File operations (`REM`, `MV`) are left to `bash`.

use super::{arg_str, resolve_path, Tool, ToolContext};
use crate::error::{NurError, Result};
use serde_json::Value;
use std::collections::HashMap;

/// The snapshot tag of `text`: 4 uppercase hex digits over the content with
/// trailing spaces, tabs and CRs stripped from every line, so a CRLF checkout
/// and an editor that trims trailing whitespace do not invalidate it.
pub fn file_tag(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            hasher.update(b"\n");
        }
        hasher.update(line.trim_end_matches([' ', '\t', '\r']).as_bytes());
    }
    let digest = hasher.finalize();
    format!("{:02X}{:02X}", digest[0], digest[1])
}

/// The `[path#TAG]` line `read_file` puts above a file's contents.
pub fn header(path: &str, text: &str) -> String {
    format!("[{}#{}]", path.replace('\\', "/"), file_tag(text))
}

/// Where a gap insertion lands, as the number of original lines before it.
type Gap = usize;

#[derive(Debug, Clone, PartialEq)]
enum Content {
    Body(Vec<String>),
    /// Paste a register: `None` is the anonymous one (the last unnamed CUT).
    Register(Option<String>),
}

#[derive(Debug, Clone, PartialEq)]
enum Op {
    /// Replace original lines `start..=end` (1-based).
    Replace {
        start: usize,
        end: usize,
        with: Content,
    },
    Insert {
        at: Gap,
        with: Content,
    },
    Cut {
        start: usize,
        end: usize,
        register: Option<String>,
    },
}

/// Parse the edit text against the file's `lines`.
fn parse(edits: &str, lines: &[&str]) -> std::result::Result<Vec<Op>, String> {
    let mut ops: Vec<Op> = Vec::new();
    // The body rows of the most recent body-taking PUT.
    let mut open_body: Option<usize> = None;
    for (n, raw) in edits.lines().enumerate() {
        let row = n + 1;
        if let Some(text) = raw.strip_prefix('+') {
            let Some(index) = open_body else {
                return Err(format!(
                    "row {row}: a `+` body row needs a `PUT ...:` header above it"
                ));
            };
            if let Op::Replace {
                with: Content::Body(body),
                ..
            }
            | Op::Insert {
                with: Content::Body(body),
                ..
            } = &mut ops[index]
            {
                body.push(text.to_string());
            }
            continue;
        }
        let line = raw.trim();
        if line.is_empty()
            || line.starts_with("*** ")
            || (line.starts_with('[') && line.ends_with(']') && line.contains('#'))
        {
            continue;
        }
        open_body = None;
        let (keyword, rest) = line
            .split_once(char::is_whitespace)
            .map(|(k, r)| (k, r.trim()))
            .unwrap_or((line, ""));
        let keyword = keyword.to_ascii_uppercase();
        let is_op = matches!(keyword.as_str(), "PUT" | "CUT" | "REM" | "MV");
        if !is_op && (line.starts_with('-') || raw.starts_with(' ')) {
            return Err(format!(
                "row {row}: `{line}` is not an operation. Body rows start with `+` and hold the \
                 final text; never send removed (`-`) or unchanged context lines"
            ));
        }
        match keyword.as_str() {
            "PUT" => {
                let (locator, register, takes_body) = split_put(rest);
                let op = if let Some(at) = gap(locator, lines, row)? {
                    let with = if takes_body {
                        Content::Body(Vec::new())
                    } else {
                        Content::Register(register)
                    };
                    Op::Insert { at, with }
                } else {
                    let (start, end) = span(locator, lines, row)?;
                    let with = if takes_body {
                        Content::Body(Vec::new())
                    } else {
                        Content::Register(Some(register.ok_or_else(|| {
                            format!(
                                "row {row}: pasting over lines needs a named register \
                                 (`PUT {start}.={end} @name`), or end the header with `:` and \
                                 give `+` rows"
                            )
                        })?))
                    };
                    Op::Replace { start, end, with }
                };
                if takes_body {
                    open_body = Some(ops.len());
                }
                ops.push(op);
            }
            "CUT" => {
                let (locator, register) = match rest.split_once('@') {
                    Some((l, r)) => (l.trim(), Some(r.trim().to_string())),
                    None => (rest.trim(), None),
                };
                let (start, end) = span(locator.trim_end_matches(':'), lines, row)?;
                ops.push(Op::Cut {
                    start,
                    end,
                    register,
                });
            }
            "REM" | "MV" => {
                return Err(format!(
                    "row {row}: `{keyword}` is not supported here; delete or move files with bash"
                ))
            }
            _ => {
                return Err(format!(
                    "row {row}: expected `PUT` or `CUT`, found `{line}`"
                ))
            }
        }
    }
    if ops.is_empty() {
        return Err("no operations: give at least one `PUT` or `CUT`".into());
    }
    Ok(ops)
}

/// `PUT` arguments: (locator, register, takes a body).
fn split_put(rest: &str) -> (&str, Option<String>, bool) {
    if let Some(locator) = rest.strip_suffix(':') {
        return (locator.trim(), None, true);
    }
    match rest.split_once('@') {
        Some((locator, name)) => (locator.trim(), Some(name.trim().to_string()), false),
        None => (rest.trim(), None, false),
    }
}

/// A gap locator (`<N`, `>N`, `>$`, `>N*` after a block), as the count of
/// original lines before it.
fn gap(locator: &str, lines: &[&str], row: usize) -> std::result::Result<Option<Gap>, String> {
    let len = lines.len();
    if let Some(rest) = locator.strip_prefix('>') {
        if rest == "$" {
            return Ok(Some(len));
        }
        if let Some(anchor) = rest.strip_suffix('*') {
            let n = line_number(anchor, len, row)?;
            return block_end(lines, n, row).map(Some);
        }
        let n = line_number(rest, len, row)?;
        return Ok(Some(n));
    }
    if let Some(rest) = locator.strip_prefix('<') {
        let rest = rest.strip_suffix('*').unwrap_or(rest);
        if rest == "1" {
            return Ok(Some(0));
        }
        let n = line_number(rest, len, row)?;
        return Ok(Some(n - 1));
    }
    Ok(None)
}

/// A line span: `N.=M` (also `N-M`, `N..M`, `N,M`), `N` for one line, or `N*`
/// for the block line N opens.
fn span(locator: &str, lines: &[&str], row: usize) -> std::result::Result<(usize, usize), String> {
    let len = lines.len();
    if let Some(anchor) = locator.strip_suffix('*') {
        let start = line_number(anchor, len, row)?;
        return Ok((start, block_end(lines, start, row)?));
    }
    let (a, b) = [".=", "..", "-", ","]
        .iter()
        .find_map(|sep| locator.split_once(sep))
        .unwrap_or((locator, locator));
    let start = line_number(a, len, row)?;
    let end = line_number(b, len, row)?;
    if end < start {
        return Err(format!("row {row}: range {start}.={end} runs backwards"));
    }
    Ok((start, end))
}

/// The last line of the block that line `start` opens (1-based).
fn block_end(lines: &[&str], start: usize, row: usize) -> std::result::Result<usize, String> {
    let opener = lines[start - 1];
    if let Some(end) = bracket_block_end(lines, start) {
        return Ok(end);
    }
    let trimmed = opener.trim_end();
    if let Some(level) = heading_level(trimmed) {
        let next = (start..lines.len())
            .find(|&i| heading_level(lines[i]).is_some_and(|other| other <= level))
            .unwrap_or(lines.len());
        return Ok(last_content_line(lines, start, next));
    }
    if trimmed.ends_with(':') {
        let indent = indent_of(opener);
        let mut end = start;
        for (i, line) in lines.iter().enumerate().skip(start) {
            if line.trim().is_empty() {
                continue;
            }
            if indent_of(line) <= indent {
                break;
            }
            end = i + 1;
        }
        if end > start {
            return Ok(end);
        }
    }
    Err(format!(
        "row {row}: line {start} opens no block (no bracket, indented body or heading starts          there); give the range `N.=M`"
    ))
}

/// Brackets opened on `start`, or by a `{` starting the next non-blank line
/// (Allman style), counted until they close at the end of a line. Closers
/// before the first opener (`} else {`) belong to the previous block.
fn bracket_block_end(lines: &[&str], start: usize) -> Option<usize> {
    let next_brace = |after: usize| {
        (after..lines.len())
            .find(|&i| !lines[i].trim().is_empty())
            .filter(|&i| lines[i].trim_start().starts_with('{'))
    };
    let mut depth = 0usize;
    let mut opened = false;
    let mut i = start - 1;
    while i < lines.len() {
        for delta in bracket_deltas(lines[i]) {
            if delta > 0 {
                depth += 1;
                opened = true;
            } else {
                depth = depth.saturating_sub(1);
            }
        }
        if i + 1 == start && depth == 0 {
            // Nothing left open on the anchor line (`void f()`): the body's
            // brace may sit below it; otherwise only a closed `{}` is a block.
            if let Some(brace) = next_brace(i + 1) {
                opened = false;
                i = brace;
                continue;
            }
            return (opened && lines[i].contains('{')).then_some(start);
        }
        if opened && depth == 0 {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

/// +1 / -1 for each bracket outside string literals and line comments.
fn bracket_deltas(line: &str) -> Vec<i8> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if c == '\\' {
                chars.next();
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '`' => quote = Some(c),
            // A char literal like '{' (a lifetime `'a` has no closing quote).
            '\'' => {
                let rest: String = chars.clone().take(3).collect();
                if rest.starts_with('\\') || rest.chars().nth(1) == Some('\'') {
                    quote = Some('\'');
                }
            }
            '/' if chars.peek() == Some(&'/') => break,
            '(' | '[' | '{' => out.push(1),
            ')' | ']' | '}' => out.push(-1),
            _ => {}
        }
    }
    out
}

fn heading_level(line: &str) -> Option<usize> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    (1..=6)
        .contains(&hashes)
        .then(|| line[hashes..].starts_with(' ').then_some(hashes))
        .flatten()
}

fn indent_of(line: &str) -> usize {
    line.chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

/// The last non-blank line from `start` up to (not including) index `next`.
fn last_content_line(lines: &[&str], start: usize, next: usize) -> usize {
    (start..next)
        .rev()
        .find(|&i| !lines[i].trim().is_empty())
        .map(|i| i + 1)
        .unwrap_or(start)
}

fn line_number(text: &str, len: usize, row: usize) -> std::result::Result<usize, String> {
    let n: usize = text
        .trim()
        .parse()
        .map_err(|_| format!("row {row}: `{text}` is not a line number"))?;
    if n == 0 || n > len {
        return Err(format!(
            "row {row}: line {n} is outside the file (1-{len}); re-read it"
        ));
    }
    Ok(n)
}

/// Apply `ops` to `lines`. Returns the new lines and, for each, whether this
/// edit wrote it (for the result excerpt).
fn apply(lines: &[&str], ops: &[Op]) -> std::result::Result<Vec<(String, bool)>, String> {
    // Spans must not overlap, and no insertion may land inside one.
    let mut spans: Vec<(usize, usize)> = ops
        .iter()
        .filter_map(|op| match op {
            Op::Replace { start, end, .. } | Op::Cut { start, end, .. } => Some((*start, *end)),
            Op::Insert { .. } => None,
        })
        .collect();
    spans.sort_unstable();
    for pair in spans.windows(2) {
        if pair[1].0 <= pair[0].1 {
            return Err(format!(
                "ranges {}.={} and {}.={} overlap; merge them into one",
                pair[0].0, pair[0].1, pair[1].0, pair[1].1
            ));
        }
    }
    for op in ops {
        if let Op::Insert { at, .. } = op {
            if let Some((s, e)) = spans.iter().find(|(s, e)| *at >= *s && *at < *e) {
                return Err(format!(
                    "an insertion after line {at} falls inside the replaced range {s}.={e}"
                ));
            }
        }
    }

    // Registers hold original lines, so their contents do not depend on order.
    let mut registers: HashMap<Option<String>, Vec<String>> = HashMap::new();
    for op in ops {
        if let Op::Cut {
            start,
            end,
            register,
        } = op
        {
            registers.insert(
                register.clone(),
                lines[start - 1..*end]
                    .iter()
                    .map(|l| l.to_string())
                    .collect(),
            );
        }
    }
    let resolve = |content: &Content| -> std::result::Result<Vec<String>, String> {
        match content {
            Content::Body(body) => Ok(body.clone()),
            Content::Register(name) => registers.get(name).cloned().ok_or_else(|| match name {
                Some(name) => format!("register @{name} is empty: CUT into it in this same call"),
                None => "nothing to paste: no unnamed CUT in this call".to_string(),
            }),
        }
    };

    let mut out: Vec<(String, bool)> = Vec::with_capacity(lines.len());
    let mut index = 0; // original lines already passed
    loop {
        for op in ops {
            if let Op::Insert { at, with } = op {
                if *at == index {
                    out.extend(resolve(with)?.into_iter().map(|l| (l, true)));
                }
            }
        }
        if index == lines.len() {
            break;
        }
        let line = index + 1;
        let starts_here = ops.iter().find(|op| match op {
            Op::Replace { start, .. } | Op::Cut { start, .. } => *start == line,
            Op::Insert { .. } => false,
        });
        match starts_here {
            Some(Op::Replace { end, with, .. }) => {
                out.extend(resolve(with)?.into_iter().map(|l| (l, true)));
                index = *end;
            }
            Some(Op::Cut { end, .. }) => index = *end,
            _ => {
                out.push((lines[index].to_string(), false));
                index += 1;
            }
        }
    }
    Ok(out)
}

/// The changed lines of `edited` with two lines of context, numbered in the
/// new file, so the model can chain the next edit without a re-read.
fn excerpt(edited: &[(String, bool)], max_rows: usize) -> String {
    let changed: Vec<usize> = edited
        .iter()
        .enumerate()
        .filter(|(_, (_, new))| *new)
        .map(|(i, _)| i)
        .collect();
    if changed.is_empty() {
        return "(only deletions)".into();
    }
    let mut show = vec![false; edited.len()];
    for i in changed {
        for slot in show
            .iter_mut()
            .take((i + 3).min(edited.len()))
            .skip(i.saturating_sub(2))
        {
            *slot = true;
        }
    }
    let mut out = String::new();
    let mut rows = 0;
    let mut gap = false;
    for (i, (text, _)) in edited.iter().enumerate() {
        if !show[i] {
            gap = true;
            continue;
        }
        if gap && !out.is_empty() {
            out.push_str("   ...\n");
        }
        gap = false;
        if rows == max_rows {
            out.push_str("   ... (excerpt cut; read_file for the rest)\n");
            break;
        }
        out.push_str(&format!("{:>6}|{}\n", i + 1, text));
        rows += 1;
    }
    out
}

pub struct HashlineEdit;

impl Tool for HashlineEdit {
    fn name(&self) -> &str {
        "hashline_edit"
    }

    fn description(&self) -> &str {
        "Edit an existing file by line number instead of retyping old text. Copy `tag` from the \
         `[path#TAG]` header of your latest read_file (or hashline_edit) of the file; if the file \
         changed since, the edit is refused and you re-read. `edits` is one operation per header \
         line, numbers from that read (never shifted by earlier operations in the same call):\n\
         PUT N.=M:  replace lines N-M (N.=N for one) with the `+` rows that follow\n\
         PUT <N:  insert before line N (<1 = file head);  PUT >N:  after N (>$ = file tail)\n\
         CUT N.=M  delete lines N-M;  CUT N.=M @name  delete and keep them in register @name\n\
         PUT >N @name  paste a register (no body);  PUT N.=M @name  replace a range with it\n\
         N* anywhere a range or >N goes means the block line N opens (to its closing bracket, \
         the end of its indented body, or the next heading): PUT 12*:  CUT 12*  PUT >12*:\n\
         Body rows are `+` then the exact final text with its indentation (`+` alone is a blank \
         line; a line that itself starts with + or - is `++...` / `+-...`). Never send removed or \
         unchanged lines. Keep ranges tight and split changes that are not adjacent. The result \
         gives the new tag and the changed lines."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File to edit (workspace-relative)"},
                "tag": {"type": "string", "description": "4-hex snapshot tag from the latest read_file header"},
                "edits": {"type": "string", "description": "PUT / CUT operations, one header per line, + body rows"}
            },
            "required": ["path", "tag", "edits"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let path = arg_str(args, "path")?;
        let tag = arg_str(args, "tag")?
            .trim()
            .trim_start_matches('#')
            .to_ascii_uppercase();
        let edits = arg_str(args, "edits")?;
        let full = resolve_path(&ctx.cwd, &path)?;
        if !full.is_file() {
            return Err(NurError::Tool(format!(
                "{path} does not exist; create files with write_file"
            )));
        }
        super::guard_unchanged_since_read(&full)?;
        let bytes = std::fs::read(&full)
            .map_err(|e| NurError::Tool(format!("read {}: {e}", full.display())))?;
        // A lossy decode would write U+FFFD over every byte that is not UTF-8.
        let text = String::from_utf8(bytes).map_err(|_| {
            NurError::Tool(format!(
                "{path} is not UTF-8 text; hashline_edit would corrupt its encoding"
            ))
        })?;
        let current = file_tag(&text);
        if current != tag {
            return Err(NurError::Tool(format!(
                "stale tag {tag}: {path} is now #{current}, so it changed since that read. \
                 read_file it again and use the new line numbers and tag"
            )));
        }

        let (bom, body) = match text.strip_prefix('\u{feff}') {
            Some(rest) => ("\u{feff}", rest),
            None => ("", text.as_str()),
        };
        let crlf = body.matches("\r\n").count();
        let eol = if crlf * 2 > body.matches('\n').count() {
            "\r\n"
        } else {
            "\n"
        };
        let trailing_newline = body.ends_with('\n') || body.is_empty();
        let mut lines: Vec<&str> = body
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l))
            .collect();
        if body.ends_with('\n') || body.is_empty() {
            lines.pop();
        }

        let ops = parse(&edits, &lines).map_err(NurError::Tool)?;
        let edited = apply(&lines, &ops).map_err(NurError::Tool)?;
        let mut updated = String::from(bom);
        updated.push_str(
            &edited
                .iter()
                .map(|(l, _)| l.as_str())
                .collect::<Vec<_>>()
                .join(eol),
        );
        if trailing_newline && !edited.is_empty() {
            updated.push_str(eol);
        }
        if updated == text {
            return Ok(format!(
                "no change: the operations reproduce {path} exactly. {}",
                header(&path, &text)
            ));
        }
        if let Some(staged) =
            super::commit_file_write(ctx, &path, &full, &updated, "hashline_edit")?
        {
            return Ok(staged);
        }
        Ok(format!(
            "edited {path}: {} operation(s), {} -> {} lines\n{}\n{}",
            ops.len(),
            lines.len(),
            edited.len(),
            header(&path, &updated),
            excerpt(&edited, 60)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, edits: &str) -> std::result::Result<String, String> {
        let lines: Vec<&str> = text.lines().collect();
        let ops = parse(edits, &lines)?;
        Ok(apply(&lines, &ops)?
            .into_iter()
            .map(|(l, _)| l)
            .collect::<Vec<_>>()
            .join("\n"))
    }

    const FILE: &str = "a\nb\nc\nd\ne";

    /// Every operation, with numbers from the original snapshot even when an
    /// earlier operation in the same call changed the line count.
    #[test]
    fn operations_use_original_line_numbers() {
        assert_eq!(
            run(FILE, "PUT 2.=3:\n+B\n+C\n+C2").unwrap(),
            "a\nB\nC\nC2\nd\ne"
        );
        assert_eq!(run(FILE, "PUT 2:\n+B").unwrap(), "a\nB\nc\nd\ne");
        assert_eq!(run(FILE, "PUT <1:\n+head").unwrap(), "head\na\nb\nc\nd\ne");
        assert_eq!(run(FILE, "PUT >$:\n+tail").unwrap(), "a\nb\nc\nd\ne\ntail");
        assert_eq!(run(FILE, "PUT >2:\n+x").unwrap(), "a\nb\nx\nc\nd\ne");
        assert_eq!(run(FILE, "PUT <2:\n+x").unwrap(), "a\nx\nb\nc\nd\ne");
        assert_eq!(run(FILE, "CUT 2.=4").unwrap(), "a\ne");
        // Line 5 is still "e" after the earlier insert and cut.
        assert_eq!(
            run(FILE, "PUT >1:\n+x\n+y\nCUT 2.=3\nPUT 5.=5:\n+E").unwrap(),
            "a\nx\ny\nd\nE"
        );
        // Blank lines and literal leading +/- in bodies.
        assert_eq!(
            run(FILE, "PUT 1:\n+\n++plus\n+-minus").unwrap(),
            "\n+plus\n-minus\nb\nc\nd\ne"
        );
    }

    #[test]
    fn registers_move_lines_within_one_call() {
        assert_eq!(
            run(FILE, "CUT 1.=2 @top\nPUT >$ @top").unwrap(),
            "c\nd\ne\na\nb"
        );
        assert_eq!(run(FILE, "CUT 5\nPUT <1").unwrap(), "e\na\nb\nc\nd");
        assert_eq!(run(FILE, "CUT 1 @x\nPUT 5.=5 @x").unwrap(), "b\nc\nd\na");
        assert!(run(FILE, "PUT >1 @nothing")
            .unwrap_err()
            .contains("@nothing"));
        assert!(run(FILE, "PUT 2.=3")
            .unwrap_err()
            .contains("named register"));
    }

    /// The mistakes models make, each refused with a message that says what
    /// to do instead, and none of them half-applied.
    #[test]
    fn malformed_edits_are_refused_with_guidance() {
        let cases = [
            ("PUT 2.=4:\n+x\nPUT 4.=5:\n+y", "overlap"),
            ("PUT 2.=4:\n+x\nPUT >2:\n+y", "inside the replaced range"),
            ("PUT 7:\n+x", "outside the file"),
            ("PUT 3.=2:\n+x", "backwards"),
            ("+orphan", "needs a `PUT"),
            ("PUT 2:\n-b\n+B", "never send removed"),
            ("PUT 2*:\n+x", "opens no block"),
            ("REM", "not supported"),
            ("FIX 2", "expected `PUT` or `CUT`"),
            ("", "no operations"),
        ];
        for (edits, needle) in cases {
            let err = run(FILE, edits).unwrap_err();
            assert!(err.contains(needle), "{edits:?} -> {err}");
        }
    }

    /// Ways a block anchor can resolve wrong: stop at the first closer, count
    /// a bracket inside a string or comment, include the `} else {` closer's
    /// block, miss an Allman brace, run an indented body past its dedent, or
    /// swallow the next heading or trailing blank lines.
    #[test]
    fn block_anchors_cover_the_block_the_line_opens() {
        let rust = "fn a() {\n    let s = \"}\"; // }\n    if x {\n        y();\n    } else {\n        z();\n    }\n}\nfn b() {}";
        assert_eq!(run(rust, "CUT 1*").unwrap(), "fn b() {}");
        assert_eq!(
            run(rust, "PUT 5*:\n+    } else {}").unwrap(),
            "fn a() {\n    let s = \"}\"; // }\n    if x {\n        y();\n    } else {}\n}\nfn b() {}"
        );
        assert_eq!(
            run(rust, "PUT >1*:\n+fn c() {}").unwrap().lines().nth(8),
            Some("fn c() {}")
        );
        assert_eq!(
            run(rust, "PUT 9*:\n+fn b2() {}").unwrap().lines().last(),
            Some("fn b2() {}")
        );
        let allman = "void f()\n{\n  g();\n}\nint x;";
        assert_eq!(run(allman, "CUT 1*").unwrap(), "int x;");
        let lifetimes = "fn f<'a>(s: &'a str) -> char {\n    '{'\n}\nend";
        assert_eq!(run(lifetimes, "CUT 1*").unwrap(), "end");
        let python = "def f():\n    a = 1\n\n    return a\n\ndef g():\n    pass";
        assert_eq!(run(python, "CUT 1*").unwrap(), "\ndef g():\n    pass");
        let markdown = "# Top\n## One\ntext\n\n## Two\nmore";
        assert_eq!(run(markdown, "CUT 2*").unwrap(), "# Top\n\n## Two\nmore");
        assert_eq!(run(markdown, "CUT 1*").unwrap(), "");
        assert!(run("let x = 1;\nlet y = 2;", "CUT 1*")
            .unwrap_err()
            .contains("opens no block"));
    }

    /// The tag ignores trailing whitespace and CRs, and changes with content.
    #[test]
    fn the_tag_tracks_content_not_line_endings() {
        let tag = file_tag("a\nb\n");
        assert_eq!(tag.len(), 4);
        assert!(tag
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase()));
        assert_eq!(file_tag("a\r\nb  \r\n"), tag);
        assert_ne!(file_tag("a\nc\n"), tag);
        assert_eq!(header("src\\x.rs", "a\nb\n"), format!("[src/x.rs#{tag}]"));
    }

    /// End to end through the tool: a stale tag writes nothing; a fresh one
    /// keeps CRLF, the BOM and the final newline, and returns the new tag.
    #[test]
    fn the_tool_refuses_stale_tags_and_keeps_the_file_format() {
        let dir = std::env::temp_dir().join(format!("nur-hashline-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.txt");
        let original = "\u{feff}one\r\ntwo\r\nthree\r\n";
        std::fs::write(&file, original).unwrap();
        let ctx = ToolContext::default_for_test(&dir);
        let call = |tag: &str, edits: &str| {
            HashlineEdit.execute(
                &serde_json::json!({"path": "x.txt", "tag": tag, "edits": edits}),
                &ctx,
            )
        };

        let stale = call("0000", "PUT 2:\n+TWO").unwrap_err().to_string();
        assert!(stale.contains("stale tag 0000"), "{stale}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);

        let tag = file_tag(original);
        let out = call(&tag.to_ascii_lowercase(), "PUT 2:\n+TWO").unwrap();
        let written = std::fs::read_to_string(&file).unwrap();
        assert_eq!(written, "\u{feff}one\r\nTWO\r\nthree\r\n");
        assert!(
            out.contains(&format!("[x.txt#{}]", file_tag(&written))),
            "{out}"
        );
        assert!(out.contains("     2|TWO"), "{out}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
