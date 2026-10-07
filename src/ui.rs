//! Terminal output. On a terminal: color and a symbol per line. Anywhere else
//! (pipes, CI logs, `NO_COLOR`): plain `bvm:` lines that scripts can grep.
//! `anstream` decides, honoring `NO_COLOR`, `CLICOLOR_FORCE` and `TERM=dumb`,
//! and turns on escape-sequence support in Windows consoles.

use anstream::ColorChoice;
use anstyle::{AnsiColor, Style};
use std::fmt::Display;
use std::path::Path;

const BOLD: Style = Style::new().bold();
const DIM: Style = Style::new().dimmed();
const GREEN: Style = AnsiColor::Green.on_default();
const CYAN: Style = AnsiColor::Cyan.on_default();
const YELLOW: Style = AnsiColor::Yellow.on_default();
const RED: Style = AnsiColor::Red.on_default();

/// Styles for one stream; they render as plain text when that stream gets no
/// color.
#[derive(Clone, Copy)]
pub struct Paint {
    pub on: bool,
}

fn colored(choice: ColorChoice) -> bool {
    !matches!(choice, ColorChoice::Never)
}

/// Styles for messages (stderr).
pub fn err() -> Paint {
    Paint {
        on: colored(anstream::AutoStream::choice(&std::io::stderr())),
    }
}

/// Styles for results (stdout).
pub fn out() -> Paint {
    Paint {
        on: colored(anstream::AutoStream::choice(&std::io::stdout())),
    }
}

impl Paint {
    fn wrap(self, style: Style, text: impl Display) -> String {
        if self.on {
            format!("{style}{text}{style:#}")
        } else {
            text.to_string()
        }
    }
    pub fn bold(self, text: impl Display) -> String {
        self.wrap(BOLD, text)
    }
    pub fn dim(self, text: impl Display) -> String {
        self.wrap(DIM, text)
    }
    pub fn green(self, text: impl Display) -> String {
        self.wrap(GREEN.bold(), text)
    }
    pub fn yellow(self, text: impl Display) -> String {
        self.wrap(YELLOW.bold(), text)
    }
    pub fn red(self, text: impl Display) -> String {
        self.wrap(RED.bold(), text)
    }
    /// A Bun or bvm version.
    pub fn version(self, text: impl Display) -> String {
        self.wrap(GREEN.bold(), text)
    }
    /// Something to type.
    pub fn cmd(self, text: impl Display) -> String {
        self.wrap(CYAN, text)
    }
    /// A file or directory, with the home directory shown as `~`.
    pub fn path(self, path: &Path) -> String {
        self.bold(tilde(path))
    }
}

/// `path` with a leading home directory written as `~`.
pub fn tilde(path: &Path) -> String {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    if let Some(home) = home.filter(|home| !home.is_empty())
        && let Ok(rest) = path.strip_prefix(&home)
    {
        let sep = std::path::MAIN_SEPARATOR;
        return if rest.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~{sep}{}", rest.display())
        };
    }
    path.display().to_string()
}

/// Windows' classic console font has no ✓; it has √.
fn check_mark() -> &'static str {
    let modern = !cfg!(windows)
        || std::env::var_os("WT_SESSION").is_some()
        || std::env::var_os("TERM_PROGRAM").is_some();
    if modern { "✓" } else { "√" }
}

/// On a terminal, `code` spans lose their backticks and turn cyan.
fn commands(paint: Paint, message: impl Display) -> String {
    let text = message.to_string();
    if !paint.on || text.matches('`').count() < 2 {
        return text;
    }
    let mut result = String::new();
    for (index, part) in text.split('`').enumerate() {
        if index % 2 == 1 {
            result.push_str(&paint.cmd(part));
        } else {
            result.push_str(part);
        }
    }
    result
}

fn line(symbol: impl FnOnce(Paint) -> String, message: impl Display) {
    let paint = err();
    if paint.on {
        anstream::eprintln!("  {} {}", symbol(paint), commands(paint, message));
    } else {
        anstream::eprintln!("bvm: {message}");
    }
}

/// Something finished.
pub fn done(message: impl Display) {
    line(|p| p.green(check_mark()), message);
}

/// Something is under way.
pub fn working(message: impl Display) {
    line(|p| p.cmd("→"), message);
}

/// Worth knowing, nothing wrong.
pub fn note(message: impl Display) {
    line(|p| p.dim("•"), message);
}

pub fn warn(message: impl Display) {
    let paint = err();
    anstream::eprintln!(
        "{} {}",
        if paint.on {
            paint.yellow("warning:")
        } else {
            "bvm: warning:".into()
        },
        commands(paint, message)
    );
}

/// A failure, and optionally what to do about it.
pub fn error(message: impl Display) {
    let paint = err();
    anstream::eprintln!(
        "{} {}",
        if paint.on {
            paint.red("error:")
        } else {
            "bvm: error:".into()
        },
        commands(paint, message)
    );
}

/// A free-form line (a blank line, a heading, an indented command).
pub fn say(message: impl Display) {
    anstream::eprintln!("{message}");
}
