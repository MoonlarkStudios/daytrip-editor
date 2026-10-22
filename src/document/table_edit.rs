//! Table operations preserve source delimiters and untouched cell formatting.

use crate::document::projection::{ProjectedEdit, Projection};
use crate::document::rich_document::Table;
use crate::document::selection::Selection;

pub fn cell_at(projection: &Projection, selection: Selection) -> Option<(&Table, usize, usize)> {
    let selected = projection.display_selection(selection).range();
    let index = projection
        .rich
        .tables
        .partition_point(|table| table.range.end < selected.start);
    if let Some(table) = projection.rich.tables.get(index)
        && table.range.start <= selected.start
        && selected.end <= table.range.end
    {
        let first = table
            .rows
            .partition_point(|row| row.last().is_some_and(|cell| cell.end < selected.start));
        for (row_index, row) in table.rows.iter().enumerate().skip(first).take(1) {
            for (column, cell) in row.iter().enumerate() {
                if cell.start <= selected.start && selected.end <= cell.end {
                    return Some((table, row_index, column));
                }
            }
        }
    }
    None
}

/// Paste a rectangle of TSV/plain lines into cells. The first cell honors the
/// current insertion point/selection; subsequent cells replace their contents.
pub fn paste(
    projection: &Projection,
    source: &str,
    selection: Selection,
    text: &str,
) -> Option<ProjectedEdit> {
    let (table, row, column) = cell_at(projection, selection)?;
    let columns = table.alignment.len();
    if columns == 0 {
        return None;
    }
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let normalized = normalized.strip_suffix('\n').unwrap_or(&normalized);
    let mut inputs = Vec::new();
    let mut target_row = row;
    for line in normalized.split('\n') {
        let mut last_row = target_row;
        for (column_delta, value) in line.split('\t').enumerate() {
            let column_offset = column + column_delta;
            last_row = target_row + column_offset / columns;
            inputs.push((last_row, column_offset % columns, escape_pipes(value)));
        }
        target_row = last_row + 1;
    }
    let (last_row, last_column, _) = inputs.last()?;
    let (last_row, last_column) = (*last_row, *last_column);
    let mut changes = Vec::new();
    let mut additions = Vec::<Vec<String>>::new();
    let mut first = true;
    let mut caret = selection.head;
    for (row, column, value) in inputs {
        if let Some(cell) = table.source_rows.get(row).and_then(|row| row.get(column)) {
            let range = if first {
                selection.range()
            } else {
                cell.clone()
            };
            // Native/source affinity may put an empty-cell caret in surrounding
            // whitespace; it must stay inside that cell's source boundaries.
            let range =
                range.start.max(cell.start).min(cell.end)..range.end.max(cell.start).min(cell.end);
            caret = range.start + value.len();
            changes.push((range, value));
        } else {
            let index = row.saturating_sub(table.source_rows.len());
            while additions.len() <= index {
                additions.push(vec![String::new(); columns]);
            }
            additions[index][column] = value;
        }
        first = false;
    }
    changes.sort_by_key(|(range, _)| range.start);
    let mut replacement = source[table.source_range.clone()].to_owned();
    if let Some((last, value)) = changes.last() {
        let shift = changes
            .iter()
            .take(changes.len().saturating_sub(1))
            .map(|(range, value)| value.len() as isize - range.len() as isize)
            .sum::<isize>();
        caret = (last.start as isize + shift) as usize + value.len();
    }
    for (range, value) in changes.into_iter().rev() {
        replacement.replace_range(
            range.start - table.source_range.start..range.end - table.source_range.start,
            &value,
        );
    }
    let prefix = row_prefix(source, table);
    if !additions.is_empty() && !replacement.ends_with('\n') {
        replacement.push('\n');
    }
    for (index, row) in additions.into_iter().enumerate() {
        replacement.push_str(&prefix);
        replacement.push('|');
        for (column, value) in row.into_iter().enumerate() {
            replacement.push(' ');
            replacement.push_str(&value);
            if index + table.source_rows.len() == last_row && column == last_column {
                caret =
                    table.source_range.start + replacement.len() + usize::from(value.is_empty());
            }
            replacement.push_str(" |");
        }
        replacement.push('\n');
    }
    Some(ProjectedEdit {
        range: table.source_range.clone(),
        text: replacement,
        selection: Selection::caret(caret),
    })
}

pub fn row_prefix(source: &str, table: &Table) -> String {
    let start = source[..table.source_range.start]
        .rfind('\n')
        .map_or(0, |n| n + 1);
    source[start..table.source_range.start]
        .chars()
        .map(|c| if c == '>' || c == '\t' { c } else { ' ' })
        .collect()
}

fn escape_pipes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut backslashes = 0;
    for c in text.chars() {
        if c == '|' && backslashes % 2 == 0 {
            result.push('\\');
        }
        result.push(c);
        backslashes = if c == '\\' { backslashes + 1 } else { 0 };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::projection::Affinity;

    #[test]
    fn rectangular_paste_preserves_unedited_markdown_and_adds_rows() {
        let source = "Before\n\n| A | B |\n| :--- | ---: |\n| **left** | *right* |\n\nAfter";
        let projection = Projection::new(source);
        let cell = &projection.rich.tables[0].rows[1][0];
        let selected = Selection {
            anchor: projection.to_source(cell.start, Affinity::After, None),
            head: projection.to_source(cell.end, Affinity::Before, None),
        };
        let edit = paste(&projection, source, selected, "one\ttwo\nthree\tfour\n").unwrap();
        let mut result = source.to_owned();
        result.replace_range(edit.range, &edit.text);
        assert!(result.starts_with("Before\n\n| A | B |\n| :--- | ---: |\n| **one** | two |\n"));
        assert!(result.ends_with("| three | four |\n\nAfter"));
        assert_eq!(
            &result[edit.selection.head - 4..edit.selection.head],
            "four"
        );
        let projected = Projection::new(&result);
        assert_eq!(projected.rich.tables[0].rows.len(), 3);
        assert_eq!(
            projected.text,
            "Before\n\nA\tB\none\ttwo\nthree\tfour\n\nAfter"
        );
    }

    #[test]
    fn pipes_and_empty_pasted_cells_do_not_change_column_count() {
        let source = "| A | B |\n|---|---|\n| left | right |\n";
        let projection = Projection::new(source);
        let point = source.find("left").unwrap();
        let edit = paste(&projection, source, Selection::caret(point), "x|y\t\n\t").unwrap();
        let mut result = source.to_owned();
        result.replace_range(edit.range, &edit.text);
        let projected = Projection::new(&result);
        assert!(result.contains("x\\|yleft"));
        assert!(
            projected.rich.tables[0]
                .rows
                .iter()
                .all(|row| row.len() == 2)
        );
        assert_eq!(
            cell_at(&projected, edit.selection).map(|(_, row, column)| (row, column)),
            Some((2, 1))
        );
    }
}
