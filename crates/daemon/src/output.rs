//! Printing a command's result in whichever shape the caller wants.
//!
//! JSON is a product feature with a stable schema (`ARCHITECTURE.md`), so a
//! command builds a [`View`] — its result rendered every way it can be — and
//! this module picks one. Building all the shapes together, in the command
//! that knows what the result means, is what keeps them from disagreeing:
//! there is no path where the table says one thing and the JSON another.
//!
//! Not every result is a list. `--format ids` on a status report has no honest
//! answer, so it is a usage error rather than an empty line.

use crate::error::{CliError, Result};
use clap::ValueEnum;
use lazydap_protocol::{ErrorCode, IpcError};
use std::io::IsTerminal;

/// How to print a command's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum OutputFormat {
    /// Human-readable. Do not parse it.
    Table,
    /// One JSON object. Stable schema — a product feature, not a debug aid.
    Json,
    /// One JSON object per line, for streams and `while read`.
    Jsonl,
    /// For spreadsheets and `cut`.
    Csv,
    /// Bare ids, one per line. What `xargs` wants.
    Ids,
}

impl OutputFormat {
    /// Whether this format needs the result to be a list of things.
    fn needs_rows(&self) -> bool {
        matches!(self, Self::Jsonl | Self::Csv | Self::Ids)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Json => "json",
            Self::Jsonl => "jsonl",
            Self::Csv => "csv",
            Self::Ids => "ids",
        }
    }
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

/// One command's result, in every shape it can be printed.
pub struct View {
    json: serde_json::Value,
    table: String,
    /// Column names, matching each row's cells.
    headers: Vec<String>,
    /// `None` for a result that is one thing rather than a list of them.
    rows: Option<Vec<Row>>,
}

/// One item of a list-shaped result.
pub struct Row {
    /// What `--format ids` prints for this item.
    pub id: String,
    /// Cells, in the order the view's headers name them.
    pub cells: Vec<String>,
    /// What `--format jsonl` prints for this item.
    pub json: serde_json::Value,
}

impl Row {
    /// Build a row from the value it describes.
    ///
    /// The serialisation happens here rather than at each of the callers,
    /// which otherwise all need an opinion about a failure that cannot happen:
    /// every one of these is a plain `serde` struct of owned strings and
    /// numbers. Left to themselves, callers reach for `unwrap_or(Null)` and a
    /// `--format jsonl` stream quietly grows a `null` where a variable should
    /// be.
    pub fn new<T: serde::Serialize>(id: impl Into<String>, cells: Vec<String>, value: &T) -> Self {
        Self {
            id: id.into(),
            cells,
            json: serde_json::to_value(value)
                .expect("lazydap's own output types are always serialisable"),
        }
    }
}

impl View {
    /// A result that is one thing: a status report, a launched session.
    pub fn single(json: serde_json::Value, table: String) -> Self {
        Self {
            json,
            table,
            headers: Vec::new(),
            rows: None,
        }
    }

    /// A result that is a list: breakpoints, frames, variables, threads.
    ///
    /// `json` is still the whole-result object, because `--format json` should
    /// give one object whatever the shape of what is inside it.
    pub fn list(json: serde_json::Value, headers: &[&str], rows: Vec<Row>) -> Self {
        let table = render_table(headers, &rows);
        Self {
            json,
            table,
            headers: headers.iter().map(|header| header.to_string()).collect(),
            rows: Some(rows),
        }
    }

    /// Add a line of explanation for a person reading the table.
    ///
    /// The table only. A note is prose — "recorded; it will apply to the next
    /// launch" — and the JSON already carries the fields it is derived from,
    /// so putting it there too would be a second, less precise copy of the
    /// same fact for a machine to parse.
    pub fn with_note(mut self, note: String) -> Self {
        if self.table.is_empty() {
            self.table = note;
        } else {
            self.table = format!("{}\n\n{note}", self.table);
        }
        self
    }

    pub fn print(&self, format: OutputFormat) -> Result<()> {
        if self.rows.is_none() && format.needs_rows() {
            return Err(not_a_list(format));
        }
        let rows = self.rows.iter().flatten();

        match format {
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&self.json)?),
            OutputFormat::Table => println!("{}", self.table),
            OutputFormat::Jsonl => {
                for row in rows {
                    println!("{}", serde_json::to_string(&row.json)?);
                }
            }
            OutputFormat::Ids => {
                for row in rows {
                    println!("{}", row.id);
                }
            }
            OutputFormat::Csv => {
                // The header line comes from the same list the table renders
                // from, so a column added to one appears in the other.
                println!("{}", csv_line(&self.headers));
                for row in rows {
                    println!("{}", csv_line(&row.cells));
                }
            }
        }
        Ok(())
    }
}

fn not_a_list(format: OutputFormat) -> CliError {
    let mut error = CliError::from(
        IpcError::new(
            ErrorCode::BadRequest,
            format!(
                "`--format {}` prints a list, and this command's result is not one; \
                 use `--format json` or `--format table`",
                format.as_str(),
            ),
        )
        .with_details(serde_json::json!({ "format": format.as_str() })),
    );
    // Choosing a format the command cannot produce is a usage mistake, and a
    // script should be able to tell it apart from a debugger failure.
    error.exit_code = crate::error::exit::USAGE;
    error
}

/// Render `rows` as an aligned table with a header line.
fn render_table(headers: &[&str], rows: &[Row]) -> String {
    let mut widths: Vec<usize> = headers.iter().map(|header| header.len()).collect();
    for row in rows {
        for (index, cell) in row.cells.iter().enumerate() {
            if index < widths.len() {
                widths[index] = widths[index].max(cell.len());
            }
        }
    }

    let mut lines = vec![pad(
        &headers
            .iter()
            .map(|header| header.to_uppercase())
            .collect::<Vec<_>>(),
        &widths,
    )];
    lines.extend(rows.iter().map(|row| pad(&row.cells, &widths)));
    lines.join("\n")
}

fn pad(cells: &[String], widths: &[usize]) -> String {
    cells
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            // The last column is not padded: trailing spaces are invisible
            // damage in a copy-paste.
            match widths.get(index) {
                Some(width) if index + 1 < cells.len() => format!("{cell:<width$}"),
                _ => cell.clone(),
            }
        })
        .collect::<Vec<_>>()
        .join("  ")
        .trim_end()
        .to_string()
}

/// One CSV record, quoted where a naive join would corrupt it.
fn csv_line(cells: &[String]) -> String {
    cells
        .iter()
        .map(|cell| {
            if cell.contains([',', '"', '\n', '\r']) {
                format!("\"{}\"", cell.replace('"', "\"\""))
            } else {
                cell.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Render `rows` as an aligned two-column label/value block.
pub fn render_fields(rows: &[(&str, String)]) -> String {
    let width = rows.iter().map(|(label, _)| label.len()).max().unwrap_or(0);
    rows.iter()
        .map(|(label, value)| format!("{label:<width$}  {value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `-` rather than an empty cell, so a table's columns stay legible.
pub fn or_dash<T: std::fmt::Display>(value: Option<T>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, cells: &[&str]) -> Row {
        Row {
            id: id.to_string(),
            cells: cells.iter().map(|cell| cell.to_string()).collect(),
            json: serde_json::json!({ "id": id }),
        }
    }

    fn listing() -> View {
        View::list(
            serde_json::json!({ "breakpoints": [] }),
            &["id", "location", "enabled"],
            vec![
                row("1", &["1", "main.c:19", "true"]),
                row("2", &["2", "a/very/long/path.c:1", "false"]),
            ],
        )
    }

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

    #[test]
    fn a_table_lines_its_columns_up_on_the_widest_cell() {
        let table = listing().table;
        let lines: Vec<&str> = table.lines().collect();

        assert!(lines[0].starts_with("ID  LOCATION"), "got: {table}");
        assert_eq!(lines.len(), 3, "a header and two rows: {table}");
        assert!(
            lines[1].starts_with("1   main.c:19"),
            "a one-character id still fills the width of its header: {table}",
        );
        assert!(
            lines[1].ends_with("true"),
            "and the location column is as wide as the longest path: {table}",
        );
    }

    #[test]
    fn a_table_row_never_ends_in_trailing_whitespace() {
        // Invisible damage in a copy-paste, and noise in a golden test.
        for line in listing().table.lines() {
            assert_eq!(line, line.trim_end(), "got: {line:?}");
        }
    }

    #[test]
    fn a_format_that_prints_a_list_refuses_a_result_that_is_not_one() {
        let single = View::single(serde_json::json!({ "ok": true }), "ok".to_string());
        let error = match single.print(OutputFormat::Ids) {
            Err(error) => error,
            Ok(()) => unreachable!("a status report has no ids to print"),
        };
        assert_eq!(
            error.exit_code,
            crate::error::exit::USAGE,
            "asking for the impossible is a usage mistake, not a debugger failure",
        );
    }

    #[test]
    fn a_csv_cell_containing_a_comma_is_quoted_rather_than_splitting_the_row() {
        let line = csv_line(&[
            "1".to_string(),
            "main.c:19".to_string(),
            "x > 5, y < 3".to_string(),
        ]);
        assert_eq!(line, r#"1,main.c:19,"x > 5, y < 3""#, "got: {line}");
    }

    #[test]
    fn a_csv_cell_containing_a_quote_doubles_it() {
        let line = csv_line(&[r#"say "hi""#.to_string()]);
        assert_eq!(line, r#""say ""hi""""#, "got: {line}");
    }

    #[test]
    fn a_missing_value_prints_as_a_dash_rather_than_nothing() {
        assert_eq!(or_dash(None::<i32>), "-");
        assert_eq!(or_dash(Some(7)), "7");
    }
}
