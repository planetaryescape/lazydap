//! `lazydap launches`: the named ways to start this project.
//!
//! Two sources, one list (D008, and the blueprint's `08-state-and-config.md`):
//! `.lazydap/state.toml` for lazydap's own, `.vscode/launch.json` for the ones
//! the repository already had. On a name collision lazydap's own wins, with a
//! warning — the file lazydap owns is the one somebody chose to write.
//!
//! Resolved entirely client-side, like every other path (see the module
//! comment on [`crate::commands`]). Both files are found by walking up from
//! the working directory, and the daemon's is wherever it happened to be
//! started; asking it to read them would find a different project's, or none.
//! `run` sends the same `Launch` request `lazydap launch` sends, so nothing
//! about launching moves into the daemon (D047).

use crate::commands::session::{self, LaunchOptions};
use crate::error::{CliError, Result};
use crate::instance::Instance;
use crate::output::{OutputFormat, Row, View};
use lazydap_config::launch_json;
use lazydap_core::LaunchConfig;
use lazydap_protocol::{ErrorCode, IpcError};
use lazydap_store::ProjectStore;
use std::path::Path;

/// Every configuration lazydap can see for this project, and everything that
/// went wrong reading them.
struct Catalogue {
    configs: Vec<LaunchConfig>,
    warnings: Vec<String>,
}

pub async fn list(instance: &Instance, format: OutputFormat) -> Result<()> {
    let catalogue = collect(&instance.project_root)?;

    let rows: Vec<Row> = catalogue.configs.iter().map(row).collect();
    let json = serde_json::json!({
        "configs": catalogue.configs.iter().map(describe).collect::<Vec<_>>(),
        "warnings": catalogue.warnings,
    });

    let view = View::list(
        json,
        &[
            "name", "source", "adapter", "request", "program", "runnable",
        ],
        rows,
    );
    match note(&catalogue) {
        Some(note) => view.with_note(note),
        None => view,
    }
    .print(format)
}

/// The line under the table, for a person: what went wrong, or why there is no
/// table.
///
/// The table only — the JSON carries `warnings` as an array already, and a
/// second prose copy of the same fact is one more thing to keep in step.
fn note(catalogue: &Catalogue) -> Option<String> {
    if !catalogue.warnings.is_empty() {
        return Some(
            catalogue
                .warnings
                .iter()
                .map(|warning| format!("warning: {warning}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    // An empty table with nothing under it reads as a failure. Say where it
    // looked instead.
    catalogue.configs.is_empty().then(|| {
        format!(
            "no launch configurations in {} or {}",
            lazydap_store::STATE_FILE,
            launch_json::LAUNCH_JSON_PATH,
        )
    })
}

/// Start the configuration called `name`.
pub async fn run(
    instance: &Instance,
    name: &str,
    stop_on_entry: bool,
    format: OutputFormat,
) -> Result<()> {
    let catalogue = collect(&instance.project_root)?;

    let config = catalogue
        .configs
        .iter()
        .find(|config| config.name == name)
        .ok_or_else(|| unknown_name(name, &catalogue.configs))?;

    // The same answer `launches list` prints in its `runnable` column, from
    // the same function, so the list cannot promise something `run` refuses.
    let program = config.runnable_program().map_err(|reason| {
        CliError::from(
            IpcError::new(
                ErrorCode::InvalidLaunchConfig,
                format!("cannot run `{name}`: {reason}"),
            )
            .with_details(serde_json::json!({
                "name": name,
                "source": config.source.as_str(),
                "adapter_type": config.adapter_type,
                "unresolved": config.unresolved,
            })),
        )
    })?;

    // Warnings go to stderr, not into the result: stdout belongs to the JSON
    // the caller is parsing.
    for warning in &catalogue.warnings {
        eprintln!("warning: {warning}");
    }

    session::launch(
        instance,
        LaunchOptions {
            program: program.to_path_buf(),
            args: config.args.clone(),
            // A configuration without one runs where VS Code would run it:
            // the project root, not wherever the shell happens to be.
            cwd: Some(
                config
                    .cwd
                    .clone()
                    .unwrap_or_else(|| instance.project_root.clone()),
            ),
            env: config.env.clone(),
            adapter: config.adapter,
            // The interpreter the configuration insists on, when it names one
            // (`python` / `pythonPath`). It replaces discovery rather than
            // seeding it: a virtualenv is named precisely because the one on
            // `PATH` is the wrong one.
            adapter_command: config.adapter_command.clone(),
            // `--stop-on-entry` on the command line adds to the configuration
            // rather than replacing it: nobody asks for it and means "and turn
            // the file's setting off".
            stop_on_entry: config.stop_on_entry || stop_on_entry,
        },
        format,
    )
    .await
}

/// Both sources, lazydap's own first.
fn collect(root: &Path) -> Result<Catalogue> {
    let store = ProjectStore::load(root).map_err(CliError::general)?;
    let (configs, mut warnings) = store.launch_configs();

    let imported = launch_json::import(root)?;
    warnings.extend(imported.warnings);

    let mut catalogue = Catalogue { configs, warnings };
    for config in imported.configs {
        if catalogue
            .configs
            .iter()
            .any(|existing| existing.name == config.name)
        {
            catalogue.warnings.push(format!(
                "`{}` is in both {} and {}; lazydap's own is the one that runs",
                config.name,
                lazydap_store::STATE_FILE,
                launch_json::LAUNCH_JSON_PATH,
            ));
            continue;
        }
        catalogue.configs.push(config);
    }
    Ok(catalogue)
}

/// The error for a name that is not there, listing the ones that are.
///
/// A bare "not found" from a command whose whole job is names is a wasted
/// round trip: the next thing anyone types is `launches list`.
fn unknown_name(name: &str, configs: &[LaunchConfig]) -> CliError {
    let known: Vec<&str> = configs.iter().map(|config| config.name.as_str()).collect();
    let message = if known.is_empty() {
        format!("no launch configuration is called `{name}`, and this project has none")
    } else {
        format!(
            "no launch configuration is called `{name}`. There is: {}",
            known.join(", "),
        )
    };
    CliError::from(
        IpcError::new(ErrorCode::InvalidLaunchConfig, message)
            .with_details(serde_json::json!({ "name": name, "available": known })),
    )
}

fn describe(config: &LaunchConfig) -> serde_json::Value {
    let reason = config.not_runnable();
    serde_json::json!({
        "name": config.name,
        "source": config.source.as_str(),
        "adapter": config.adapter.map(|adapter| adapter.as_str()),
        "adapter_type": config.adapter_type,
        "request": config.kind.as_str(),
        "program": config.program,
        "args": config.args,
        "cwd": config.cwd,
        "env": config.env,
        "stop_on_entry": config.stop_on_entry,
        "runnable": reason.is_none(),
        "not_runnable_reason": reason.map(|reason| reason.to_string()),
        "unresolved": config.unresolved,
    })
}

fn row(config: &LaunchConfig) -> Row {
    Row::new(
        config.name.clone(),
        vec![
            config.name.clone(),
            config.source.as_str().to_string(),
            config.adapter_type.clone(),
            config.kind.as_str().to_string(),
            config
                .program
                .as_ref()
                .map(|program| program.display().to_string())
                .unwrap_or_else(|| "-".to_string()),
            match config.not_runnable() {
                None => "yes".to_string(),
                Some(reason) => format!("no ({reason})"),
            },
        ],
        &describe(config),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazydap_core::{AdapterKind, LaunchConfigSource, LaunchKind};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// A project directory that deletes itself.
    struct TempProject(PathBuf);

    impl TempProject {
        fn new(label: &str) -> Self {
            static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "lazydap-launches-{label}-{}-{unique}",
                std::process::id(),
            ));
            std::fs::create_dir_all(&path).expect("create project");
            Self(path)
        }

        fn write(&self, relative: &str, body: &str) {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("create parent");
            std::fs::write(path, body).expect("write");
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn config(name: &str, source: LaunchConfigSource) -> LaunchConfig {
        LaunchConfig {
            name: name.to_string(),
            adapter: Some(AdapterKind::Codelldb),
            adapter_type: "lldb".to_string(),
            kind: LaunchKind::Launch,
            program: Some(PathBuf::from("/p/app")),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            stop_on_entry: false,
            adapter_command: None,
            source,
            unresolved: Vec::new(),
            blocked: None,
        }
    }

    #[test]
    fn both_files_feed_one_list() {
        let project = TempProject::new("both");
        project.write(
            ".lazydap/state.toml",
            "[[launch_configs]]\nname = \"from state\"\nprogram = \"app\"\n",
        );
        project.write(
            ".vscode/launch.json",
            r#"{"configurations": [
                {"type": "lldb", "request": "launch", "name": "from vscode",
                 "program": "${workspaceFolder}/app"}
            ]}"#,
        );

        let catalogue = collect(project.path()).expect("collect");

        let names: Vec<&str> = catalogue
            .configs
            .iter()
            .map(|config| config.name.as_str())
            .collect();
        assert_eq!(names, vec!["from state", "from vscode"]);
        assert!(
            catalogue.warnings.is_empty(),
            "got: {:?}",
            catalogue.warnings
        );
    }

    #[test]
    fn a_project_with_neither_file_lists_nothing_rather_than_failing() {
        let project = TempProject::new("empty");
        let catalogue = collect(project.path()).expect("collect");

        assert!(catalogue.configs.is_empty());
        assert!(catalogue.warnings.is_empty());
    }

    #[test]
    fn a_name_in_both_files_resolves_to_lazydap_s_own_with_a_warning() {
        let project = TempProject::new("clash");
        project.write(
            ".lazydap/state.toml",
            "[[launch_configs]]\nname = \"main\"\nprogram = \"ours\"\n",
        );
        project.write(
            ".vscode/launch.json",
            r#"{"configurations": [
                {"type": "lldb", "request": "launch", "name": "main", "program": "theirs"}
            ]}"#,
        );

        let catalogue = collect(project.path()).expect("collect");

        assert_eq!(catalogue.configs.len(), 1);
        assert_eq!(
            catalogue.configs[0].program,
            Some(project.path().join("ours")),
        );
        assert!(
            catalogue.warnings[0].contains("main"),
            "silently picking one of two things called the same is how you debug the wrong \
             binary: {:?}",
            catalogue.warnings,
        );
    }

    #[test]
    fn an_unknown_name_lists_the_ones_that_exist() {
        let error = unknown_name(
            "typo",
            &[
                config("Debug binary", LaunchConfigSource::VsCodeLaunchJson),
                config("Debug tests", LaunchConfigSource::ProjectState),
            ],
        );
        let message = error.to_string();

        assert!(message.contains("Debug binary"), "got: {message}");
        assert!(message.contains("Debug tests"), "got: {message}");
    }

    #[test]
    fn an_unknown_name_in_a_project_with_no_configurations_says_that_instead() {
        let error = unknown_name("anything", &[]);
        assert!(error.to_string().contains("has none"), "got: {error}");
    }

    #[test]
    fn the_table_and_the_json_agree_about_whether_a_config_can_run() {
        let unrunnable = LaunchConfig {
            adapter: None,
            adapter_type: "python".to_string(),
            ..config("API", LaunchConfigSource::VsCodeLaunchJson)
        };

        let json = describe(&unrunnable);
        let row = row(&unrunnable);

        assert_eq!(json["runnable"], false);
        assert!(
            row.cells.last().expect("a runnable cell").starts_with("no"),
            "got: {:?}",
            row.cells,
        );
        assert!(
            json["not_runnable_reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("python")),
            "got: {json}",
        );
    }

    #[test]
    fn a_state_file_that_is_not_toml_at_all_is_an_error_rather_than_an_empty_list() {
        // The store's own rule: a malformed state file must not read as "you
        // had nothing".
        let project = TempProject::new("broken");
        project.write(".lazydap/state.toml", "[[launch_configs\nname =");

        assert!(collect(project.path()).is_err());
    }
}
