//! Importing `.vscode/launch.json` (D008).
//!
//! Read-only, always. VS Code owns this file; lazydap borrows the
//! configurations in it so that dropping lazydap into an existing repository
//! gives you the debug targets that repository already had.
//!
//! # Why the parser is hand-rolled
//!
//! `launch.json` is JSONC — JSON with `//` and `/* */` comments and trailing
//! commas — which `serde_json` will not read. The alternative to the stripper
//! below is a dependency (`json5`, `jsonc-parser`, `serde_jsonrc`), and the
//! dependency budget is small enough that a hundred lines of scanner is the
//! cheaper side of that trade. See D046.
//!
//! The scanner is string-aware, which is the entire difficulty: `"http://x"`
//! and `{"sep": ","}` both contain sequences that mean something outside a
//! string and nothing inside one. Everything else here is bookkeeping.
//!
//! # Why nothing is guessed
//!
//! A configuration this build cannot run is still listed, with the reason
//! ([`lazydap_core::NotRunnable`]). A `${...}` variable nothing can expand is
//! left in the string verbatim and recorded, rather than being replaced with
//! an empty string — `${workspaceFolder}/x` silently becoming `/x` is how you
//! debug the wrong binary.

use lazydap_core::{AdapterKind, LaunchConfig, LaunchConfigSource, LaunchKind};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where VS Code keeps it, relative to the project root.
pub const LAUNCH_JSON_PATH: &str = ".vscode/launch.json";

#[derive(Debug, thiserror::Error)]
pub enum LaunchJsonError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not valid launch.json: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    /// Refused before `serde_json` ever saw it — an unterminated comment or
    /// string, which VS Code refuses too.
    #[error("{path} is not valid launch.json: {problem}")]
    Malformed { path: PathBuf, problem: String },
}

pub type Result<T> = std::result::Result<T, LaunchJsonError>;

/// What one `launch.json` yielded.
///
/// Warnings are separate from the configurations rather than fatal: a file
/// with one unreadable configuration and three good ones should give you the
/// three, and say what happened to the fourth.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Imported {
    pub configs: Vec<LaunchConfig>,
    pub warnings: Vec<String>,
}

/// Import `<root>/.vscode/launch.json`, if there is one.
///
/// A project without the file is the normal case, not an error.
pub fn import(root: &Path) -> Result<Imported> {
    let path = root.join(LAUNCH_JSON_PATH);
    let body = match std::fs::read_to_string(&path) {
        Ok(body) => body,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Imported::default());
        }
        Err(source) => return Err(LaunchJsonError::Read { path, source }),
    };
    parse(&path, &body, root)
}

/// Parse the contents of a `launch.json`, with `root` as `${workspaceFolder}`.
pub fn parse(path: &Path, body: &str, root: &Path) -> Result<Imported> {
    let stripped = strip_jsonc(body).map_err(|problem| LaunchJsonError::Malformed {
        path: path.to_path_buf(),
        problem,
    })?;
    let file: LaunchJsonFile =
        serde_json::from_str(&stripped).map_err(|source| LaunchJsonError::Parse {
            path: path.to_path_buf(),
            source,
        })?;

    let mut imported = Imported::default();
    for (index, raw) in file.configurations.into_iter().enumerate() {
        // Per configuration, so one unreadable entry costs only itself. The
        // whole file is one `serde_json` call otherwise, and a single bad
        // `args` would take the other configurations down with it.
        match serde_json::from_value::<VsCodeConfig>(raw) {
            Ok(config) => match map(config, root, &mut imported.warnings) {
                Some(config) => imported.configs.push(config),
                None => continue,
            },
            Err(error) => imported
                .warnings
                .push(format!("configuration {index} could not be read: {error}")),
        }
    }
    Ok(imported)
}

/// The subset of VS Code's file lazydap reads.
///
/// Unknown keys are ignored rather than rejected — `compounds`,
/// `preLaunchTask`, `MIMode`, `sourceFileMap` and the rest are VS Code's
/// business, and a file that works there must not fail here.
#[derive(Debug, Deserialize)]
struct LaunchJsonFile {
    #[serde(default)]
    configurations: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VsCodeConfig {
    name: String,
    #[serde(rename = "type")]
    adapter_type: String,
    request: String,
    program: Option<String>,
    #[serde(default)]
    args: Arguments,
    cwd: Option<String>,
    /// codelldb's spelling: a map.
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// cppdbg's spelling for the same thing: an array of `{name, value}`.
    /// A configuration that sets `LD_LIBRARY_PATH` here and is launched
    /// without it is a program in the wrong state, with nothing said about it.
    #[serde(default)]
    environment: Vec<EnvPair>,
    #[serde(default)]
    stop_on_entry: bool,
    /// cppdbg's spelling for `stopOnEntry`.
    #[serde(default)]
    stop_at_entry: bool,
    /// delve's launch mode: `debug` (compile source or a package), `exec` (run
    /// a prebuilt binary), `test` (compile and run tests), `auto`. Meaningless
    /// to the other adapters, read only for `type: go`.
    mode: Option<String>,
    /// debugpy's interpreter pin. A string, or a list whose head is the
    /// interpreter and whose tail is arguments for it.
    python: Option<Interpreter>,
    /// The older spelling of the same thing, still in plenty of committed
    /// files. `python` wins when a configuration carries both.
    python_path: Option<String>,
}

/// `python` as debugpy's schema allows it to be written.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Interpreter {
    Path(String),
    /// `["python3", "-X", "faulthandler"]` — the interpreter, then flags for
    /// it. lazydap takes the head and drops the tail: it runs the interpreter
    /// as `<python> -m debugpy.adapter`, and there is nowhere to put extra
    /// interpreter flags without changing what that command means.
    Argv(Vec<String>),
}

impl Interpreter {
    fn path(self) -> Option<String> {
        match self {
            Self::Path(path) => Some(path),
            Self::Argv(argv) => argv.into_iter().next(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct EnvPair {
    name: String,
    value: String,
}

/// `args` as either debugger spells it.
///
/// codelldb documents a list and also accepts one shell-style string;
/// real files in the wild carry both. Refusing the string form would drop a
/// configuration that works in the editor it came from.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Arguments {
    List(Vec<String>),
    Line(String),
}

impl Default for Arguments {
    fn default() -> Self {
        Self::List(Vec::new())
    }
}

/// One VS Code configuration as a lazydap one, or `None` if it is not a debug
/// configuration at all.
fn map(config: VsCodeConfig, root: &Path, warnings: &mut Vec<String>) -> Option<LaunchConfig> {
    let name = config.name;

    let kind = match config.request.as_str() {
        "launch" => LaunchKind::Launch,
        "attach" => LaunchKind::Attach,
        other => {
            warnings.push(format!(
                "`{name}` asks for request `{other}`, which is neither launch nor attach; skipped",
            ));
            return None;
        }
    };

    let adapter = match config.adapter_type.as_str() {
        "lldb" => Some(AdapterKind::Codelldb),
        // `python` is the older VS Code Python extension's spelling and
        // `debugpy` the current one. Files in the wild carry both, and they
        // name the same adapter (M18).
        "debugpy" | "python" => Some(AdapterKind::Debugpy),
        // What the VS Code Go extension writes. It has one `type` for every
        // mode — `debug`, `exec`, `test` — and says which in a `mode` field
        // (M22). `debug`/`exec`/`auto` run under delve; `test` is deferred and
        // an `exec` naming a `.go` source is a contradiction — both are turned
        // into a `blocked` reason below rather than run into an adapter error.
        "go" => Some(AdapterKind::Delve),
        // Microsoft's C/C++ extension describes the same thing — a native
        // program, its arguments, a working directory — so codelldb can run
        // it. Its `MIMode`, `miDebuggerPath` and `setupCommands` are not read,
        // which is worth saying out loud rather than discovering at a
        // breakpoint that never binds.
        "cppdbg" => {
            warnings.push(format!(
                "`{name}` is a cppdbg configuration; lazydap runs it under codelldb, which reads \
                 its program, arguments, working directory, environment and stopAtEntry, but not \
                 its MIMode, miDebuggerPath or setupCommands",
            ));
            Some(AdapterKind::Codelldb)
        }
        _ => None,
    };

    // Read before `config.program` and `config.mode` are consumed below. Only
    // meaningful for delve; `None` for every other adapter.
    let mode_block = match adapter {
        Some(AdapterKind::Delve) => {
            delve_mode_block(config.mode.as_deref(), config.program.as_deref())
        }
        _ => None,
    };

    let mut unresolved = Vec::new();

    // The interpreter the configuration insists on, if it names one. This is
    // how a virtualenv is spelled — `"python": "${workspaceFolder}/.venv/bin/python"`
    // — so it is expanded and made absolute like any other path here. Dropping
    // it would run the program under whichever interpreter `PATH` offers,
    // which is exactly the one without the project's dependencies.
    let adapter_command = config
        .python
        .and_then(Interpreter::path)
        .or(config.python_path)
        .map(|python| absolute(&expand(&python, root, &mut unresolved), root));

    let program = config
        .program
        .map(|program| absolute(&expand(&program, root, &mut unresolved), root));
    let cwd = config
        .cwd
        .map(|cwd| absolute(&expand(&cwd, root, &mut unresolved), root));

    // cppdbg writes `environment: [{name, value}]` where codelldb writes
    // `env: {}`. Both are read, and `env` wins a collision only because
    // something has to: a file carrying both spellings of one variable has
    // already contradicted itself.
    let mut env: BTreeMap<String, String> = config
        .environment
        .into_iter()
        .map(|pair| (pair.name, expand(&pair.value, root, &mut unresolved)))
        .collect();
    env.extend(
        config
            .env
            .into_iter()
            .map(|(key, value)| (key, expand(&value, root, &mut unresolved))),
    );

    let (args, blocked) = match config.args {
        Arguments::List(args) => (
            args.iter()
                .map(|arg| expand(arg, root, &mut unresolved))
                .collect(),
            None,
        ),
        Arguments::Line(line) => match split_arguments(&expand(&line, root, &mut unresolved)) {
            Ok(args) => (args, None),
            Err(problem) => {
                warnings.push(format!(
                    "`{name}` has an argument string that cannot be split: {problem}"
                ));
                (
                    Vec::new(),
                    Some(lazydap_core::NotRunnable::BadArguments { problem }),
                )
            }
        },
    };

    if !unresolved.is_empty() {
        warnings.push(format!(
            "`{name}` uses {}, which nothing here can expand; it is left as written",
            unresolved.join(", "),
        ));
    }

    Some(LaunchConfig {
        name,
        adapter,
        adapter_type: config.adapter_type,
        kind,
        program,
        args,
        cwd,
        env,
        // Either spelling means the same thing, and a configuration that says
        // both means it once.
        stop_on_entry: config.stop_on_entry || config.stop_at_entry,
        adapter_command,
        source: LaunchConfigSource::VsCodeLaunchJson,
        unresolved,
        // A bad `args` string and a bad delve mode are both reasons not to run;
        // either one is enough, so the first found is reported.
        blocked: blocked.or(mode_block),
    })
}

/// Whether a `type: go` configuration names a delve mode lazydap will not run.
///
/// `test` is deferred (M22): delve can compile-and-test, but lazydap has no way
/// to tell it "this is a test" — a `.go` file is an ordinary program to
/// `debug` — so a `test` config is listed and refused rather than silently run
/// as the wrong thing. `exec` names a prebuilt binary, so an `exec` pointing at
/// a `.go` *source* is a contradiction delve would reject; caught here so the
/// reason names the file rather than surfacing as an adapter crash. `debug`,
/// `auto`, and an absent mode are all runnable, and the adapter infers `debug`
/// vs `exec` from the program's shape (`delve.rs`).
fn delve_mode_block(
    mode: Option<&str>,
    program: Option<&str>,
) -> Option<lazydap_core::NotRunnable> {
    use lazydap_core::NotRunnable::DelveMode;
    match mode {
        Some("test") => Some(DelveMode {
            problem: "it uses delve's `test` mode, which lazydap does not run yet".to_string(),
        }),
        Some("exec") if program.is_some_and(is_go_source) => Some(DelveMode {
            problem: "it uses delve's `exec` mode, which runs a prebuilt binary, but names a \
                      `.go` source"
                .to_string(),
        }),
        _ => None,
    }
}

/// Whether a program path is a `.go` source file rather than a built binary.
fn is_go_source(program: &str) -> bool {
    Path::new(program)
        .extension()
        .and_then(|extension| extension.to_str())
        == Some("go")
}

/// Split one shell-style argument string into arguments.
///
/// Deliberately small: whitespace separates, double and single quotes group,
/// and a backslash escapes the next character outside single quotes. That is
/// the part of shell word-splitting that appears in a `launch.json`; variable
/// expansion, globbing, `$(...)` and the rest are the shell's job and are not
/// happening here — a debugger silently running a subshell out of a config
/// file would be a far worse surprise than an argument that keeps its `$`.
///
/// An unterminated quote is an error rather than a guess. Closing it for the
/// author would hand the program an argument list they never wrote.
fn split_arguments(line: &str) -> std::result::Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();

    while let Some(c) = chars.next() {
        match c {
            '\\' if quote != Some('\'') => match chars.next() {
                Some(escaped) => {
                    current.push(escaped);
                    started = true;
                }
                None => return Err("it ends in a trailing backslash".to_string()),
            },
            '"' | '\'' if quote.is_none() => {
                quote = Some(c);
                // An empty quoted string is an argument: `--name ""` passes
                // one, and dropping it shifts every argument after it.
                started = true;
            }
            c if Some(c) == quote => quote = None,
            c if c.is_whitespace() && quote.is_none() => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            c => {
                current.push(c);
                started = true;
            }
        }
    }

    if let Some(quote) = quote {
        return Err(format!("it has an unterminated {quote} quote"));
    }
    if started {
        args.push(current);
    }
    Ok(args)
}

/// A path from the file as an absolute one.
///
/// VS Code resolves a relative path against the workspace folder, so lazydap
/// does too. Not canonicalised: the program may not be built yet, and
/// `launches list` must still show what it would run.
fn absolute(path: &str, root: &Path) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

/// Expand the `${...}` variables lazydap knows, and record the ones it does
/// not.
///
/// An unknown variable is left in the string exactly as written. That is the
/// conservative half of the bargain: the caller can then refuse to launch
/// (see [`LaunchConfig::not_runnable`]) instead of running whatever path is
/// left after deleting the parts nobody understood.
fn expand(raw: &str, root: &Path, unresolved: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;

    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        // A `${` with no `}` after it is either a variable somebody mistyped
        // — `${workspaceFolder/app` — or a string that happens to contain a
        // brace, like `cost ${100`. Every variable VS Code defines starts with
        // a letter, so that is where the line goes: recorded as unresolved
        // when it reads as a name, left alone when it does not. Either way the
        // text passes through exactly as written, because deleting the part
        // nobody understood is how you debug the wrong binary. Before this it
        // was always the second reading, and `launches list` called
        // `${workspaceFolder/app` runnable right up until the launch failed.
        let Some(end) = after.find('}') else {
            let tail = rest[start..].to_string();
            if after.starts_with(|c: char| c.is_ascii_alphabetic()) && !unresolved.contains(&tail) {
                unresolved.push(tail.clone());
            }
            out.push_str(&tail);
            return out;
        };

        let name = &after[..end];
        match resolve(name, root) {
            Some(value) => out.push_str(&value),
            None => {
                let token = format!("${{{name}}}");
                if !unresolved.contains(&token) {
                    unresolved.push(token.clone());
                }
                out.push_str(&token);
            }
        }
        rest = &after[end + 1..];
    }

    out.push_str(rest);
    out
}

/// The variables lazydap can answer for. Everything else is somebody else's:
/// `${file}` and `${selectedText}` need an editor, `${command:...}` needs VS
/// Code to run an extension command, and `${input:...}` needs a prompt.
fn resolve(name: &str, root: &Path) -> Option<String> {
    match name {
        // `workspaceRoot` is the pre-2018 spelling. Still all over real files.
        "workspaceFolder" | "workspaceRoot" => Some(root.to_string_lossy().into_owned()),
        "workspaceFolderBasename" => root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        "userHome" => dirs::home_dir().map(|home| home.to_string_lossy().into_owned()),
        "pathSeparator" | "/" => Some(std::path::MAIN_SEPARATOR.to_string()),
        _ => name
            .strip_prefix("env:")
            // A variable that is not set is *unresolved*, not empty. VS Code
            // substitutes the empty string; doing that to
            // `${env:BUILD_DIR}/app` gives `/app`, which is a real path on
            // every Unix machine and the wrong one on all of them.
            .and_then(|variable| std::env::var(variable).ok()),
    }
}

/// JSON with the comments and trailing commas taken out.
///
/// Comments become **the whitespace they occupied**, character for character,
/// newlines included. Two reasons, and the second is the one that bites:
///
/// 1. A `serde_json` error then points at the line *and column* the reader is
///    looking at in their editor.
/// 2. Deleting a comment joins what was on either side of it. `tr/*x*/ue`
///    would become `true` — a document VS Code rejects, quietly accepted here
///    as something the author never wrote. A space keeps it two tokens, and
///    `serde_json` refuses it exactly as VS Code does.
///
/// Returns the offending construct rather than a string when the input runs
/// out mid-comment or mid-string. Both are malformed in VS Code, and a parser
/// that accepts what its own format's editor rejects is a parser that
/// disagrees with the file on screen.
fn strip_jsonc(source: &str) -> std::result::Result<String, String> {
    let mut out = String::with_capacity(source.len());
    // A comma that has been read but not written yet, plus whatever
    // whitespace followed it. Held back because whether it is legal depends
    // on what comes next: `[1, 2]` keeps it, `[1, ]` does not.
    let mut pending_comma = false;
    let mut gap = String::new();
    let mut chars = source.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            // A line comment ends at the newline, which survives it. Running
            // off the end of the file is fine here — a file may end in one.
            '/' if chars.peek() == Some(&'/') => {
                gap.push_str("  ");
                chars.next();
                for c in chars.by_ref() {
                    gap.push(blank(c));
                    if c == '\n' {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                gap.push_str("  ");
                chars.next();
                let mut previous = '\0';
                let mut closed = false;
                for c in chars.by_ref() {
                    gap.push(blank(c));
                    if previous == '*' && c == '/' {
                        closed = true;
                        break;
                    }
                    previous = c;
                }
                if !closed {
                    return Err("a /* comment is never closed".to_string());
                }
            }
            c if c.is_whitespace() => gap.push(c),
            ',' => {
                // Flush first: two commas in a row is malformed JSON, and
                // swallowing one here would hide it from serde_json, which is
                // the thing that gives a good error about it.
                flush(&mut out, &mut pending_comma, &mut gap);
                pending_comma = true;
            }
            '}' | ']' => {
                // The trailing comma VS Code tolerates and JSON does not.
                pending_comma = false;
                out.push_str(&gap);
                gap.clear();
                out.push(c);
            }
            '"' => {
                flush(&mut out, &mut pending_comma, &mut gap);
                out.push('"');
                // Copied verbatim to the closing quote. A `//` in a URL and a
                // `,` in a separator are data, and this is the whole reason
                // the scanner exists.
                let mut escaped = false;
                let mut closed = false;
                for c in chars.by_ref() {
                    out.push(c);
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == '"' {
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return Err("a \" string is never closed".to_string());
                }
            }
            other => {
                flush(&mut out, &mut pending_comma, &mut gap);
                out.push(other);
            }
        }
    }

    // Whatever is left is malformed input; hand it over as it was and let
    // serde_json describe it.
    flush(&mut out, &mut pending_comma, &mut gap);
    Ok(out)
}

/// What a character becomes when the comment around it is taken out.
///
/// Line breaks stay, so line numbers survive; everything else becomes a space,
/// so column numbers do too — and so that removing a comment never joins the
/// tokens on either side of it.
fn blank(c: char) -> char {
    if c == '\n' || c == '\r' { c } else { ' ' }
}

fn flush(out: &mut String, pending_comma: &mut bool, gap: &mut String) {
    if std::mem::take(pending_comma) {
        out.push(',');
    }
    out.push_str(gap);
    gap.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/p";

    fn import_str(body: &str) -> Imported {
        parse(Path::new("/p/.vscode/launch.json"), body, Path::new(ROOT)).expect("parse")
    }

    fn strip_ok(source: &str) -> String {
        strip_jsonc(source).expect("valid JSONC")
    }

    // --- The stripper ---

    #[test]
    fn line_and_block_comments_are_removed() {
        let stripped = strip_ok(
            r#"{
                // the version VS Code writes
                "version": "0.2.0", /* and a block one */
                "configurations": []
            }"#,
        );
        let value: serde_json::Value = serde_json::from_str(&stripped).expect("valid JSON");
        assert_eq!(value["version"], "0.2.0");
    }

    #[test]
    fn a_comment_marker_inside_a_string_is_data() {
        // The bug every naive stripper has: a URL is not a comment.
        let stripped = strip_ok(r#"{"url": "https://example.com/x", "n": 1}"#);
        let value: serde_json::Value = serde_json::from_str(&stripped).expect("valid JSON");
        assert_eq!(value["url"], "https://example.com/x");
        assert_eq!(value["n"], 1);
    }

    #[test]
    fn a_block_comment_marker_inside_a_string_is_data_too() {
        let stripped = strip_ok(r#"{"glob": "src/*/ *.c", "n": 1}"#);
        let value: serde_json::Value = serde_json::from_str(&stripped).expect("valid JSON");
        assert_eq!(value["glob"], "src/*/ *.c");
    }

    #[test]
    fn an_escaped_quote_does_not_end_the_string() {
        let stripped = strip_ok(r#"{"say": "she said \"// hi\"", "n": 1}"#);
        let value: serde_json::Value = serde_json::from_str(&stripped).expect("valid JSON");
        assert_eq!(value["say"], r#"she said "// hi""#);
    }

    #[test]
    fn a_string_ending_in_an_escaped_backslash_ends_where_it_looks_like_it_does() {
        let stripped = strip_ok(r#"{"path": "C:\\", "n": 1}"#);
        let value: serde_json::Value = serde_json::from_str(&stripped).expect("valid JSON");
        assert_eq!(value["path"], r"C:\");
        assert_eq!(value["n"], 1);
    }

    #[test]
    fn trailing_commas_are_dropped_because_vs_code_accepts_them() {
        let stripped = strip_ok(r#"{"args": ["a", "b",], "n": 1,}"#);
        let value: serde_json::Value = serde_json::from_str(&stripped).expect("valid JSON");
        assert_eq!(value["args"][1], "b");
        assert_eq!(value["n"], 1);
    }

    #[test]
    fn a_trailing_comma_followed_by_a_comment_is_still_trailing() {
        let stripped = strip_ok(
            r#"{"args": [
                "a", // the only one
            ]}"#,
        );
        let value: serde_json::Value = serde_json::from_str(&stripped).expect("valid JSON");
        assert_eq!(value["args"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn a_comma_inside_a_string_is_not_a_separator() {
        let stripped = strip_ok(r#"{"sep": ",", "list": [1]}"#);
        let value: serde_json::Value = serde_json::from_str(&stripped).expect("valid JSON");
        assert_eq!(value["sep"], ",");
    }

    #[test]
    fn stripping_keeps_the_line_numbers_a_reader_would_count() {
        // A parse error has to point at the line the person is looking at.
        let source = "{\n// one\n/* two\nthree */\n\"a\": 1\n}";
        assert_eq!(
            strip_ok(source).matches('\n').count(),
            source.matches('\n').count(),
            "comments must leave their newlines behind",
        );
    }

    #[test]
    fn a_comment_becomes_the_space_it_occupied_rather_than_disappearing() {
        // Deleting it would join what was either side: `tr/*x*/ue` reads as
        // `true`, which VS Code rejects and we would have silently accepted.
        let stripped = strip_ok(r#"{"a": tr/*x*/ue}"#);
        assert!(
            serde_json::from_str::<serde_json::Value>(&stripped).is_err(),
            "got: {stripped}",
        );
        assert!(
            !stripped.contains("true"),
            "the comment must not weld the two halves together: {stripped}",
        );
    }

    #[test]
    fn stripping_keeps_the_columns_a_parse_error_would_point_at() {
        let source = r#"{"a": /* two */ 1}"#;
        let stripped = strip_ok(source);
        assert_eq!(stripped.len(), source.len(), "got: {stripped}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stripped).expect("valid JSON")["a"],
            1,
        );
    }

    #[test]
    fn an_unclosed_block_comment_is_refused_rather_than_swallowing_the_file() {
        let problem = strip_jsonc(r#"{"a": 1} /* and then nothing"#)
            .expect_err("VS Code will not read this either");
        assert!(problem.contains("never closed"), "got: {problem}");
    }

    #[test]
    fn a_comment_that_only_looks_closed_is_still_unclosed() {
        // `/*/` — the slash that opened it cannot also close it.
        let problem = strip_jsonc(r#"{"a": 1} /*/"#).expect_err("that is not a closed comment");
        assert!(problem.contains("never closed"), "got: {problem}");
    }

    #[test]
    fn an_unclosed_string_is_refused() {
        let problem = strip_jsonc(r#"{"a": "unterminated}"#).expect_err("that string never ends");
        assert!(problem.contains("never closed"), "got: {problem}");
    }

    #[test]
    fn a_file_ending_in_a_line_comment_is_fine() {
        // Unlike the two above: a line comment is closed by the end of the
        // file just as it is by a newline.
        let stripped = strip_ok("{\"a\": 1} // trailing thought");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stripped).expect("valid JSON")["a"],
            1,
        );
    }

    // --- Shell-style argument strings ---

    #[test]
    fn an_argument_string_splits_on_whitespace() {
        assert_eq!(
            split_arguments("--verbose --level 3").expect("split"),
            vec!["--verbose", "--level", "3"],
        );
    }

    #[test]
    fn quotes_group_words_that_contain_spaces() {
        assert_eq!(
            split_arguments(r#"--path "/tmp/two words" --name 'single quoted'"#).expect("split"),
            vec!["--path", "/tmp/two words", "--name", "single quoted"],
        );
    }

    #[test]
    fn an_empty_quoted_argument_is_still_an_argument() {
        // Dropping it would shift every argument after it by one.
        assert_eq!(
            split_arguments(r#"--name "" --last"#).expect("split"),
            vec!["--name", "", "--last"],
        );
    }

    #[test]
    fn a_backslash_escapes_the_next_character_outside_single_quotes() {
        assert_eq!(
            split_arguments(r"a\ b 'c\ d'").expect("split"),
            vec!["a b", r"c\ d"],
        );
    }

    #[test]
    fn nothing_in_an_argument_string_is_expanded_by_a_shell() {
        // A debugger quietly running a subshell out of a config file would be
        // a much worse surprise than an argument that keeps its `$`.
        assert_eq!(
            split_arguments("--home $HOME --sub $(whoami)").expect("split"),
            vec!["--home", "$HOME", "--sub", "$(whoami)"],
        );
    }

    #[test]
    fn an_unterminated_quote_is_an_error_rather_than_a_guess() {
        let problem = split_arguments(r#"--name "never closed"#).expect_err("unterminated");
        assert!(problem.contains("unterminated"), "got: {problem}");
    }

    #[test]
    fn a_configuration_whose_arguments_cannot_be_split_is_listed_and_refused() {
        let imported = import_str(
            r#"{"configurations": [{
                "type": "lldb", "request": "launch", "name": "Quoted",
                "program": "app", "args": "--name \"never closed"
            }]}"#,
        );

        let config = &imported.configs[0];
        assert!(config.args.is_empty());
        let reason = config.not_runnable().expect("it cannot run");
        assert!(reason.to_string().contains("unterminated"), "got: {reason}");
        assert!(
            imported.warnings.iter().any(|w| w.contains("Quoted")),
            "got: {:?}",
            imported.warnings,
        );
    }

    #[test]
    fn an_argument_string_is_accepted_the_way_codelldb_accepts_it() {
        let imported = import_str(
            r#"{"configurations": [{
                "type": "lldb", "request": "launch", "name": "Line",
                "program": "app", "args": "--in ${workspaceFolder}/data --verbose"
            }]}"#,
        );

        assert_eq!(
            imported.configs[0].args,
            vec!["--in", "/p/data", "--verbose"],
            "variables expand before splitting, so a path with a space still splits right",
        );
        assert_eq!(imported.configs[0].not_runnable(), None);
    }

    // --- Mapping ---

    #[test]
    fn a_real_vs_code_lldb_configuration_becomes_a_launch_config() {
        let imported = import_str(
            r#"{
                "version": "0.2.0",
                "configurations": [
                    {
                        "type": "lldb",
                        "request": "launch",
                        "name": "Debug binary",
                        "program": "${workspaceFolder}/build/hello",
                        "args": ["--verbose"],
                        "cwd": "${workspaceFolder}",
                        "stopOnEntry": true,
                        "env": { "RUST_LOG": "debug" }
                    }
                ]
            }"#,
        );

        assert_eq!(imported.warnings, Vec::<String>::new());
        let config = &imported.configs[0];
        assert_eq!(config.name, "Debug binary");
        assert_eq!(config.adapter, Some(AdapterKind::Codelldb));
        assert_eq!(config.kind, LaunchKind::Launch);
        assert_eq!(config.program, Some(PathBuf::from("/p/build/hello")));
        assert_eq!(config.cwd, Some(PathBuf::from("/p")));
        assert_eq!(config.args, vec!["--verbose"]);
        assert_eq!(config.env["RUST_LOG"], "debug");
        assert!(config.stop_on_entry);
        assert_eq!(config.source, LaunchConfigSource::VsCodeLaunchJson);
        assert_eq!(config.not_runnable(), None);
    }

    #[test]
    fn keys_lazydap_does_not_model_are_ignored_rather_than_refused() {
        // Straight out of a real C++ project: none of this is lazydap's, and
        // all of it has to be survivable.
        let imported = import_str(
            r#"{
                "configurations": [{
                    "type": "lldb",
                    "request": "launch",
                    "name": "app",
                    "program": "${workspaceFolder}/app",
                    "preLaunchTask": "build",
                    "sourceFileMap": { "/build": "${workspaceFolder}" },
                    "initCommands": ["settings set target.language c++"],
                    "terminal": "integrated",
                    "presentation": { "hidden": false, "order": 1 }
                }],
                "compounds": [{ "name": "all", "configurations": ["app"] }]
            }"#,
        );

        assert_eq!(imported.configs.len(), 1);
        assert_eq!(imported.configs[0].name, "app");
        assert_eq!(imported.warnings, Vec::<String>::new());
    }

    #[test]
    fn a_cppdbg_configuration_keeps_its_environment_and_its_entry_stop() {
        // cppdbg spells both differently from codelldb. A program that needs
        // LD_LIBRARY_PATH and is launched without it is in the wrong state,
        // and nothing would have said so.
        let imported = import_str(
            r#"{"configurations": [{
                "type": "cppdbg",
                "request": "launch",
                "name": "C++ app",
                "program": "${workspaceFolder}/build/app",
                "cwd": "${workspaceFolder}",
                "stopAtEntry": true,
                "MIMode": "lldb",
                "environment": [
                    {"name": "LD_LIBRARY_PATH", "value": "${workspaceFolder}/lib"},
                    {"name": "LOG", "value": "debug"}
                ]
            }]}"#,
        );

        let config = &imported.configs[0];
        assert_eq!(config.adapter, Some(AdapterKind::Codelldb));
        assert_eq!(
            config.env["LD_LIBRARY_PATH"], "/p/lib",
            "and it expands too"
        );
        assert_eq!(config.env["LOG"], "debug");
        assert!(config.stop_on_entry, "stopAtEntry means stopOnEntry");
        assert_eq!(config.not_runnable(), None);
        assert!(
            imported.warnings.iter().any(|w| w.contains("MIMode")),
            "the fields that are still ignored are named: {:?}",
            imported.warnings,
        );
    }

    #[test]
    fn both_environment_spellings_in_one_configuration_are_merged() {
        let imported = import_str(
            r#"{"configurations": [{
                "type": "cppdbg", "request": "launch", "name": "Both", "program": "app",
                "environment": [{"name": "FROM_ARRAY", "value": "1"}],
                "env": {"FROM_MAP": "2"}
            }]}"#,
        );

        let env = &imported.configs[0].env;
        assert_eq!(env["FROM_ARRAY"], "1");
        assert_eq!(env["FROM_MAP"], "2");
    }

    #[test]
    fn a_configuration_for_another_debugger_is_listed_with_its_own_type() {
        // `coreclr`, because this test needs a debugger lazydap does not ship
        // and `go` stopped being one at M22.
        let imported = import_str(
            r#"{"configurations": [
                {"type": "coreclr", "request": "launch", "name": "API", "program": "App.dll"}
            ]}"#,
        );

        let config = &imported.configs[0];
        assert_eq!(config.adapter, None);
        assert_eq!(config.adapter_type, "coreclr");
        assert!(
            config.not_runnable().is_some(),
            "listing it is useful; pretending lazydap can run it is not",
        );
    }

    #[test]
    fn a_go_configuration_runs_under_delve() {
        // What the VS Code Go extension writes. A `debug` config naming a
        // source file is runnable; the adapter infers compile-vs-run from the
        // program's shape (`delve.rs`).
        let imported = import_str(
            r#"{"configurations": [
                {"type": "go", "request": "launch", "name": "API", "mode": "debug", "program": "main.go"}
            ]}"#,
        );

        let config = &imported.configs[0];
        assert_eq!(config.adapter, Some(AdapterKind::Delve));
        assert_eq!(config.not_runnable(), None);
    }

    #[test]
    fn a_go_debug_config_naming_a_package_directory_is_runnable() {
        // The standard shape: `debug` mode pointed at a package, not a single
        // file. It must list as runnable — the fix to the adapter's mode
        // inference is what makes the launch itself work.
        let imported = import_str(
            r#"{"configurations": [
                {"type": "go", "request": "launch", "name": "server", "mode": "debug", "program": "${workspaceFolder}/cmd/server"}
            ]}"#,
        );
        assert_eq!(imported.configs[0].not_runnable(), None);
    }

    #[test]
    fn a_go_exec_config_naming_a_built_binary_is_runnable() {
        let imported = import_str(
            r#"{"configurations": [
                {"type": "go", "request": "launch", "name": "run", "mode": "exec", "program": "${workspaceFolder}/bin/server"}
            ]}"#,
        );
        assert_eq!(imported.configs[0].adapter, Some(AdapterKind::Delve));
        assert_eq!(imported.configs[0].not_runnable(), None);
    }

    #[test]
    fn a_go_test_config_is_listed_but_not_runnable() {
        // `test` mode is deferred (M22): delve can build-and-test, but lazydap
        // has no way to say "this is a test". Falsely marking it runnable and
        // then running it as an ordinary program is the bug this prevents.
        let imported = import_str(
            r#"{"configurations": [
                {"type": "go", "request": "launch", "name": "unit", "mode": "test", "program": "${workspaceFolder}/pkg"}
            ]}"#,
        );

        let config = &imported.configs[0];
        // Still listed, and still names delve — a person wants to see it.
        assert_eq!(config.adapter, Some(AdapterKind::Delve));
        assert_eq!(config.adapter_type, "go");
        let reason = config.not_runnable().expect("test mode is not runnable");
        assert!(
            reason.to_string().contains("test"),
            "the reason must name the mode: {reason}",
        );
    }

    #[test]
    fn a_go_exec_config_pointing_at_a_go_source_is_not_runnable() {
        // `exec` runs a prebuilt binary; a `.go` source is not one. Caught at
        // import so the reason names the file rather than surfacing as an
        // adapter crash at launch.
        let imported = import_str(
            r#"{"configurations": [
                {"type": "go", "request": "launch", "name": "oops", "mode": "exec", "program": "main.go"}
            ]}"#,
        );

        let reason = imported.configs[0]
            .not_runnable()
            .expect("exec on a .go source is a contradiction");
        assert!(
            reason.to_string().contains("exec"),
            "the reason must name the mode: {reason}",
        );
    }

    /// A virtualenv pin, which is the normal way a Python project says which
    /// interpreter it means. Discarding it runs the program under whichever
    /// interpreter `PATH` offers — the one without the project's dependencies
    /// — and reports the import error that follows as the program's own.
    #[test]
    fn a_configuration_that_pins_its_interpreter_keeps_it() {
        let imported = import_str(
            r#"{"configurations": [{
                "type": "debugpy", "request": "launch", "name": "API",
                "program": "${workspaceFolder}/app.py",
                "python": "${workspaceFolder}/.venv/bin/python"
            }]}"#,
        );

        assert_eq!(
            imported.configs[0].adapter_command,
            Some(PathBuf::from("/p/.venv/bin/python")),
            "the pin is expanded and made absolute like any other path here",
        );
    }

    #[test]
    fn the_older_python_path_spelling_is_read_too() {
        // Deprecated by the extension, still in plenty of committed files.
        let imported = import_str(
            r#"{"configurations": [{
                "type": "python", "request": "launch", "name": "API",
                "program": "app.py", "pythonPath": "/usr/local/bin/python3.12"
            }]}"#,
        );

        assert_eq!(
            imported.configs[0].adapter_command,
            Some(PathBuf::from("/usr/local/bin/python3.12")),
        );
    }

    #[test]
    fn an_interpreter_written_as_a_list_gives_up_its_head() {
        // debugpy's schema allows `["python3", "-X", "faulthandler"]`. lazydap
        // runs the interpreter as `<python> -m debugpy.adapter`, so there is
        // nowhere to put the flags without changing what that command means.
        let imported = import_str(
            r#"{"configurations": [{
                "type": "debugpy", "request": "launch", "name": "API",
                "program": "app.py", "python": ["/usr/bin/python3", "-X", "faulthandler"]
            }]}"#,
        );

        assert_eq!(
            imported.configs[0].adapter_command,
            Some(PathBuf::from("/usr/bin/python3")),
        );
    }

    #[test]
    fn a_configuration_naming_no_interpreter_leaves_it_to_discovery() {
        let imported = import_str(
            r#"{"configurations": [{
                "type": "debugpy", "request": "launch", "name": "API", "program": "app.py"
            }]}"#,
        );

        assert_eq!(imported.configs[0].adapter_command, None);
    }

    /// Both spellings the Python extension has used, and both runnable since
    /// M18. Until then these were imported, listed, and refused.
    #[test]
    fn a_python_configuration_is_runnable_under_debugpy() {
        for adapter_type in ["python", "debugpy"] {
            let imported = import_str(&format!(
                r#"{{"configurations": [
                    {{"type": "{adapter_type}", "request": "launch",
                     "name": "API", "program": "${{workspaceFolder}}/app.py"}}
                ]}}"#,
            ));

            let config = &imported.configs[0];
            assert_eq!(config.adapter, Some(AdapterKind::Debugpy));
            assert_eq!(config.adapter_type, adapter_type);
            assert_eq!(
                config.not_runnable(),
                None,
                "a `{adapter_type}` configuration is runnable now",
            );
        }
    }

    #[test]
    fn an_attach_configuration_survives_the_import() {
        let imported = import_str(
            r#"{"configurations": [
                {"type": "lldb", "request": "attach", "name": "PID", "pid": 42}
            ]}"#,
        );
        assert_eq!(imported.configs[0].kind, LaunchKind::Attach);
    }

    #[test]
    fn a_configuration_that_is_neither_launch_nor_attach_is_skipped_with_a_warning() {
        let imported = import_str(
            r#"{"configurations": [
                {"type": "lldb", "request": "custom", "name": "Odd"}
            ]}"#,
        );
        assert!(imported.configs.is_empty());
        assert!(
            imported.warnings[0].contains("Odd"),
            "got: {:?}",
            imported.warnings
        );
    }

    #[test]
    fn one_unreadable_configuration_does_not_cost_the_others() {
        let imported = import_str(
            r#"{"configurations": [
                {"type": "lldb", "request": "launch"},
                {"type": "lldb", "request": "launch", "name": "Good", "program": "app"}
            ]}"#,
        );

        assert_eq!(imported.configs.len(), 1, "the good one survives");
        assert_eq!(imported.configs[0].name, "Good");
        assert_eq!(imported.warnings.len(), 1, "and the bad one is reported");
    }

    #[test]
    fn a_variable_nothing_can_expand_is_left_alone_and_reported() {
        let imported = import_str(
            r#"{"configurations": [{
                "type": "lldb", "request": "launch", "name": "Pick",
                "program": "${command:pickProcess}/app"
            }]}"#,
        );

        let config = &imported.configs[0];
        assert_eq!(
            config.program,
            Some(PathBuf::from("/p/${command:pickProcess}/app")),
            "substituting nothing would leave a path that exists and is wrong",
        );
        assert_eq!(config.unresolved, vec!["${command:pickProcess}"]);
        assert!(config.not_runnable().is_some());
        assert!(
            imported.warnings[0].contains("Pick"),
            "got: {:?}",
            imported.warnings
        );
    }

    #[test]
    fn the_workspace_folder_basename_is_the_directory_name() {
        let imported = import_str(
            r#"{"configurations": [{
                "type": "lldb", "request": "launch", "name": "n",
                "program": "build/${workspaceFolderBasename}"
            }]}"#,
        );
        assert_eq!(
            imported.configs[0].program,
            Some(PathBuf::from("/p/build/p"))
        );
    }

    #[test]
    fn an_absolute_program_is_left_absolute() {
        let imported = import_str(
            r#"{"configurations": [{
                "type": "lldb", "request": "launch", "name": "n", "program": "/opt/app"
            }]}"#,
        );
        assert_eq!(imported.configs[0].program, Some(PathBuf::from("/opt/app")));
    }

    #[test]
    fn an_environment_variable_that_is_set_is_expanded() {
        // Read from the real environment rather than a mutated one: setting a
        // variable is `unsafe` in edition 2024 and this workspace forbids it.
        // PATH is there on every machine that can run this test.
        let path = std::env::var("PATH").expect("PATH");
        let mut unresolved = Vec::new();

        let expanded = expand("/opt/${env:PATH}/bin", Path::new(ROOT), &mut unresolved);

        assert_eq!(expanded, format!("/opt/{path}/bin"));
        assert!(unresolved.is_empty());
    }

    #[test]
    fn an_environment_variable_that_is_not_set_is_unresolved_rather_than_empty() {
        let mut unresolved = Vec::new();

        let expanded = expand(
            "${env:LAZYDAP_NO_SUCH_VARIABLE}/app",
            Path::new(ROOT),
            &mut unresolved,
        );

        assert_eq!(
            expanded, "${env:LAZYDAP_NO_SUCH_VARIABLE}/app",
            "VS Code substitutes the empty string here, which turns this into `/app`",
        );
        assert_eq!(unresolved, vec!["${env:LAZYDAP_NO_SUCH_VARIABLE}"]);
    }

    #[test]
    fn a_string_with_a_lone_dollar_brace_is_not_mangled() {
        let mut unresolved = Vec::new();
        assert_eq!(
            expand("cost ${100", Path::new(ROOT), &mut unresolved),
            "cost ${100",
        );
        assert!(unresolved.is_empty());
    }

    #[test]
    fn an_unterminated_variable_is_recorded_rather_than_passed_off_as_runnable() {
        // `${workspaceFolder/app` is a typo, and it used to be listed as a
        // runnable configuration whose program had a `${` in the middle of it.
        let mut unresolved = Vec::new();
        assert_eq!(
            expand("${workspaceFolder/app", Path::new(ROOT), &mut unresolved),
            "${workspaceFolder/app",
            "the text is still left exactly as written",
        );
        assert_eq!(unresolved, vec!["${workspaceFolder/app"]);
    }

    #[test]
    fn a_file_with_no_configurations_imports_as_nothing() {
        let imported = import_str(r#"{"version": "0.2.0"}"#);
        assert!(imported.configs.is_empty());
        assert!(imported.warnings.is_empty());
    }

    #[test]
    fn a_project_without_a_launch_json_is_not_an_error() {
        let imported = import(Path::new("/nowhere/at/all")).expect("a missing file is normal");
        assert_eq!(imported, Imported::default());
    }

    #[test]
    fn a_malformed_file_names_itself() {
        let error = parse(
            Path::new("/p/.vscode/launch.json"),
            "{ not json at all",
            Path::new(ROOT),
        )
        .expect_err("that is not JSON");

        assert!(error.to_string().contains("launch.json"), "got: {error}");
    }
}
