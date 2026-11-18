//! `racksmith lifecycle`: print the machine lifecycle transition table.
//!
//! The output is generated from [`TRANSITIONS`], is byte-for-byte deterministic and ends with a
//! newline. `cargo xtask docs` embeds the Markdown form in `docs/lifecycle.md` and the TLA+ form
//! in `specs/tla/MachineLifecycle.tla`, so neither the docs nor the model can drift from the code.

use std::io::{self, Write};

use racksmith_domain::{MachineEvent, MachineState, TRANSITIONS};

use crate::cli::LifecycleFormat;

/// Column headings of the Markdown table.
const HEADINGS: [&str; 3] = ["From", "Event", "To"];

/// Writes the lifecycle transition table to `out` in `format`.
///
/// # Errors
///
/// Returns an error if writing to `out` fails.
pub(crate) fn run(format: LifecycleFormat, out: &mut impl Write) -> io::Result<()> {
    match format {
        LifecycleFormat::Markdown => write_table(out, TRANSITIONS),
        LifecycleFormat::Tla => write_tla(out, TRANSITIONS),
    }
}

/// Writes `transitions` as the TLA+ definition of `Transitions`: a set of `<<from, event, to>>`
/// triples with aligned columns, naming states and events by their Rust variant names.
fn write_tla(
    out: &mut impl Write,
    transitions: &[(MachineState, MachineEvent, MachineState)],
) -> io::Result<()> {
    let rows: Vec<[String; 3]> = transitions
        .iter()
        .map(|&(from, event, to)| {
            [
                format!("\"{}\",", pascal_case(from.as_str())),
                format!("\"{}\",", pascal_case(event.as_str())),
                format!("\"{}\"", pascal_case(to.as_str())),
            ]
        })
        .collect();
    let widths = column_widths(&rows, [0; 3]);
    writeln!(out, "Transitions ==")?;
    if rows.is_empty() {
        return writeln!(out, "    {{}}");
    }
    let last = rows.len() - 1;
    for (index, [from, event, to]) in rows.iter().enumerate() {
        let open = if index == 0 { "    { " } else { "      " };
        let close = if index == last { " }" } else { "," };
        let [from_width, event_width, _] = widths;
        writeln!(
            out,
            "{open}<<{from:<from_width$} {event:<event_width$} {to}>>{close}"
        )?;
    }
    Ok(())
}

/// Converts a `snake_case` wire name to the `PascalCase` Rust variant name.
fn pascal_case(snake: &str) -> String {
    snake
        .split('_')
        .flat_map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_ascii_uppercase())
                .into_iter()
                .chain(chars)
        })
        .collect()
}

/// The width of each column: the longest cell, but at least `minimum`.
fn column_widths(rows: &[[String; 3]], minimum: [usize; 3]) -> [usize; 3] {
    let mut widths = minimum;
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.len());
        }
    }
    widths
}

/// Writes `transitions` as a Markdown table with aligned columns and code-formatted names.
fn write_table(
    out: &mut impl Write,
    transitions: &[(MachineState, MachineEvent, MachineState)],
) -> io::Result<()> {
    let rows: Vec<[String; 3]> = transitions
        .iter()
        .map(|&(from, event, to)| [code(from.as_str()), code(event.as_str()), code(to.as_str())])
        .collect();
    let widths = column_widths(&rows, HEADINGS.map(str::len));

    write_row(out, &HEADINGS, &widths)?;
    write_separator(out, &widths)?;
    rows.iter().try_for_each(|row| write_row(out, row, &widths))
}

/// Formats `name` as inline code.
fn code(name: &str) -> String {
    format!("`{name}`")
}

/// Writes one table row, padding each cell to its column width.
fn write_row(
    out: &mut impl Write,
    cells: &[impl AsRef<str>; 3],
    widths: &[usize; 3],
) -> io::Result<()> {
    for (cell, &width) in cells.iter().zip(widths) {
        write!(out, "| {:<width$} ", cell.as_ref())?;
    }
    writeln!(out, "|")
}

/// Writes the row separating the headings from the body.
fn write_separator(out: &mut impl Write, widths: &[usize; 3]) -> io::Result<()> {
    for &width in widths {
        write!(out, "|{}", "-".repeat(width + 2))?;
    }
    writeln!(out, "|")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(transitions: &[(MachineState, MachineEvent, MachineState)]) -> String {
        let mut out = Vec::new();
        write_table(&mut out, transitions).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn renders_aligned_markdown_table() {
        let table = render(&[
            (
                MachineState::Ready,
                MachineEvent::Allocate,
                MachineState::Allocated,
            ),
            (
                MachineState::Broken,
                MachineEvent::MarkFixed,
                MachineState::Ready,
            ),
        ]);
        let expected = "\
| From     | Event        | To          |
|----------|--------------|-------------|
| `ready`  | `allocate`   | `allocated` |
| `broken` | `mark_fixed` | `ready`     |
";
        assert_eq!(table, expected, "rendered table");
    }

    #[test]
    fn headings_set_the_minimum_width() {
        let table = render(&[]);
        assert_eq!(
            table, "| From | Event | To |\n|------|-------|----|\n",
            "empty table"
        );
    }

    fn render_tla(transitions: &[(MachineState, MachineEvent, MachineState)]) -> String {
        let mut out = Vec::new();
        write_tla(&mut out, transitions).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn renders_aligned_tla_set() {
        let tla = render_tla(&[
            (
                MachineState::Ready,
                MachineEvent::Allocate,
                MachineState::Allocated,
            ),
            (
                MachineState::Broken,
                MachineEvent::MarkFixed,
                MachineState::Ready,
            ),
        ]);
        let expected = "\
Transitions ==
    { <<\"Ready\",  \"Allocate\",  \"Allocated\">>,
      <<\"Broken\", \"MarkFixed\", \"Ready\">> }
";
        assert_eq!(tla, expected, "rendered TLA+");
        assert_eq!(render_tla(&[]), "Transitions ==\n    {}\n", "empty set");
    }

    #[test]
    fn tla_names_are_the_rust_variant_names() {
        assert_eq!(
            pascal_case("commissioning_succeeded"),
            "CommissioningSucceeded"
        );
        assert_eq!(pascal_case("new"), "New");
    }

    #[test]
    fn full_table_has_one_row_per_transition_and_ends_with_newline() {
        let mut out = Vec::new();
        run(LifecycleFormat::Markdown, &mut out).unwrap();
        let table = String::from_utf8(out).unwrap();
        assert!(table.ends_with('\n'), "ends with a newline");
        assert_eq!(
            table.lines().count(),
            TRANSITIONS.len() + 2,
            "heading + separator + rows"
        );
        let width = table.lines().next().unwrap().len();
        assert!(
            table.lines().all(|line| line.len() == width),
            "columns are aligned"
        );
    }

    #[test]
    fn output_is_deterministic() {
        for format in [LifecycleFormat::Markdown, LifecycleFormat::Tla] {
            let mut first = Vec::new();
            let mut second = Vec::new();
            run(format, &mut first).unwrap();
            run(format, &mut second).unwrap();
            assert_eq!(first, second, "two {format:?} runs produce identical bytes");
        }
    }
}
