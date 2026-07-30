//! `lazydap break`: one subcommand, four modes.
//!
//! The blueprint puts setting, listing, removing and toggling under a single
//! `break` (`docs/blueprint/06-cli.md`), which means the mode has to be worked
//! out from the flags. That is done in one place, [`Mode::resolve`], so an
//! ambiguous combination fails with a sentence rather than doing whichever
//! thing the match arm happened to reach first.

use super::{resolve_source, unexpected};
use crate::auto_spawn::ensure_daemon_running;
use crate::error::{CliError, Result};
use crate::instance::Instance;
use crate::output::{OutputFormat, Row, View, or_dash};
use lazydap_core::{BreakpointId, BreakpointSelector, Location, NewBreakpoint};
use lazydap_protocol::{BreakpointAction, BreakpointReport, Request, Response};
use std::path::Path;

/// Everything `lazydap break` accepts, before it has been made sense of.
pub struct BreakArgs {
    pub location: Option<String>,
    pub list: bool,
    pub remove: bool,
    pub toggle: bool,
    pub ids: Vec<u32>,
    pub all: bool,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub log_message: Option<String>,
    pub disabled: bool,
    pub dry_run: bool,
}

/// Which of the four things this invocation is.
enum Mode {
    List,
    Add(NewBreakpoint),
    Remove(BreakpointSelector),
    Toggle(BreakpointSelector),
}

impl Mode {
    fn resolve(args: &BreakArgs) -> Result<Self> {
        if args.list {
            return Ok(Self::List);
        }
        if args.remove {
            return Ok(Self::Remove(selector(args)?));
        }
        if args.toggle {
            return Ok(Self::Toggle(selector(args)?));
        }

        let Some(raw) = &args.location else {
            // Bare `lazydap break` could mean "list" or "you forgot the
            // location". Saying so beats guessing, and the suggestion costs
            // one line.
            return Err(CliError::usage(
                "`lazydap break` needs a location, as `file:line` — \
                 or `--list` to see the ones you have",
            ));
        };
        let location: Location = raw
            .parse()
            .map_err(|error| CliError::usage(format!("{error}")))?;

        Ok(Self::Add(NewBreakpoint {
            source: resolve_source(&location.source)?,
            line: location.line,
            column: None,
            condition: args.condition.clone(),
            hit_condition: args.hit_condition.clone(),
            log_message: args.log_message.clone(),
            enabled: !args.disabled,
        }))
    }
}

/// What a `--remove` or `--toggle` is talking about.
///
/// Deliberately exclusive. `--all --id 3` has two readings — "all of them" and
/// "just that one" — and picking either quietly is how a script ends up
/// deleting every breakpoint in a project.
fn selector(args: &BreakArgs) -> Result<BreakpointSelector> {
    let named = usize::from(args.all)
        + usize::from(!args.ids.is_empty())
        + usize::from(args.location.is_some());
    if named > 1 {
        return Err(CliError::usage(
            "choose one of a location, `--id` or `--all`, not several",
        ));
    }

    if args.all {
        return Ok(BreakpointSelector::All);
    }
    if !args.ids.is_empty() {
        return Ok(BreakpointSelector::Ids(
            args.ids.iter().map(|id| BreakpointId(*id)).collect(),
        ));
    }
    match &args.location {
        Some(raw) => {
            let location: Location = raw
                .parse()
                .map_err(|error| CliError::usage(format!("{error}")))?;
            Ok(BreakpointSelector::Location {
                source: resolve_source(&location.source)?,
                line: location.line,
            })
        }
        None => Err(CliError::usage(
            "say which breakpoints: a `file:line`, `--id N`, or `--all`",
        )),
    }
}

pub async fn run(instance: &Instance, args: BreakArgs, format: OutputFormat) -> Result<()> {
    // Work out what was meant *before* starting a daemon: a usage mistake
    // should not leave a background process behind it.
    let mode = Mode::resolve(&args)?;
    let dry_run = args.dry_run;

    let request = match mode {
        Mode::List => Request::BreakpointList,
        Mode::Add(breakpoint) => Request::BreakpointAdd {
            breakpoint,
            dry_run,
        },
        Mode::Remove(selector) => Request::BreakpointRemove { selector, dry_run },
        Mode::Toggle(selector) => Request::BreakpointToggle { selector, dry_run },
    };

    let mut client = ensure_daemon_running(instance).await?;
    let response = client.request(request).await?;
    let Response::Breakpoints(report) = response else {
        return Err(unexpected(response));
    };

    view(&report).print(format)
}

fn view(report: &BreakpointReport) -> View {
    let rows: Vec<Row> = report
        .breakpoints
        .iter()
        .map(|status| {
            let location = format!(
                "{}:{}",
                short(&status.breakpoint.source),
                status.effective_line(),
            );
            Row::new(
                status.breakpoint.id.to_string(),
                vec![
                    status.breakpoint.id.to_string(),
                    location,
                    status.breakpoint.enabled.to_string(),
                    status.verified.to_string(),
                    or_dash(status.breakpoint.condition.as_ref()),
                ],
                status,
            )
        })
        .collect();

    let json = serde_json::json!({
        "action": report.action,
        "dry_run": report.dry_run,
        "breakpoints": report.breakpoints,
        "not_found": report.not_found,
        "applied_to_session": report.applied_to_session,
    });

    let mut view = View::list(
        json,
        &["id", "location", "enabled", "verified", "condition"],
        rows,
    );
    if let Some(note) = note(report) {
        view = view.with_note(note);
    }
    view
}

/// The line a person needs that the table itself does not say.
fn note(report: &BreakpointReport) -> Option<String> {
    let mut notes = Vec::new();

    if report.dry_run {
        notes.push(format!(
            "dry run: {} {} breakpoint(s), nothing changed",
            report.action.would(),
            report.breakpoints.len(),
        ));
    }
    if !report.not_found.is_empty() {
        notes.push(format!(
            "no breakpoint with id {}",
            report
                .not_found
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    // Only worth saying when something changed: a plain `--list` has no
    // session to apply to and nobody expects it to.
    if !report.dry_run
        && !report.applied_to_session
        && matches!(
            report.action,
            BreakpointAction::Added | BreakpointAction::Removed | BreakpointAction::Toggled
        )
    {
        notes.push("recorded; it will apply to the next `lazydap launch`".to_string());
    }

    (!notes.is_empty()).then(|| notes.join("\n"))
}

/// A path relative to the working directory when that is shorter, which for a
/// project file it always is.
fn short(source: &Path) -> String {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| source.strip_prefix(cwd).ok())
        .unwrap_or(source)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> BreakArgs {
        BreakArgs {
            location: None,
            list: false,
            remove: false,
            toggle: false,
            ids: Vec::new(),
            all: false,
            condition: None,
            hit_condition: None,
            log_message: None,
            disabled: false,
            dry_run: false,
        }
    }

    fn usage_error(args: BreakArgs) -> CliError {
        match Mode::resolve(&args) {
            Err(error) => error,
            Ok(_) => unreachable!("that combination should not have resolved"),
        }
    }

    #[test]
    fn listing_needs_nothing_else() {
        let mode = Mode::resolve(&BreakArgs {
            list: true,
            ..args()
        })
        .expect("resolve");
        assert!(matches!(mode, Mode::List));
    }

    #[test]
    fn removing_by_id_does_not_need_a_file_to_exist() {
        // The point of ids: `break --list --format ids | xargs` has no paths
        // in it, and the file may well have been deleted since.
        let mode = Mode::resolve(&BreakArgs {
            remove: true,
            ids: vec![3, 4],
            ..args()
        })
        .expect("resolve");

        match mode {
            Mode::Remove(BreakpointSelector::Ids(ids)) => {
                assert_eq!(ids, vec![BreakpointId(3), BreakpointId(4)]);
            }
            _ => unreachable!("expected an id selection"),
        }
    }

    #[test]
    fn removing_everything_and_removing_one_thing_cannot_be_asked_for_together() {
        let error = usage_error(BreakArgs {
            remove: true,
            all: true,
            ids: vec![3],
            ..args()
        });
        assert_eq!(
            error.exit_code,
            crate::error::exit::USAGE,
            "picking one quietly is how a project loses every breakpoint",
        );
    }

    #[test]
    fn removing_without_saying_what_is_refused() {
        let error = usage_error(BreakArgs {
            remove: true,
            ..args()
        });
        assert!(error.to_string().contains("--all"), "got: {error}");
    }

    #[test]
    fn a_bare_break_suggests_the_two_things_it_could_have_meant() {
        let error = usage_error(args());
        assert!(error.to_string().contains("file:line"), "got: {error}");
        assert!(error.to_string().contains("--list"), "got: {error}");
    }

    #[test]
    fn a_location_without_a_line_number_is_a_usage_error() {
        let error = usage_error(BreakArgs {
            location: Some("main.c".to_string()),
            ..args()
        });
        assert_eq!(error.exit_code, crate::error::exit::USAGE, "got: {error}");
    }

    #[test]
    fn a_disabled_breakpoint_is_still_recorded() {
        let file = std::env::temp_dir().join(format!("lazydap-bp-{}.c", std::process::id()));
        std::fs::write(&file, "int main(void) { return 0; }\n").expect("write");

        let mode = Mode::resolve(&BreakArgs {
            location: Some(format!("{}:1", file.display())),
            disabled: true,
            ..args()
        })
        .expect("resolve");

        match mode {
            Mode::Add(new) => {
                assert!(!new.enabled);
                assert_eq!(new.line, 1);
            }
            _ => unreachable!("expected an add"),
        }
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn a_dry_run_report_says_what_it_would_have_done() {
        let report = BreakpointReport {
            action: BreakpointAction::Removed,
            dry_run: true,
            breakpoints: Vec::new(),
            not_found: Vec::new(),
            applied_to_session: false,
        };
        let note = note(&report).expect("a note");
        assert!(note.contains("would remove"), "got: {note}");
        assert!(note.contains("nothing changed"), "got: {note}");
    }

    #[test]
    fn a_breakpoint_set_with_no_session_says_when_it_will_take_effect() {
        let report = BreakpointReport {
            action: BreakpointAction::Added,
            dry_run: false,
            breakpoints: Vec::new(),
            not_found: Vec::new(),
            applied_to_session: false,
        };
        let note = note(&report).expect("a note");
        assert!(note.contains("next `lazydap launch`"), "got: {note}");
    }

    #[test]
    fn a_stale_id_is_named_rather_than_silently_matching_nothing() {
        let report = BreakpointReport {
            action: BreakpointAction::Removed,
            dry_run: false,
            breakpoints: Vec::new(),
            not_found: vec![BreakpointId(7)],
            applied_to_session: true,
        };
        let note = note(&report).expect("a note");
        assert!(note.contains("id 7"), "got: {note}");
    }
}
