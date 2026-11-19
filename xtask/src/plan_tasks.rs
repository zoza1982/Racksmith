//! The task IDs of the M1 plan, the only tracking references that `requirements.toml` and the
//! `[subsystems]` of `architecture.toml` accept.
//!
//! A task is a row of a Markdown table in [`PLAN_FILE`] whose first cell is a task ID: `A-01`,
//! `C-03b`, or `M1-H-01` (rows of the work breakdown omit the `M1-` prefix, the lab tasks of §2
//! write it). References always use the prefixed form. A task is done when its ID cell is struck
//! through (`~~A-01~~`); referencing a done task is an error, because the gap it tracked must then
//! be closed.

use std::collections::BTreeMap;
use std::fmt;

use anyhow::{Result, bail};

/// The plan whose tasks are the tracking references, relative to the workspace root.
pub(crate) const PLAN_FILE: &str = "docs/plan/m1.md";

/// The milestone prefix every reference carries.
const PREFIX: &str = "M1-";

/// Every task of the plan, by prefixed ID, with whether it is done.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct PlanTasks {
    done: BTreeMap<String, bool>,
}

/// Why a task reference is not acceptable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TaskProblem {
    /// Not of the form `M1-A-01`.
    Malformed,
    /// No row of the plan has this ID.
    Unknown,
    /// The task is struck through in the plan.
    Done,
}

impl fmt::Display for TaskProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Malformed => "is not a plan task ID of the form `M1-A-01`",
            Self::Unknown => "is not a task of docs/plan/m1.md",
            Self::Done => "is done (struck through in docs/plan/m1.md), so it tracks nothing",
        })
    }
}

impl PlanTasks {
    /// Parses the task tables of the plan.
    ///
    /// # Errors
    ///
    /// Returns an error if two rows define the same task ID.
    pub(crate) fn parse(text: &str) -> Result<Self> {
        let mut done = BTreeMap::new();
        let mut in_fence = false;
        for line in text.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                continue;
            }
            let Some((id, is_done)) = first_cell(trimmed).and_then(task_cell) else {
                continue;
            };
            if done.insert(id.clone(), is_done).is_some() {
                bail!("{PLAN_FILE}: task `{id}` is defined by more than one table row");
            }
        }
        Ok(Self { done })
    }

    /// The number of tasks in the plan.
    pub(crate) fn len(&self) -> usize {
        self.done.len()
    }

    /// Checks one reference, such as `M1-A-01`.
    ///
    /// # Errors
    ///
    /// Returns the problem if the reference is malformed, unknown or done.
    pub(crate) fn check(&self, reference: &str) -> Result<(), TaskProblem> {
        let bare = reference
            .strip_prefix(PREFIX)
            .ok_or(TaskProblem::Malformed)?;
        if !is_bare_id(bare) {
            return Err(TaskProblem::Malformed);
        }
        match self.done.get(reference) {
            None => Err(TaskProblem::Unknown),
            Some(true) => Err(TaskProblem::Done),
            Some(false) => Ok(()),
        }
    }
}

/// The trimmed first cell of a Markdown table row.
fn first_cell(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('|')?;
    let (cell, _) = rest.split_once('|')?;
    Some(cell.trim())
}

/// The prefixed ID in a first cell, and whether it is struck through.
fn task_cell(cell: &str) -> Option<(String, bool)> {
    let struck = cell
        .strip_prefix("~~")
        .and_then(|inner| inner.strip_suffix("~~"))
        .map(str::trim);
    let (id, done) = struck.map_or((cell, false), |inner| (inner, true));
    let bare = id.strip_prefix(PREFIX).unwrap_or(id);
    is_bare_id(bare).then(|| (format!("{PREFIX}{bare}"), done))
}

/// Whether `id` is an unprefixed task ID: an epic letter, `-`, at least two digits and an
/// optional lowercase suffix, as in `A-01` or `C-03b`.
fn is_bare_id(id: &str) -> bool {
    let mut chars = id.chars();
    let epic = chars.next().is_some_and(|c| c.is_ascii_uppercase());
    let dash = chars.next() == Some('-');
    let rest = chars.as_str();
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let suffix_ok = rest.chars().skip(digits).all(|c| c.is_ascii_lowercase());
    epic && dash && digits >= 2 && suffix_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = "\
# Plan

| # | Action |
|---|---|
| W0-1 | owner action, not a task |

### 2.3 Tasks

| ID | Task |
|---|---|
| M1-H-01 | runner ADR |
| ~~M1-H-02~~ | device plugin |

### 4.2 Epic A

| ID | Task |
|---|---|
| A-01 | ids |
| ~~A-02~~ | lifecycle |
| C-03b | split task |

```text
| A-99 | inside a code block |
```
";

    fn plan() -> PlanTasks {
        PlanTasks::parse(PLAN).unwrap()
    }

    #[test]
    fn parses_task_rows_of_every_table_and_ignores_other_rows() {
        let tasks = plan();
        assert_eq!(tasks.len(), 5, "{tasks:?}");
        assert_eq!(tasks.check("M1-A-01"), Ok(()));
        assert_eq!(tasks.check("M1-C-03b"), Ok(()));
        assert_eq!(tasks.check("M1-H-01"), Ok(()));
        assert_eq!(
            tasks.check("M1-A-99"),
            Err(TaskProblem::Unknown),
            "code block"
        );
    }

    #[test]
    fn struck_through_tasks_are_done() {
        assert_eq!(plan().check("M1-A-02"), Err(TaskProblem::Done));
        assert_eq!(plan().check("M1-H-02"), Err(TaskProblem::Done));
    }

    #[test]
    fn references_must_carry_the_milestone_prefix() {
        for reference in ["A-01", "M1-A-1", "M1-a-01", "M1-A-01B", "#12", "M1-", ""] {
            assert_eq!(
                plan().check(reference),
                Err(TaskProblem::Malformed),
                "{reference}"
            );
        }
        assert_eq!(plan().check("M1-Z-01"), Err(TaskProblem::Unknown));
    }

    #[test]
    fn a_task_defined_twice_is_an_error() {
        let text = "| A-01 | one |\n| M1-A-01 | again |\n";
        let error = PlanTasks::parse(text).unwrap_err().to_string();
        assert!(
            error.contains("`M1-A-01` is defined by more than one"),
            "{error}"
        );
    }

    #[test]
    fn parses_the_repository_plan() {
        let tasks = PlanTasks::parse(include_str!("../../docs/plan/m1.md")).unwrap();
        assert!(tasks.len() > 100, "found {} tasks", tasks.len());
        assert_ne!(
            tasks.check("M1-A-01"),
            Err(TaskProblem::Unknown),
            "epic task"
        );
        assert_ne!(
            tasks.check("M1-H-01"),
            Err(TaskProblem::Unknown),
            "lab task"
        );
    }
}
