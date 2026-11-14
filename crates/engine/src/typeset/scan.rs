//! A small reader over LaTeX source: control words, `{…}` groups, `[…]` options and `$…$`, with
//! braces balanced and every step bounded by the input's length.

pub(crate) struct Scan {
    cs: Vec<char>,
    pub(crate) i: usize,
}

impl Scan {
    pub(crate) fn new(src: &str) -> Self {
        Scan { cs: src.chars().take(super::MAX_SOURCE).collect(), i: 0 }
    }

    /// The cursor as a byte offset into the source.
    pub(crate) fn byte_offset(&self) -> usize {
        self.cs.get(..self.i.min(self.cs.len())).unwrap_or(&[]).iter().map(|c| c.len_utf8()).sum()
    }

    pub(crate) fn at_end(&self) -> bool {
        self.i >= self.cs.len()
    }

    pub(crate) fn peek(&self) -> Option<char> {
        self.cs.get(self.i).copied()
    }

    pub(crate) fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        self.i += 1;
        c
    }

    pub(crate) fn eat(&mut self, c: char) -> bool {
        self.ws();
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    /// Skips white space and `%` comments.
    pub(crate) fn ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.i += 1;
            } else if c == '%' {
                while self.peek().is_some_and(|c| c != '\n') {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
    }

    /// The command at the cursor (after white space), without its backslash: a control word
    /// (`infer`) or a control symbol (`\\`, `,`). `None` (nothing read) when there is none.
    pub(crate) fn command(&mut self) -> Option<String> {
        self.ws();
        if self.peek() != Some('\\') {
            return None;
        }
        self.i += 1;
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        if self.i == start {
            return self.bump().map(|c| c.to_string());
        }
        Some(self.cs.get(start..self.i).unwrap_or(&[]).iter().collect())
    }

    /// The text of a `{…}` group (after white space), braces balanced and escaped braces kept.
    pub(crate) fn group(&mut self) -> Option<String> {
        self.ws();
        if self.peek() != Some('{') {
            return None;
        }
        self.i += 1;
        Some(self.balanced('}'))
    }

    /// The text of a `[…]` option, if there is one.
    pub(crate) fn opt(&mut self) -> Option<String> {
        self.ws();
        if self.peek() != Some('[') {
            return None;
        }
        self.i += 1;
        Some(self.balanced(']'))
    }

    /// A command argument: a group, or one token (`\infer1`'s `1`, `\hypo A`'s `A`).
    pub(crate) fn arg(&mut self) -> Option<String> {
        self.ws();
        match self.peek()? {
            '{' => self.group(),
            '\\' => {
                let start = self.i;
                self.command()?;
                Some(self.cs.get(start..self.i).unwrap_or(&[]).iter().collect())
            }
            _ => self.bump().map(|c| c.to_string()),
        }
    }

    /// Text up to `close` at brace depth 0 (which is consumed), or to the end.
    fn balanced(&mut self, close: char) -> String {
        let start = self.i;
        let mut depth = 0u32;
        while let Some(c) = self.peek() {
            match c {
                '\\' => {
                    // An escaped character (`\{`, `\]`) never opens, closes or ends anything.
                    self.i += 2;
                    continue;
                }
                '{' => depth = depth.saturating_add(1),
                '}' if depth > 0 => depth -= 1,
                c if c == close && depth == 0 => break,
                _ => {}
            }
            self.i += 1;
        }
        let end = self.i.min(self.cs.len());
        self.i = end + 1;
        self.cs.get(start..end).unwrap_or(&[]).iter().collect()
    }

    /// Math in `$…$` at the cursor (after white space), if any.
    pub(crate) fn dollar(&mut self) -> Option<String> {
        self.ws();
        if self.peek() != Some('$') {
            return None;
        }
        self.i += 1;
        Some(self.balanced('$'))
    }

    /// Whether a digit (an `\infer` arity) comes next, bare or braced.
    /// An `\infer` arity: one bare digit, or a braced number (`{10}`; ebproof reads the arity
    /// as an argument, so ten or more premises need the braces). Nothing is read otherwise.
    pub(crate) fn digit_next(&mut self) -> Option<usize> {
        self.ws();
        match self.peek() {
            Some(d) if d.is_ascii_digit() => {
                self.i += 1;
                d.to_digit(10).map(|d| d as usize)
            }
            Some('{') => {
                let save = self.i;
                let n = self.group().filter(|g| (1..=3).contains(&g.trim().len())).and_then(|g| g.trim().parse::<usize>().ok());
                if n.is_none() {
                    self.i = save;
                }
                n
            }
            _ => None,
        }
    }
}

/// `s` split at the top-level (brace depth 0) separators `seps`; a control-word separator such as
/// `\and` must not run into a longer word.
pub(crate) fn split_top(s: &str, seps: &[&str]) -> Vec<String> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0u32;
    let mut i = 0;
    'outer: while let Some(&c) = cs.get(i) {
        if depth == 0 {
            for sep in seps {
                let sc: Vec<char> = sep.chars().collect();
                let matches = cs.get(i..i + sc.len()) == Some(&sc[..]);
                let word_sep = sc.last().is_some_and(|c| c.is_ascii_alphabetic());
                let ends_word = !word_sep || !cs.get(i + sc.len()).is_some_and(|c| c.is_ascii_alphabetic());
                if matches && ends_word {
                    out.push(std::mem::take(&mut cur));
                    i += sc.len();
                    continue 'outer;
                }
            }
        }
        match c {
            '\\' => {
                cur.push(c);
                if let Some(&n) = cs.get(i + 1) {
                    cur.push(n);
                }
                i += 2;
                continue;
            }
            '{' => depth = depth.saturating_add(1),
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        cur.push(c);
        i += 1;
    }
    out.push(cur);
    out
}
