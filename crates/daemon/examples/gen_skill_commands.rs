//! Generate `skill/references/commands.md` from the clap command tree.
//!
//! An example rather than a second `[[bin]]`: this is build tooling, and a
//! binary in the product crate would be installed onto users' machines by
//! `cargo install` for no reason. Examples are compiled by
//! `cargo check --all-targets`, so it still cannot rot silently. See D035.
//!
//! It walks the real `Cli` type, not `--help` output. Parsing help text would
//! be a second, worse parser for something clap already models, and would
//! quietly lose whatever it did not understand.
//!
//!     cargo run --example gen_skill_commands -- skill/references/commands.md
//!
//! `scripts/build-skill.sh` runs this; CI runs that and fails if the result
//! differs from what is committed.

use clap::{Arg, ArgAction, Command, CommandFactory};
use lazydap_daemon::cli::Cli;
use std::fmt::Write as _;

fn main() {
    let destination = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "skill/references/commands.md".to_string());

    let mut command = Cli::command();
    command.build();

    let rendered = render(&command);
    if let Some(parent) = std::path::Path::new(&destination).parent() {
        std::fs::create_dir_all(parent).expect("create the references directory");
    }
    std::fs::write(&destination, rendered).expect("write commands.md");
    eprintln!("wrote {destination}");
}

fn render(root: &Command) -> String {
    let mut out = String::new();

    out.push_str(
        "# lazydap commands\n\n\
         Generated from lazydap's own argument parser by\n\
         `cargo run --example gen_skill_commands`. Do not edit by hand — edit\n\
         `crates/daemon/src/cli.rs` and rebuild the skill.\n\n\
         Every command accepts the global flags below, and every command that\n\
         prints a result accepts `--format`. Exit codes are in\n\
         [`error-codes.md`](error-codes.md); the JSON shapes are in\n\
         [`output-schemas.md`](output-schemas.md).\n\n",
    );

    out.push_str("## Global flags\n\n");
    push_arguments(&mut out, root, Scope::Root);

    out.push_str("\n## Commands\n\n");
    for sub in visible_subcommands(root) {
        push_command(&mut out, sub, "");
    }

    out
}

/// Whether the global flags belong in this table.
///
/// clap copies them onto every subcommand, which is right for `--help` and
/// wrong for a reference an agent reads top to bottom: the same two rows on
/// twenty commands bury the rows that differ.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Root,
    Subcommand,
}

/// One command, then everything under it.
///
/// Recursive because a subcommand's subcommands are the surface too: an agent
/// that reads this file and never sees `launches list` cannot call it, and the
/// whole point of generating the file is that it cannot omit what the parser
/// accepts. `path` is what a caller types before this command's own name.
fn push_command(out: &mut String, command: &Command, path: &str) {
    let name = command.get_name();
    let full = if path.is_empty() {
        name.to_string()
    } else {
        format!("{path} {name}")
    };
    // `###` for a top-level command, one deeper per level below it.
    let heading = "#".repeat(2 + full.split(' ').count());
    let _ = writeln!(out, "{heading} `lazydap {full}`");
    out.push('\n');

    let aliases: Vec<&str> = command.get_visible_aliases().collect();
    if !aliases.is_empty() {
        let _ = writeln!(out, "*Also spelled:* {}\n", code_list(&aliases));
    }

    if let Some(about) = command.get_long_about().or_else(|| command.get_about()) {
        let _ = writeln!(out, "{}\n", about.to_string().trim());
    }

    let _ = writeln!(out, "```\n{}\n```\n", usage(command));

    push_arguments(out, command, Scope::Subcommand);
    out.push('\n');

    for sub in visible_subcommands(command) {
        push_command(out, sub, &full);
    }
}

/// The subcommands worth documenting.
///
/// `help` is clap's own, sits under every command that has subcommands, and
/// carries a copy of each of its siblings. Following it would fill the file
/// with sections describing how to read the file.
fn visible_subcommands(command: &Command) -> impl Iterator<Item = &Command> {
    command
        .get_subcommands()
        .filter(|sub| sub.get_name() != "help" && !sub.is_hide_set())
}

/// The usage line, with clap's styling stripped.
///
/// `render_usage` returns a `StyledStr` whose `Display` already drops the ANSI
/// codes when styling is off, which it is here — but going through
/// `to_string` rather than the raw buffer is what guarantees that.
fn usage(command: &Command) -> String {
    let mut command = command.clone();
    command.render_usage().to_string().trim().to_string()
}

fn push_arguments(out: &mut String, command: &Command, scope: Scope) {
    let arguments: Vec<&Arg> = command
        .get_arguments()
        // `--help` and `--version` are on every command and are not part of
        // the tool surface an agent is choosing between.
        .filter(|arg| !matches!(arg.get_id().as_str(), "help" | "version"))
        .filter(|arg| scope == Scope::Root || !arg.is_global_set())
        .collect();

    if arguments.is_empty() {
        return;
    }

    out.push_str("| Argument | Required | Default | Description |\n");
    out.push_str("|---|---|---|---|\n");
    for arg in arguments {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} |",
            spelling(arg),
            if arg.is_required_set() { "yes" } else { "no" },
            default_of(arg),
            help_of(arg),
        );
    }
}

/// How a caller writes this argument.
fn spelling(arg: &Arg) -> String {
    if arg.is_positional() {
        return format!("`<{}>`", value_name(arg));
    }

    let mut forms = Vec::new();
    if let Some(short) = arg.get_short() {
        forms.push(format!("-{short}"));
    }
    if let Some(long) = arg.get_long() {
        forms.push(format!("--{long}"));
    }

    let flag = matches!(
        arg.get_action(),
        ArgAction::SetTrue | ArgAction::SetFalse | ArgAction::Count
    );
    let joined = forms.join(", ");
    if flag {
        format!("`{joined}`")
    } else {
        format!("`{joined} <{}>`", value_name(arg))
    }
}

fn value_name(arg: &Arg) -> String {
    arg.get_value_names()
        .and_then(|names| names.first())
        .map(|name| name.to_string())
        .unwrap_or_else(|| arg.get_id().as_str().to_uppercase())
}

fn default_of(arg: &Arg) -> String {
    let defaults: Vec<String> = arg
        .get_default_values()
        .iter()
        .map(|value| format!("`{}`", value.to_string_lossy()))
        .collect();

    if defaults.is_empty() {
        "-".to_string()
    } else {
        defaults.join(", ")
    }
}

/// One-line help, with newlines flattened so the table row survives, and the
/// accepted values spelled out.
///
/// The values matter more here than in `--help`: an agent choosing a flag from
/// a document cannot try one and read the error.
fn help_of(arg: &Arg) -> String {
    let mut help = arg
        .get_long_help()
        .or_else(|| arg.get_help())
        .map(|help| {
            help.to_string()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                // A pipe in help text would end the table cell early.
                .replace('|', "\\|")
        })
        .unwrap_or_default();

    let values: Vec<String> = arg
        .get_possible_values()
        .iter()
        .map(|value| format!("`{}`", value.get_name()))
        .collect();
    if !values.is_empty() {
        if !help.is_empty() && !help.ends_with('.') {
            help.push('.');
        }
        let _ = write!(help, " One of: {}", values.join(", "));
    }

    help.trim().to_string()
}

fn code_list(items: &[&str]) -> String {
    items
        .iter()
        .map(|item| format!("`{item}`"))
        .collect::<Vec<_>>()
        .join(", ")
}
