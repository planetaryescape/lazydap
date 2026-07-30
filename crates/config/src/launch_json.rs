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
    let stripped = strip_jsonc(body);
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
    args: Vec<String>,
    cwd: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    stop_on_entry: bool,
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
        // Microsoft's C/C++ extension describes the same thing — a native
        // program, its arguments, a working directory — so codelldb can run
        // it. Its `MIMode`, `miDebuggerPath` and `setupCommands` are not read,
        // which is worth saying out loud rather than discovering at a
        // breakpoint that never binds.
        "cppdbg" => {
            warnings.push(format!(
                "`{name}` is a cppdbg configuration; lazydap runs it under codelldb and ignores \
                 its MIMode and setupCommands",
            ));
            Some(AdapterKind::Codelldb)
        }
        _ => None,
    };

    let mut unresolved = Vec::new();
    let program = config
        .program
        .map(|program| absolute(&expand(&program, root, &mut unresolved), root));
    let cwd = config
        .cwd
        .map(|cwd| absolute(&expand(&cwd, root, &mut unresolved), root));
    let args = config
        .args
        .iter()
        .map(|arg| expand(arg, root, &mut unresolved))
        .collect();
    let env = config
        .env
        .into_iter()
        .map(|(key, value)| (key, expand(&value, root, &mut unresolved)))
        .collect();

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
        stop_on_entry: config.stop_on_entry,
        source: LaunchConfigSource::VsCodeLaunchJson,
        unresolved,
    })
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
        // A `${` with no `}` after it is not a variable, it is a string with
        // a brace in it. Leave the remainder alone.
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
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
/// Comments become the whitespace they occupied — newlines inside them are
/// kept — so a `serde_json` error still points at the line the reader is
/// looking at in their editor.
fn strip_jsonc(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    // A comma that has been read but not written yet, plus whatever
    // whitespace followed it. Held back because whether it is legal depends
    // on what comes next: `[1, 2]` keeps it, `[1, ]` does not.
    let mut pending_comma = false;
    let mut gap = String::new();
    let mut chars = source.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        gap.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                for c in chars.by_ref() {
                    if previous == '*' && c == '/' {
                        break;
                    }
                    if c == '\n' {
                        gap.push('\n');
                    }
                    previous = c;
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
                for c in chars.by_ref() {
                    out.push(c);
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == '"' {
                        break;
                    }
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
    out
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

    // --- The stripper ---

    #[test]
    fn line_and_block_comments_are_removed() {
        let stripped = strip_jsonc(
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
        let stripped = strip_jsonc(r#"{"url": "https://example.com/x", "n": 1}"#);
        let value: serde_json::Value = serde_json::from_str(&stripped).expect("valid JSON");
        assert_eq!(value["url"], "https://example.com/x");
        assert_eq!(value["n"], 1);
    }

    #[test]
    fn a_block_comment_marker_inside_a_string_is_data_too() {
        let stripped = strip_jsonc(r#"{"glob": "src/*/ *.c", "n": 1}"#);
        let value: serde_json::Value = serde_json::from_str(&stripped).expect("valid JSON");
        assert_eq!(value["glob"], "src/*/ *.c");
    }

    #[test]
    fn an_escaped_quote_does_not_end_the_string() {
        let stripped = strip_jsonc(r#"{"say": "she said \"// hi\"", "n": 1}"#);
        let value: serde_json::Value = serde_json::from_str(&stripped).expect("valid JSON");
        assert_eq!(value["say"], r#"she said "// hi""#);
    }

    #[test]
    fn a_string_ending_in_an_escaped_backslash_ends_where_it_looks_like_it_does() {
        let stripped = strip_jsonc(r#"{"path": "C:\\", "n": 1}"#);
        let value: serde_json::Value = serde_json::from_str(&stripped).expect("valid JSON");
        assert_eq!(value["path"], r"C:\");
        assert_eq!(value["n"], 1);
    }

    #[test]
    fn trailing_commas_are_dropped_because_vs_code_accepts_them() {
        let stripped = strip_jsonc(r#"{"args": ["a", "b",], "n": 1,}"#);
        let value: serde_json::Value = serde_json::from_str(&stripped).expect("valid JSON");
        assert_eq!(value["args"][1], "b");
        assert_eq!(value["n"], 1);
    }

    #[test]
    fn a_trailing_comma_followed_by_a_comment_is_still_trailing() {
        let stripped = strip_jsonc(
            r#"{"args": [
                "a", // the only one
            ]}"#,
        );
        let value: serde_json::Value = serde_json::from_str(&stripped).expect("valid JSON");
        assert_eq!(value["args"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn a_comma_inside_a_string_is_not_a_separator() {
        let stripped = strip_jsonc(r#"{"sep": ",", "list": [1]}"#);
        let value: serde_json::Value = serde_json::from_str(&stripped).expect("valid JSON");
        assert_eq!(value["sep"], ",");
    }

    #[test]
    fn stripping_keeps_the_line_numbers_a_reader_would_count() {
        // A parse error has to point at the line the person is looking at.
        let source = "{\n// one\n/* two\nthree */\n\"a\": 1\n}";
        assert_eq!(
            strip_jsonc(source).matches('\n').count(),
            source.matches('\n').count(),
            "comments must leave their newlines behind",
        );
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
    fn a_configuration_for_another_debugger_is_listed_with_its_own_type() {
        let imported = import_str(
            r#"{"configurations": [
                {"type": "python", "request": "launch", "name": "API", "program": "app.py"}
            ]}"#,
        );

        let config = &imported.configs[0];
        assert_eq!(config.adapter, None);
        assert_eq!(config.adapter_type, "python");
        assert!(
            config.not_runnable().is_some(),
            "listing it is useful; pretending lazydap can run it is not",
        );
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
