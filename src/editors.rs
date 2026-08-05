//! Editor syntax-highlighting integration.
//!
//! lux ships highlighting for a few editors as small files under `editors/` in
//! the repo. They are embedded here so the installed binary carries its own copy
//! — the same trick `lux learn` uses for its reference — and `lux editors
//! highlighting` writes them into the right per-user config directories. Nothing here
//! needs the network or root: every path lives under the user's own home.
//!
//! `lux update` never writes these files. It only reads the state to print a
//! tip, so updating the binary can't surprise anyone by rewriting an editor
//! config — least of all the nano colours, which are meant to be hand-tuned.

use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Command, Stdio};

// The highlighting files, embedded from the repo so the binary is self-contained.
// Each editor belongs to a platform, so its file ships only in the build that can
// use it — the Unix editors on Unix, Notepad++ on Windows.
#[cfg(unix)]
const GTKSOURCEVIEW_LANG: &str = include_str!("../editors/gtksourceview/lux.lang");
#[cfg(unix)]
const VIM_SYNTAX: &str = include_str!("../editors/vim/syntax/lux.vim");
#[cfg(unix)]
const VIM_FTDETECT: &str = include_str!("../editors/vim/ftdetect/lux.vim");
#[cfg(unix)]
const VIM_FTPLUGIN: &str = include_str!("../editors/vim/ftplugin/lux.vim");
#[cfg(unix)]
const NANO_NANORC: &str = include_str!("../editors/nano/lux.nanorc");
#[cfg(windows)]
const NOTEPADPP_UDL: &str = include_str!("../editors/notepad++/lux.xml");

// The one line nano needs in ~/.nanorc to load lux's highlighting.
const NANO_INCLUDE: &str = "include \"~/.nano/lux.nanorc\"";

/// One file lux places for an editor: where it goes and what goes in it.
struct EditorFile {
    path: PathBuf,
    body: &'static str,
}

/// An editor integration: a human name, whether the editor is on this machine,
/// the files that make up its highlighting, and — nano only — a config file it
/// shares rather than owns, into which one `include` line is added.
struct Integration {
    name: &'static str,
    present: bool,
    files: Vec<EditorFile>,
    nanorc_include: Option<PathBuf>,
}

impl Integration {
    /// True when every file is already on disk (and, for nano, the include line
    /// is present) — i.e. highlighting is set up, whatever version it is.
    fn installed(&self) -> bool {
        let files_there = self.files.iter().all(|f| f.path.exists());
        let include_there = match &self.nanorc_include {
            Some(rc) => has_include(rc),
            None => true,
        };
        files_there && include_there
    }

    /// Write whatever isn't already current, creating parent directories as
    /// needed. Returns the display paths of the files it actually wrote; an empty
    /// list means everything was already up to date.
    fn write(&self) -> std::io::Result<Vec<String>> {
        let mut changed = Vec::new();
        for f in &self.files {
            if read_to_string(&f.path).as_deref() == Some(f.body) {
                continue; // identical already — leave it, and any local edits, alone
            }
            if let Some(parent) = f.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&f.path, f.body)?;
            changed.push(display(&f.path));
        }
        if let Some(rc) = &self.nanorc_include
            && !has_include(rc)
        {
            append_include(rc)?;
            changed.push(display(rc));
        }
        Ok(changed)
    }
}

/// Every editor lux knows how to highlight for on this platform, in the order
/// they're reported — or None when the directory they'd live under can't be found.
///
/// The set is platform-split: Unix has the editors that read per-user config from
/// $HOME, Windows has Notepad++. They don't overlap, so each build only carries
/// the integrations it can act on.
#[cfg(unix)]
fn integrations() -> Option<Vec<Integration>> {
    let home = home()?;
    Some(vec![
        Integration {
            name: "Neovim",
            present: on_path("nvim"),
            files: vim_files(&home.join(".config/nvim")),
            nanorc_include: None,
        },
        // Classic Vim, but only when `vim` isn't Neovim wearing the old name —
        // otherwise this would write ~/.vim files nvim never reads.
        Integration {
            name: "Vim",
            present: on_path("vim") && !vim_is_nvim(),
            files: vim_files(&home.join(".vim")),
            nanorc_include: None,
        },
        Integration {
            name: "nano",
            present: on_path("nano"),
            files: vec![EditorFile {
                path: home.join(".nano/lux.nanorc"),
                body: NANO_NANORC,
            }],
            nanorc_include: Some(home.join(".nanorc")),
        },
        // GtkSourceView 5, the engine behind GNOME Text Editor. gedit is NOT covered:
        // it forked GtkSourceView (`libgedit-gtksourceview`) and reads its own
        // language-specs path, so the upstream .lang here never reaches it.
        Integration {
            name: "GNOME Text Editor",
            present: on_path("gnome-text-editor"),
            files: vec![EditorFile {
                path: home.join(".local/share/gtksourceview-5/language-specs/lux.lang"),
                body: GTKSOURCEVIEW_LANG,
            }],
            nanorc_include: None,
        },
    ])
}

/// On Windows, one editor: Notepad++. Its per-user config lives under %APPDATA%,
/// and dropping a UDL file into `userDefineLangs\` is the whole integration —
/// Notepad++ loads every file there at startup. "Present" is that config folder
/// existing, which it does once Notepad++ has run even once.
#[cfg(windows)]
fn integrations() -> Option<Vec<Integration>> {
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from)?;
    let npp = appdata.join("Notepad++");
    Some(vec![Integration {
        name: "Notepad++",
        present: npp.exists(),
        files: vec![EditorFile {
            path: npp.join("userDefineLangs").join("lux.xml"),
            body: NOTEPADPP_UDL,
        }],
        nanorc_include: None,
    }])
}

#[cfg(unix)]
fn vim_files(root: &Path) -> Vec<EditorFile> {
    vec![
        EditorFile {
            path: root.join("syntax/lux.vim"),
            body: VIM_SYNTAX,
        },
        EditorFile {
            path: root.join("ftdetect/lux.vim"),
            body: VIM_FTDETECT,
        },
        EditorFile {
            path: root.join("ftplugin/lux.vim"),
            body: VIM_FTPLUGIN,
        },
    ]
}

/// `lux editors`: report which editors are here and whether highlighting is in.
pub fn report() -> String {
    let Some(list) = integrations() else {
        return no_home();
    };
    let mut lines = Vec::new();
    for it in list {
        let state = if !it.present {
            "not found"
        } else if it.installed() {
            "highlighting installed"
        } else {
            "found — run `lux editors highlighting`"
        };
        lines.push(format!("  {:<26}{}", it.name, state));
    }
    format!(
        "lux editor syntax highlighting\n\n{}\n\n\
         `lux editors highlighting` writes highlighting for every editor found above.",
        lines.join("\n")
    )
}

/// `lux editors highlighting`: write the highlighting for every editor that's here.
pub fn install() -> String {
    let Some(list) = integrations() else {
        return no_home();
    };
    let mut lines = Vec::new();
    for it in list {
        if !it.present {
            continue;
        }
        let line = match it.write() {
            Ok(wrote) if wrote.is_empty() => format!("  {} — already current", it.name),
            Ok(wrote) => format!("  {} — wrote {}", it.name, wrote.join(", ")),
            Err(e) => format!("  {} — could not install: {}", it.name, e),
        };
        lines.push(line);
    }
    if lines.is_empty() {
        return no_editors_found();
    }
    format!(
        "Installed lux syntax highlighting:\n\n{}\n\n\
         Open a .lux file in any of them to see it. \
         Highlighting only — nothing completes or corrects.",
        lines.join("\n")
    )
}

/// A one-line tip for `lux update` to print after refreshing the binary, or
/// nothing when there's no editor to mention. `update` never writes these files
/// itself — it only points at `lux editors highlighting`.
pub fn nudge() -> Option<String> {
    let present: Vec<Integration> = integrations()?.into_iter().filter(|i| i.present).collect();
    if present.is_empty() {
        return None;
    }
    if present.iter().any(|i| i.installed()) {
        Some(
            "Your editor highlighting may be a version behind — \
             `lux editors highlighting` refreshes it."
                .to_string(),
        )
    } else {
        let names: Vec<&str> = present.iter().map(|i| i.name).collect();
        Some(format!(
            "Tip: `lux editors highlighting` adds lux syntax highlighting for {}.",
            names.join(", ")
        ))
    }
}

// --- small helpers ---------------------------------------------------------

/// The user's home directory — $HOME on Unix, %USERPROFILE% on Windows. Used to
/// abbreviate paths for display; the Unix integrations also root their config here.
fn home() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).map(PathBuf::from)
}

fn no_home() -> String {
    if cfg!(windows) {
        "Couldn't find your %APPDATA% directory.".to_string()
    } else {
        "Couldn't find your home directory (is $HOME set?).".to_string()
    }
}

/// The message when `install` finds nothing to write for — named per platform,
/// since the editors lux looks for differ.
#[cfg(unix)]
fn no_editors_found() -> String {
    "No supported editors found on your PATH \
     (looked for nvim, vim, nano, gnome-text-editor)."
        .to_string()
}

#[cfg(windows)]
fn no_editors_found() -> String {
    "Notepad++ doesn't appear to be installed \
     (looked for its config folder under %APPDATA%)."
        .to_string()
}

/// Is `cmd` an executable on PATH? Asked by running its `--version`, which every
/// editor here answers without opening a window or waiting on input.
#[cfg(unix)]
fn on_path(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// On many systems `vim` is a symlink to Neovim; its `--version` says so. We
/// check to avoid installing classic-Vim files that Neovim would never load.
#[cfg(unix)]
fn vim_is_nvim() -> bool {
    Command::new("vim")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("NVIM"))
        .unwrap_or(false)
}

fn read_to_string(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok()
}

/// Does ~/.nanorc already pull in lux's highlighting?
fn has_include(rc: &Path) -> bool {
    read_to_string(rc)
        .map(|s| s.contains("lux.nanorc"))
        .unwrap_or(false)
}

/// Add lux's include line to ~/.nanorc, preserving whatever is already there.
fn append_include(rc: &Path) -> std::io::Result<()> {
    if let Some(parent) = rc.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut existing = read_to_string(rc).unwrap_or_default();
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(NANO_INCLUDE);
    existing.push('\n');
    std::fs::write(rc, existing)
}

/// Show a path as ~/… when it's under home, so output reads the way a person
/// would write the path themselves.
fn display(p: &Path) -> String {
    match home().and_then(|h| p.strip_prefix(&h).ok().map(|r| r.to_path_buf())) {
        Some(rel) => format!("~/{}", rel.display()),
        None => p.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::interpreter::BUILTINS;
    use crate::lexer::KEYWORDS;

    /// Is this token a lux identifier — a highlighted name — rather than a scrap of
    /// regex or markup that happened to sit inside a highlight construct?
    fn is_ident(tok: &str) -> bool {
        let mut chars = tok.chars();
        matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
            && tok.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }

    /// The names a GtkSourceView `.lang` highlights: every `<keyword>NAME</keyword>`,
    /// across its keyword, boolean, type, constructor and builtin contexts.
    fn gtk_names(s: &str) -> HashSet<&str> {
        s.split("<keyword>")
            .skip(1)
            .filter_map(|part| part.split("</keyword>").next())
            .collect()
    }

    /// The names a Notepad++ UDL highlights: the contents of its `Keywords1..8`
    /// groups — keywords, types and built-ins live in 1, 2 and 3. The Comments,
    /// Numbers and Delimiters groups have other names, so they're never reached.
    fn npp_names(s: &str) -> HashSet<&str> {
        let mut out = HashSet::new();
        for part in s.split("<Keywords name=\"Keywords").skip(1) {
            if let Some((_, rest)) = part.split_once('>') {
                let content = rest.split("</Keywords>").next().unwrap_or("");
                out.extend(content.split_whitespace().filter(|t| is_ident(t)));
            }
        }
        out
    }

    /// The names a Vim syntax file highlights: the words on each `syntax keyword
    /// lux…` line, past the group name. The `syntax match`/`syntax region` lines are
    /// left out on purpose — that's where Vim's own `contains=` directive sits, which
    /// is not the lux built-in of the same name.
    fn vim_names(s: &str) -> HashSet<&str> {
        let mut out = HashSet::new();
        for line in s.lines() {
            if let Some(rest) = line.trim_start().strip_prefix("syntax keyword ") {
                out.extend(rest.split_whitespace().skip(1));
            }
        }
        out
    }

    /// The names a nano rc highlights: the identifiers inside each `\<( … )\>`
    /// alternation on a `color` line. The number, string and comment `color` lines
    /// carry no such alternation of identifiers, so nothing from them slips in.
    fn nano_names(s: &str) -> HashSet<&str> {
        let mut out = HashSet::new();
        for line in s.lines() {
            if !line.trim_start().starts_with("color ") {
                continue;
            }
            let mut rest = line;
            while let Some(open) = rest.find('(') {
                rest = &rest[open + 1..];
                let Some(close) = rest.find(')') else { break };
                for tok in rest[..close].split('|') {
                    if is_ident(tok) {
                        out.insert(tok);
                    }
                }
                rest = &rest[close + 1..];
            }
        }
        out
    }

    /// Every built-in and every keyword is highlighted in every editor asset that
    /// carries a name list — the check that would have failed the moment 0.18.0 added
    /// contains, replace and split and left all four files a release behind (#63).
    /// It asserts two lists agree and holds no number to go stale, unlike the retired
    /// builtin-count claim; and it reads each file's highlight constructs rather than
    /// scanning for a substring, so Vim's `contains=@Spell` can't stand in for the
    /// built-in `contains`. The files are embedded here directly, unguarded by
    /// platform, so the pin covers Notepad++ on a Unix build too — the production
    /// embeds are per-platform, drift isn't.
    #[test]
    fn every_editor_asset_highlights_every_builtin_and_keyword() {
        let gtk = include_str!("../editors/gtksourceview/lux.lang");
        let npp = include_str!("../editors/notepad++/lux.xml");
        let vim = include_str!("../editors/vim/syntax/lux.vim");
        let nano = include_str!("../editors/nano/lux.nanorc");
        let assets = [
            ("gtksourceview/lux.lang", gtk_names(gtk)),
            ("notepad++/lux.xml", npp_names(npp)),
            ("vim/syntax/lux.vim", vim_names(vim)),
            ("nano/lux.nanorc", nano_names(nano)),
        ];
        for (file, names) in &assets {
            for b in BUILTINS {
                assert!(names.contains(b), "{file} is missing built-in `{b}`");
            }
            for (kw, _) in KEYWORDS {
                assert!(names.contains(kw), "{file} is missing keyword `{kw}`");
            }
        }
    }

    // A scratch HOME that cleans itself up, named per-test so parallel runs
    // never share a directory.
    #[cfg(unix)]
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lux-editors-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[cfg(unix)]
    #[test]
    fn writes_files_and_adds_the_nano_include_exactly_once() {
        let home = scratch("nano");
        let it = Integration {
            name: "nano",
            present: true,
            files: vec![EditorFile {
                path: home.join(".nano/lux.nanorc"),
                body: NANO_NANORC,
            }],
            nanorc_include: Some(home.join(".nanorc")),
        };

        // First install writes the file and the include line.
        let first = it.write().unwrap();
        assert_eq!(first.len(), 2, "should write the rc file and the include");
        assert!(it.installed());
        let rc = std::fs::read_to_string(home.join(".nanorc")).unwrap();
        assert_eq!(rc.matches(NANO_INCLUDE).count(), 1);

        // Second install is a no-op: file identical, include already there.
        let second = it.write().unwrap();
        assert!(second.is_empty(), "a second install should change nothing");
        let rc = std::fs::read_to_string(home.join(".nanorc")).unwrap();
        assert_eq!(rc.matches(NANO_INCLUDE).count(), 1, "include stays single");

        let _ = std::fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[test]
    fn a_hand_edited_nanorc_keeps_its_other_lines() {
        let home = scratch("nanorc-keep");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(".nanorc"), "set tabsize 4\n").unwrap();
        let it = Integration {
            name: "nano",
            present: true,
            files: vec![EditorFile {
                path: home.join(".nano/lux.nanorc"),
                body: NANO_NANORC,
            }],
            nanorc_include: Some(home.join(".nanorc")),
        };
        it.write().unwrap();
        let rc = std::fs::read_to_string(home.join(".nanorc")).unwrap();
        assert!(rc.contains("set tabsize 4"), "existing settings survive");
        assert!(rc.contains(NANO_INCLUDE), "include was added");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[test]
    fn all_five_embedded_files_are_present_and_named_lux() {
        assert!(GTKSOURCEVIEW_LANG.contains("id=\"lux\""));
        assert!(VIM_SYNTAX.contains("luxKeyword"));
        assert!(VIM_FTDETECT.contains("*.lux"));
        assert!(VIM_FTPLUGIN.contains("commentstring"));
        assert!(NANO_NANORC.contains("syntax lux"));
    }

    #[cfg(windows)]
    #[test]
    fn notepadpp_udl_is_present_and_named_lux() {
        assert!(NOTEPADPP_UDL.contains("name=\"lux\""));
        assert!(NOTEPADPP_UDL.contains("ext=\"lux\""));
        // the three keyword groups the styles colour
        assert!(NOTEPADPP_UDL.contains("Keywords1"));
        assert!(NOTEPADPP_UDL.contains("Keywords2"));
        assert!(NOTEPADPP_UDL.contains("Keywords3"));
        // foreground-only styling is what makes it read on light and dark themes
        assert!(NOTEPADPP_UDL.contains("colorStyle=\"1\""));
    }
}
