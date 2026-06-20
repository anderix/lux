//! `lux magic` — spells for things you want to do now.
//!
//! Where `lux learn` is a concept ladder ("what is X?"), magic is task indexed
//! ("how do I X?"). A spell is a small, runnable program that already works,
//! plus a trail to the `lux learn` topics that explain the ideas it uses. Spells
//! are allowed to run ahead of where a reader has climbed the learn ladder —
//! that is the point: a working shape now, with an honest signpost to where the
//! trick is explained. The same spell reads as plain lux once its trail is
//! climbed; the magic was never magic.
//!
//! The content lives in `magic-lux.md`, baked in at compile time, and is also
//! the test corpus: every spell is real lux the suite runs and translates.

const DOC: &str = include_str!("../magic-lux.md");

/// One spell: the join key `id`, a short title, the "how do I…?" question, a
/// runnable example, and the learn topics that explain it.
pub struct Spell {
    pub id: String,
    pub title: String,
    pub question: String,
    pub example: String,
    pub trail: Vec<String>,
}

const WIDTH: usize = 76;

// --- parsing ---------------------------------------------------------------

/// Parse every `<!-- spell: id -->` block in document order.
pub fn spells() -> Vec<Spell> {
    let marker = "<!-- spell:";
    let mut out = Vec::new();
    let mut rest = DOC;
    while let Some(pos) = rest.find(marker) {
        let after = &rest[pos + marker.len()..];
        let id_end = after.find("-->").unwrap_or(0);
        let id = after[..id_end].trim().to_string();
        let body_start = after.find('\n').map(|i| i + 1).unwrap_or(after.len());
        let body = &after[body_start..];
        let next = body.find(marker).unwrap_or(body.len());
        out.push(parse_spell(id, &body[..next]));
        rest = &body[next..];
    }
    out
}

fn parse_spell(id: String, body: &str) -> Spell {
    let mut lines = body.lines().peekable();

    let mut title = String::new();
    for line in lines.by_ref() {
        if let Some(rest) = line.trim_start().strip_prefix("## ") {
            title = rest.trim().to_string();
            break;
        }
    }

    // The question: the prose paragraph up to the example fence.
    let mut question = Vec::new();
    while let Some(line) = lines.peek() {
        if line.trim_start().starts_with("```") {
            break;
        }
        let l = line.trim();
        if !l.is_empty() {
            question.push(l.to_string());
        }
        lines.next();
    }

    // The example: the lines inside the ```lux fence.
    lines.next(); // consume the opening fence
    let mut example = Vec::new();
    for line in lines.by_ref() {
        if line.trim_start().starts_with("```") {
            break;
        }
        example.push(line);
    }

    // The trail: the `> trail:` blockquote, topic ids split on `·`.
    let mut trail = Vec::new();
    for line in lines.by_ref() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("> ") {
            if let Some(ids) = rest.trim().strip_prefix("trail:") {
                for piece in ids.split('·') {
                    let p = piece.trim();
                    if !p.is_empty() {
                        trail.push(p.to_string());
                    }
                }
            }
            break;
        }
    }

    Spell {
        id,
        title,
        question: question.join(" "),
        example: example.join("\n"),
        trail,
    }
}

/// The short summary after the `id — ` in a spell's title, for the menu line.
fn summary(title: &str) -> String {
    match title.split_once('—') {
        Some((_, rest)) => rest.trim().to_string(),
        None => title.to_string(),
    }
}

/// The intro paragraph above the first spell — its first sentence is the
/// landing-page tagline. The file opens with a maintainer comment, so the intro
/// is the prose between that comment's close and the first spell.
fn intro() -> String {
    let after_comment = match DOC.split_once("-->") {
        Some((_, rest)) => rest,
        None => DOC,
    };
    after_comment
        .split("<!-- spell:")
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

// --- rendering -------------------------------------------------------------

/// A spell's view: the question, the runnable shape, and its trail.
fn render(s: &Spell) -> String {
    let mut out = String::new();
    out.push_str(&plain(&s.title));
    out.push_str("\n\n");
    out.push_str(&wrap(&plain(&s.question), WIDTH));
    out.push_str("\n\n");
    for line in s.example.lines() {
        if line.is_empty() {
            out.push('\n');
        } else {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
        }
    }
    if !s.trail.is_empty() {
        let follow: Vec<String> = s.trail.iter().map(|t| format!("lux learn {}", t)).collect();
        out.push('\n');
        out.push_str(&wrap(
            &format!("how it works: {}", follow.join(", ")),
            WIDTH,
        ));
        out.push('\n');
    }
    out
}

/// `lux magic` with no argument: the spells on offer, each a "how do I…?".
pub fn menu() -> String {
    let spells = spells();
    let mut out = String::new();
    out.push_str("lux magic — spells for things you want to do now\n\n");
    let tagline = tagline();
    if !tagline.is_empty() {
        out.push_str(&wrap(&tagline, WIDTH));
        out.push_str("\n\n");
    }
    out.push_str("cast a spell:\n");
    let width = spells.iter().map(|s| s.id.len()).max().unwrap_or(0);
    for s in &spells {
        out.push_str(&format!(
            "  lux magic {:<width$}  {}\n",
            s.id,
            summary(&s.title),
            width = width
        ));
    }
    out.push_str("\neach spell ends with how it works — a trail into `lux learn`,\n");
    out.push_str("where the ideas it uses are explained.\n");
    out
}

/// The first sentence of the intro, stripped of markdown.
fn tagline() -> String {
    let para = intro().split("\n\n").next().unwrap_or_default().to_string();
    let unwrapped = para.split_whitespace().collect::<Vec<_>>().join(" ");
    let plain = plain(&unwrapped);
    match plain.find(". ") {
        Some(i) => plain[..=i].trim().to_string(),
        None => plain,
    }
}

/// Resolve `lux magic <id>` to its rendered spell.
pub fn lookup(name: &str) -> Option<String> {
    spells().iter().find(|s| s.id == name).map(render)
}

/// Strip the markdown a reader shouldn't see on a terminal — code-span
/// backticks and emphasis asterisks. Examples are never run through this.
fn plain(s: &str) -> String {
    s.chars().filter(|c| *c != '`' && *c != '*').collect()
}

/// Greedy word-wrap to a column width.
fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut line = String::new();
    let mut has_word = false;
    for word in text.split_whitespace() {
        if has_word && line.len() + 1 + word.len() > width {
            out.push_str(&line);
            out.push('\n');
            line = String::new();
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
