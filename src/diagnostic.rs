//! Source positions and error reporting.
//!
//! A `Span` is a byte range into the source. Every token and every ast node
//! carries one, so when something goes wrong we can underline the offending
//! text. `report` renders an error the way a teaching language should: the
//! line, a caret under the problem, and an optional note suggesting the fix.

/// A half-open byte range `[start, end)` into the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }

    /// A span stretching from the start of `self` to the end of `other`.
    pub fn to(self, other: Span) -> Span {
        Span::new(self.start, other.end)
    }
}

/// An error with a message, the span it happened at, an optional note, and an
/// optional trail to a `lux learn` topic — a `(topic, lure)` pair, where the
/// lure is a one-line hint at why the idea is worth following.
#[derive(Debug, Clone)]
pub struct LuxError {
    pub message: String,
    pub span: Span,
    pub note: Option<String>,
    pub learn: Option<(&'static str, &'static str)>,
}

impl LuxError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        LuxError {
            message: message.into(),
            span,
            note: None,
            learn: None,
        }
    }

    /// Attach a hint about how to fix the problem.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Open a trail to the `lux learn` topic that teaches this, with a one-line
    /// lure hinting at why it is worth following. Set it only where there is a
    /// clean topic that genuinely covers the mistake; a self-evident fix needs
    /// only a note, not a trail.
    pub fn with_learn(mut self, topic: &'static str, lure: &'static str) -> Self {
        self.learn = Some((topic, lure));
        self
    }
}

/// Print an error to stderr with the source line and a caret under the span.
pub fn report(filename: &str, source: &str, err: &LuxError) {
    let start = err.span.start.min(source.len());
    let line_start = source[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = source[start..]
        .find('\n')
        .map(|i| start + i)
        .unwrap_or(source.len());
    let line_no = source[..line_start].bytes().filter(|&b| b == b'\n').count() + 1;
    let col = start - line_start + 1;
    let line_text = &source[line_start..line_end];

    let caret_end = err.span.end.min(line_end);
    let caret_len = caret_end.saturating_sub(start).max(1);

    eprintln!("error: {}", err.message);
    eprintln!("  --> {}:{}:{}", filename, line_no, col);

    let gutter = format!(" {} | ", line_no);
    let blank: String = gutter.chars().map(|_| ' ').collect();
    eprintln!("{}{}", gutter, line_text);
    eprintln!("{}{}{}", blank, " ".repeat(col - 1), "^".repeat(caret_len));

    if let Some(note) = &err.note {
        eprintln!("note: {}", note);
    }
    if let Some((topic, lure)) = &err.learn {
        eprintln!("help: `lux learn {}` — {}", topic, lure);
    }
}
