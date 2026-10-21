/// Longest body the server accepts, in characters after normalization.
pub const MAX_BODY_LEN: usize = 4000;

/// What the server's TrimStrings strips besides Unicode whitespace.
fn is_trimmed(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\0' | '\u{180e}' | '\u{feff}' | '\u{200b}' | '\u{200e}')
}

/// A body the way the server stores it: trimmed, line ends unified, and runs of blank lines outside fenced
/// code collapsed.
pub fn normalize_like_server(body: &str) -> String {
    let unified = body
        .trim_matches(is_trimmed)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut out = String::with_capacity(unified.len());
    let mut prose_start = 0;
    for (start, end) in fenced_blocks(&unified) {
        out.push_str(&collapse_blank_lines(&unified[prose_start..start]));
        out.push_str(&unified[start..end]);
        prose_start = end;
    }
    out.push_str(&collapse_blank_lines(&unified[prose_start..]));
    out
}

/// The length the server checks, in code points of the normalized body.
pub fn body_len(body: &str) -> usize {
    normalize_like_server(body).chars().count()
}

fn is_opening_fence(line: &str) -> bool {
    let ticks = line.bytes().take_while(|&b| b == b'`').count();
    ticks >= 3 && !line[ticks..].contains('`')
}

fn is_closing_fence(line: &str) -> bool {
    let rest = line.trim_start_matches('`');
    line.len() - rest.len() >= 3 && rest.bytes().all(|b| b == b' ' || b == b'\t')
}

/// The server's pattern is greedy about skipping a body, so the line right after the opening one closes the
/// block only when no later line does.
fn closing_line(lines: &[(usize, &str)], open: usize) -> Option<usize> {
    (open + 2..lines.len())
        .find(|&j| is_closing_fence(lines[j].1))
        .or_else(|| {
            (open + 1 < lines.len() && is_closing_fence(lines[open + 1].1)).then_some(open + 1)
        })
}

/// Byte ranges of the fenced blocks, from the start of the opening line to the end of the closing one.
/// A fence that never closes is not listed, which leaves it to be treated as prose.
fn fenced_blocks(text: &str) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    let mut offset = 0;
    for line in text.split('\n') {
        lines.push((offset, line));
        offset += line.len() + 1;
    }
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let (start, line) = lines[i];
        if is_opening_fence(line)
            && let Some(close) = closing_line(&lines, i)
        {
            blocks.push((start, lines[close].0 + lines[close].1.len()));
            i = close + 1;
            continue;
        }
        i += 1;
    }
    blocks
}

/// `(?:[ \t]*\n){3,}` to `\n\n`, by hand to keep a regex engine out of the dependencies.
fn collapse_blank_lines(prose: &str) -> String {
    let bytes = prose.as_bytes();
    let mut out = String::with_capacity(prose.len());
    let mut i = 0;
    while i < bytes.len() {
        let (mut end, mut runs) = (i, 0);
        loop {
            let mut k = end;
            while k < bytes.len() && matches!(bytes[k], b' ' | b'\t') {
                k += 1;
            }
            if k < bytes.len() && bytes[k] == b'\n' {
                runs += 1;
                end = k + 1;
            } else {
                break;
            }
        }
        if runs >= 3 {
            out.push_str("\n\n");
            i = end;
        } else {
            // A run starting later inside this stretch would have been counted from here, so the stretch goes out whole; rescanning each space would be quadratic.
            let skip = prose[i..]
                .find(|c| !matches!(c, ' ' | '\t'))
                .unwrap_or(prose.len() - i)
                .max(prose[i..].chars().next().map_or(1, char::len_utf8));
            out.push_str(&prose[i..i + skip]);
            i += skip;
        }
    }
    out
}
