//! `lux learn` — the language's own teaching material, built into the binary.
//!
//! The content lives in `learn-lux.md` and is baked in at compile time, so the
//! reference a learner holds always matches the binary's behaviour and needs no
//! network or stray file. That one file is also the test corpus: every example
//! here is real lux that the suite runs and converts.
//!
//! The file is a sequence of *topics*, each read at two levels. A topic's
//! *card* is one screen — a stable id, a title, a one-sentence concept, a
//! runnable example, and an optional `try:` experiment that nudges the learner
//! back to the editor. An earned, optional *more* page carries the deeper why,
//! the universal name for the idea, and reason-annotated cross-references. The
//! id is the join key — what you type (`lux learn match`), what a guided lesson
//! lists, and what an error message points at. Everything `lux learn` shows is
//! some traversal of this one list of topics, plus two pages of furniture: the
//! `basics` skeleton and the graduation `ladder`.

const DOC: &str = include_str!("../learn-lux.md");

/// Guided lessons: short, ordered sequences of topics for a first read. Each is
/// a few topics on one theme, walked in order. The names are disjoint from the
/// topic ids, so one argument resolves unambiguously to a lesson or a topic.
const PATHS: &[(&str, &[&str])] = &[
    ("start", &["hello", "variables", "numbers", "strings"]),
    ("logic", &["booleans", "if", "while"]),
    ("data", &["arrays", "for", "functions", "scope"]),
    ("types", &["structs", "enums", "match"]),
    ("safety", &["option", "result"]),
];

/// A single learnable idea. The card is always present; `more` is earned.
pub struct Topic {
    pub id: String,
    pub title: String,
    pub concept: String,
    pub example: String,
    pub try_hint: Option<String>,
    pub more: Option<More>,
}

/// The earned second level of a topic: the deeper prose, plus any
/// reason-annotated cross-references to related topics.
pub struct More {
    pub prose: String,
    pub see: Vec<See>,
}

/// One cross-reference: a topic id and the reason a learner would follow it.
pub struct See {
    pub id: String,
    pub reason: String,
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
    let (card, more_src) = match body.split_once("<!-- more -->") {
        Some((c, m)) => (c, Some(m)),
        None => (body, None),
    };
    let (title, concept, example, try_hint) = parse_card(card);
    Topic {
        id,
        title,
        concept,
        example,
        try_hint,
        more: more_src.map(parse_more),
    }
}

/// Parse a topic card: title, concept paragraph, example, and `try:` hint.
fn parse_card(body: &str) -> (String, String, String, Option<String>) {
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

    // try: hint — the first blockquote line after the example, if any. A card's
    // only footer is an experiment; cross-references live on the more page.
    let mut try_hint = None;
    for line in lines.by_ref() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if let Some(rest) = l.strip_prefix("> ") {
            let rest = rest.trim();
            let hint = rest.strip_prefix("try:").map(str::trim).unwrap_or(rest);
            try_hint = Some(hint.to_string());
        }
        break;
    }

    (title, concept.join(" "), example.join("\n"), try_hint)
}

/// Parse a more page: the deeper prose, then an optional `> see:` block whose
/// entries read `id — reason`, separated by `·`.
fn parse_more(body: &str) -> More {
    let mut prose = Vec::new();
    let mut quote = String::new();
    let mut in_quote = false;
    for line in body.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("> ") {
            in_quote = true;
            if !quote.is_empty() {
                quote.push(' ');
            }
            quote.push_str(rest.trim());
        } else if !in_quote && !t.is_empty() {
            prose.push(t.to_string());
        }
    }

    let mut see = Vec::new();
    if let Some(rest) = quote.strip_prefix("see:") {
        for piece in rest.split('·') {
            let p = piece.trim();
            if p.is_empty() {
                continue;
            }
            let (id, reason) = match p.split_once('—') {
                Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
                None => (p.to_string(), String::new()),
            };
            see.push(See { id, reason });
        }
    }

    More {
        prose: prose.join(" "),
        see,
    }
}

// --- rendering -------------------------------------------------------------

const WIDTH: usize = 76;

/// A topic's default view: the one-screen card, plus a pointer to its more page
/// when one exists so the deeper level is discoverable.
fn render_card(t: &Topic) -> String {
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
    if let Some(hint) = &t.try_hint {
        out.push('\n');
        out.push_str(&wrap(&plain(&format!("try: {}", hint)), WIDTH));
        out.push('\n');
    }
    if t.more.is_some() {
        out.push_str(&format!("\nmore: lux learn {} --more\n", t.id));
    }
    out
}

/// A topic's earned second level: the deeper prose and its cross-references.
fn render_more(t: &Topic, m: &More) -> String {
    let mut out = String::new();
    out.push_str(&plain(&t.title));
    out.push_str(" — more\n\n");
    out.push_str(&wrap(&plain(&m.prose), WIDTH));
    out.push('\n');
    if !m.see.is_empty() {
        out.push_str("\nsee also:\n");
        for s in &m.see {
            if s.reason.is_empty() {
                out.push_str(&format!("  lux learn {}\n", s.id));
            } else {
                let lead = format!("  lux learn {} — ", s.id);
                let wrapped = wrap_indent(&plain(&s.reason), WIDTH, &lead, "    ");
                out.push_str(&wrapped);
                out.push('\n');
            }
        }
    }
    out
}

/// Strip the markdown a learner shouldn't see on a terminal — code-span
/// backticks and emphasis asterisks. The examples themselves are never run
/// through this; only the surrounding prose.
fn plain(s: &str) -> String {
    s.chars().filter(|c| *c != '`' && *c != '*').collect()
}

/// Drop leading `#` heading markers from each line, so a furniture section
/// reads as a plain title on a terminal rather than raw markdown.
fn dehead(s: &str) -> String {
    s.lines()
        .map(|l| l.trim_start_matches('#').trim_start_matches(' '))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Reflow any markdown tables in a block into aligned columns for the terminal,
/// passing every other line through untouched. The markdown source stays a real
/// table — this only changes how it looks on a screen, not on GitHub.
fn format_tables(text: &str) -> String {
    let mut out = String::new();
    let mut block: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with('|') {
            block.push(line);
        } else {
            if !block.is_empty() {
                out.push_str(&render_table(&block));
                block.clear();
            }
            out.push_str(line);
            out.push('\n');
        }
    }
    if !block.is_empty() {
        out.push_str(&render_table(&block));
    }
    out
}

/// One markdown table → space-aligned columns with a rule under the header.
fn render_table(lines: &[&str]) -> String {
    let rows: Vec<Vec<String>> = lines
        .iter()
        .filter(|l| !is_rule_row(l))
        .map(|l| split_cells(l))
        .collect();
    if rows.is_empty() {
        return String::new();
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut widths = vec![0usize; cols];
    for r in &rows {
        for (i, c) in r.iter().enumerate() {
            widths[i] = widths[i].max(c.chars().count());
        }
    }

    let gap = "  ";
    let mut out = String::new();
    for (ri, r) in rows.iter().enumerate() {
        let mut line = String::new();
        for (i, c) in r.iter().enumerate() {
            line.push_str(c);
            if i + 1 < r.len() {
                let pad = widths[i].saturating_sub(c.chars().count());
                line.push_str(&" ".repeat(pad));
                line.push_str(gap);
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
        if ri == 0 {
            let width: usize = widths.iter().sum::<usize>() + gap.len() * cols.saturating_sub(1);
            out.push_str(&"─".repeat(width));
            out.push('\n');
        }
    }
    out
}

/// A markdown table's `|---|---|` separator row: cells of only dashes.
fn is_rule_row(line: &str) -> bool {
    split_cells(line)
        .iter()
        .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-'))
}

/// Split a markdown table row into trimmed cells, dropping the outer pipes.
fn split_cells(line: &str) -> Vec<String> {
    line.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect()
}

/// Greedy word-wrap to a column width, so a long concept stays one screen.
fn wrap(text: &str, width: usize) -> String {
    wrap_indent(text, width, "", "")
}

/// Word-wrap with a one-time lead on the first line and a hanging indent on the
/// rest — used for `see also:` entries where the reason trails the topic name.
fn wrap_indent(text: &str, width: usize, lead: &str, hang: &str) -> String {
    let mut out = String::new();
    let mut line = String::from(lead);
    let mut has_word = false;
    for word in text.split_whitespace() {
        if has_word && line.len() + 1 + word.len() > width {
            out.push_str(&line);
            out.push('\n');
            line = String::from(hang);
            has_word = false;
        }
        if has_word {
            line.push(' ');
        }
        line.push_str(word);
        has_word = true;
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

/// A `## ...` furniture section of the learner region, from its heading to the
/// next top-level heading or the end of the region.
fn section(anchor: &str) -> String {
    let region = learner_region();
    let start = match region.find(anchor) {
        Some(i) => i,
        None => return String::new(),
    };
    let after = &region[start + anchor.len()..];
    let end = after.find("\n## ").unwrap_or(after.len());
    format!("{}{}", anchor, &after[..end]).trim_end().to_string()
}

/// The procedural-language skeleton: the pieces every language shares and where
/// to learn each in lux. The inward companion to the graduation ladder.
fn basics_page() -> String {
    section("## The shape every language shares")
}

/// The graduation table beneath the topics — where each lux feature lands in
/// Rust, Swift, and Go.
fn ladder() -> String {
    section("## Where each feature takes you")
}

/// `lux learn basics`: the shape every procedural language shares.
pub fn basics() -> String {
    let mut out = format_tables(&plain(&dehead(&basics_page())));
    out.push('\n');
    out
}

/// `lux learn` with no argument: a short landing page — the guided lessons, how
/// to look up one topic, how to go deeper, and how to read the whole thing.
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

    out.push_str("\ngo deeper on any topic:\n");
    out.push_str("  lux learn <topic> --more\n");

    out.push_str("\nthe bigger picture:\n");
    out.push_str("  lux learn basics    the shape every language shares\n");
    out.push_str("  lux learn tour      the whole language, top to bottom\n");
    out
}

/// The whole language top to bottom: intro, the basics skeleton, every card,
/// then the ladder.
pub fn tour() -> String {
    let mut out = String::new();
    out.push_str(&intro());
    out.push_str("\n\n");
    out.push_str(&rule());
    out.push('\n');
    out.push_str(&format_tables(&plain(&dehead(&basics_page()))));
    out.push_str("\n\n");
    out.push_str(&rule());
    out.push('\n');
    for t in topics() {
        out.push_str(&render_card(&t));
        out.push('\n');
        out.push_str(&rule());
        out.push('\n');
    }
    let ladder = ladder();
    if !ladder.is_empty() {
        out.push_str(&format_tables(&plain(&dehead(&ladder))));
        out.push('\n');
    }
    out
}

/// Resolve one `lux learn` argument: a guided lesson name or a topic id (card).
pub fn lookup(name: &str) -> Option<String> {
    if let Some((_, ids)) = PATHS.iter().find(|(n, _)| *n == name) {
        return Some(render_path(name, ids));
    }
    topics().iter().find(|t| t.id == name).map(render_card)
}

/// Resolve `lux learn <topic> --more`: the topic's deeper page, or its card
/// with a note when the topic has no deeper page.
pub fn topic_more(name: &str) -> Option<String> {
    let t = topics().into_iter().find(|t| t.id == name)?;
    Some(match &t.more {
        Some(m) => render_more(&t, m),
        None => {
            let mut s = render_card(&t);
            s.push_str("\n(this topic has no deeper page — the card above is the whole story.)\n");
            s
        }
    })
}

fn render_path(name: &str, ids: &[&str]) -> String {
    let all = topics();
    let mut out = String::new();
    out.push_str(&format!("lesson: {}\n\n", name));
    out.push_str(&rule());
    out.push('\n');
    for id in ids {
        if let Some(t) = all.iter().find(|t| &t.id == id) {
            out.push_str(&render_card(t));
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
