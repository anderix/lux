//! `lux learn` — the language's own teaching material, built into the binary.
//!
//! The content lives in `learn-lux.md` and is baked in at compile time, so the
//! reference a learner holds always matches the binary's behaviour and needs no
//! network or stray file. That one file is also the test corpus: every example
//! here is real lux that the suite runs and converts.
//!
//! The file is a sequence of *topics*. Each topic is one screen: a stable id, a
//! title, a one-sentence concept, a runnable example, and an optional footer
//! that nudges the learner back to the editor. The id is the join key — it is
//! what you type (`lux learn match`), what a guided lesson lists, and what an
//! error message will point at. Everything `lux learn` shows is some traversal
//! of this one list of topics.

const DOC: &str = include_str!("../learn-lux.md");

/// Guided lessons: short, ordered sequences of topics for a first read. Each is
/// a few topics on one theme, walked in order. The names are disjoint from the
/// topic ids, so one argument resolves unambiguously to a lesson or a topic.
const PATHS: &[(&str, &[&str])] = &[
    ("start", &["hello", "variables", "numbers", "strings"]),
    ("logic", &["booleans", "if", "while"]),
    ("data", &["arrays", "for", "functions"]),
    ("types", &["structs", "enums", "match"]),
    ("safety", &["option", "result"]),
];

/// A single learnable idea: one screen of the language.
pub struct Topic {
    pub id: String,
    pub title: String,
    pub concept: String,
    pub example: String,
    pub footer: Option<Footer>,
}

/// The optional last line of a topic. `Try` is an experiment to type into the
/// editor; `See` points at related topics. Earned, never required.
pub enum Footer {
    Try(String),
    See(Vec<String>),
}

// --- parsing ---------------------------------------------------------------

/// The slice of the document that `lux learn` draws from — everything above the
/// `learn:end` marker. The notes below it are for the maintainer, not learners.
fn learner_region() -> &'static str {
    DOC.split("<!-- learn:end -->").next().unwrap_or(DOC)
}

/// Parse every `<!-- topic: id -->` block in document order.
pub fn topics() -> Vec<Topic> {
    let region = learner_region();
    let marker = "<!-- topic:";
    let mut out = Vec::new();
    let mut rest = region;
    while let Some(pos) = rest.find(marker) {
        let after = &rest[pos + marker.len()..];
        let id_end = after.find("-->").unwrap_or(0);
        let id = after[..id_end].trim().to_string();
        let body_start = after.find('\n').map(|i| i + 1).unwrap_or(after.len());
        let body_region = &after[body_start..];
        let next = body_region.find(marker).unwrap_or(body_region.len());
        out.push(parse_topic(id, &body_region[..next]));
        rest = &body_region[next..];
    }
    out
}

fn parse_topic(id: String, body: &str) -> Topic {
    let mut lines = body.lines().peekable();

    // Title: the `## ...` heading.
    let mut title = String::new();
    for line in lines.by_ref() {
        if let Some(rest) = line.trim_start().strip_prefix("## ") {
            title = rest.trim().to_string();
            break;
        }
    }

    // Concept: the prose paragraph up to the example fence.
    let mut concept = Vec::new();
    while let Some(line) = lines.peek() {
        if line.trim_start().starts_with("```") {
            break;
        }
        let l = line.trim();
        if !l.is_empty() {
            concept.push(l.to_string());
        }
        lines.next();
    }

    // Example: the lines inside the ```lux fence.
    lines.next(); // consume the opening fence
    let mut example = Vec::new();
    for line in lines.by_ref() {
        if line.trim_start().starts_with("```") {
            break;
        }
        example.push(line);
    }

    // Footer: the first blockquote line after the example, if any.
    let mut footer = None;
    for line in lines.by_ref() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if let Some(rest) = l.strip_prefix("> ") {
            footer = parse_footer(rest.trim());
        }
        break;
    }

    Topic {
        id,
        title,
        concept: concept.join(" "),
        example: example.join("\n"),
        footer,
    }
}

fn parse_footer(s: &str) -> Option<Footer> {
    if let Some(rest) = s.strip_prefix("try:") {
        Some(Footer::Try(rest.trim().to_string()))
    } else if let Some(rest) = s.strip_prefix("see:") {
        let ids = rest
            .split([',', '·'])
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect();
        Some(Footer::See(ids))
    } else {
        Some(Footer::Try(s.to_string()))
    }
}

// --- rendering -------------------------------------------------------------

const WIDTH: usize = 76;

fn render_topic(t: &Topic) -> String {
    let mut out = String::new();
    out.push_str(&plain(&t.title));
    out.push_str("\n\n");
    out.push_str(&wrap(&plain(&t.concept), WIDTH));
    out.push_str("\n\n");
    for line in t.example.lines() {
        if line.is_empty() {
            out.push('\n');
        } else {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
        }
    }
    if let Some(footer) = &t.footer {
        out.push('\n');
        match footer {
            Footer::Try(s) => out.push_str(&wrap(&plain(&format!("try: {}", s)), WIDTH)),
            Footer::See(ids) => {
                let refs: Vec<String> =
                    ids.iter().map(|i| format!("lux learn {}", i)).collect();
                out.push_str(&format!("see also: {}", refs.join(" · ")));
            }
        }
        out.push('\n');
    }
    out
}

/// Strip the markdown a learner shouldn't see on a terminal — code-span
/// backticks and emphasis asterisks. The examples themselves are never run
/// through this; only the surrounding prose.
fn plain(s: &str) -> String {
    s.chars().filter(|c| *c != '`' && *c != '*').collect()
}

/// Greedy word-wrap to a column width, so a long concept stays one screen.
fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            out.push_str(&line);
            out.push('\n');
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    out.push_str(&line);
    out
}

/// The intro paragraphs at the top of the file, above the first topic.
fn intro() -> String {
    let head = learner_region()
        .split("<!-- topic:")
        .next()
        .unwrap_or_default();
    head.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// The first sentence of the intro, unwrapped and stripped of markdown — a
/// one-line description for the landing page.
fn tagline() -> String {
    let para = intro().split("\n\n").next().unwrap_or_default().to_string();
    let unwrapped = para.split_whitespace().collect::<Vec<_>>().join(" ");
    let plain = plain(&unwrapped);
    match plain.find(". ") {
        Some(i) => plain[..=i].trim().to_string(),
        None => plain,
    }
}

/// The graduation table beneath the topics — part of the full tour, but not a
/// jumpable topic of its own.
fn ladder() -> String {
    let anchor = "## Where each feature takes you";
    match learner_region().find(anchor) {
        Some(i) => learner_region()[i..].trim_end().to_string(),
        None => String::new(),
    }
}

/// `lux learn` with no argument: a short landing page — the guided lessons, how
/// to look up one topic, and how to read the whole thing.
pub fn menu() -> String {
    let topics = topics();

    let mut out = String::new();
    out.push_str("lux learn — the language, one short topic at a time\n\n");
    let tagline = tagline();
    if !tagline.is_empty() {
        out.push_str(&wrap(&tagline, WIDTH));
        out.push_str("\n\n");
    }

    out.push_str("guided lessons — read each short topic, then go write code:\n");
    for (name, ids) in PATHS {
        out.push_str(&format!("  lux learn {:<8} {}\n", name, ids.join(", ")));
    }

    out.push_str("\nlook up one idea:\n");
    out.push_str("  lux learn <topic>\n");
    let ids: Vec<&str> = topics.iter().map(|t| t.id.as_str()).collect();
    out.push_str(&wrap_ids(&ids, "    "));

    out.push_str("\nread it all:\n");
    out.push_str("  lux learn tour\n");
    out
}

/// The whole language top to bottom: intro, every topic, then the ladder.
pub fn tour() -> String {
    let mut out = String::new();
    out.push_str(&intro());
    out.push_str("\n\n");
    for t in topics() {
        out.push_str(&render_topic(&t));
        out.push('\n');
        out.push_str(&rule());
        out.push('\n');
    }
    let ladder = ladder();
    if !ladder.is_empty() {
        out.push_str(&plain(&ladder));
        out.push('\n');
    }
    out
}

/// Resolve one `lux learn` argument: a guided lesson name or a topic id.
pub fn lookup(name: &str) -> Option<String> {
    if let Some((_, ids)) = PATHS.iter().find(|(n, _)| *n == name) {
        return Some(render_path(name, ids));
    }
    topics()
        .iter()
        .find(|t| t.id == name)
        .map(render_topic)
}

fn render_path(name: &str, ids: &[&str]) -> String {
    let all = topics();
    let mut out = String::new();
    out.push_str(&format!("lesson: {}\n\n", name));
    out.push_str(&rule());
    out.push('\n');
    for id in ids {
        if let Some(t) = all.iter().find(|t| &t.id == id) {
            out.push_str(&render_topic(t));
            out.push('\n');
            out.push_str(&rule());
            out.push('\n');
        }
    }
    if let Some(pos) = PATHS.iter().position(|(n, _)| *n == name) {
        if let Some((next, _)) = PATHS.get(pos + 1) {
            out.push_str(&format!("next lesson: lux learn {}\n", next));
        } else {
            out.push_str("that's the last lesson — `lux learn tour` shows it all.\n");
        }
    }
    out
}

fn rule() -> String {
    "─".repeat(60) + "\n"
}

/// Wrap a list of ids to a readable width, each line under the given indent.
fn wrap_ids(ids: &[&str], indent: &str) -> String {
    let mut out = String::new();
    let mut line = String::from(indent);
    for id in ids {
        if line.len() + id.len() + 1 > 72 && line.trim() != "" {
            out.push_str(line.trim_end());
            out.push('\n');
            line = String::from(indent);
        }
        line.push_str(id);
        line.push(' ');
    }
    if line.trim() != "" {
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// The guided lessons, exposed so a test can check every member resolves.
pub fn paths() -> &'static [(&'static str, &'static [&'static str])] {
    PATHS
}
