use clap::ValueEnum;
use std::io::IsTerminal;

/// How to print a command's result.
///
/// `jsonl`, `csv` and `ids` are in the documented CLI surface but land with
/// M6: nothing M5 prints is a list, and a format that renders one row is not
/// worth four spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum OutputFormat {
    /// Human-readable. Do not parse it.
    Table,
    /// One JSON object. Stable schema — a product feature, not a debug aid.
    Json,
}

/// Honour `--format` if given, otherwise guess from where stdout is going.
///
/// Piping into `jq` should not require a flag, and neither should reading the
/// output yourself.
pub fn resolve_format(explicit: Option<OutputFormat>) -> OutputFormat {
    explicit.unwrap_or_else(|| {
        if std::io::stdout().is_terminal() {
            OutputFormat::Table
        } else {
            OutputFormat::Json
        }
    })
}

/// Print a value as pretty JSON on stdout.
pub fn print_json(value: &serde_json::Value) -> std::io::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Render `rows` as an aligned two-column table.
pub fn render_fields(rows: &[(&str, String)]) -> String {
    let width = rows.iter().map(|(label, _)| label.len()).max().unwrap_or(0);
    rows.iter()
        .map(|(label, value)| format!("{label:<width$}  {value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explicit_format_wins_over_the_terminal_guess() {
        assert_eq!(
            resolve_format(Some(OutputFormat::Table)),
            OutputFormat::Table
        );
    }

    #[test]
    fn fields_line_up_on_the_longest_label() {
        let rendered = render_fields(&[
            ("state", "paused".to_string()),
            ("session_id", "abc".to_string()),
        ]);
        assert_eq!(
            rendered, "state       paused\nsession_id  abc",
            "got: {rendered}"
        );
    }
}
