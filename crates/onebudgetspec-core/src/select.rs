//! Choosing which budgets one invocation checks or lists.

use crate::error::Error;
use crate::load::{Budgets, LoadedFile};
use crate::model::{Budget, SCHEMA_VERSION};
use crate::report::{ListReport, ListedBudget};

/// The filters one invocation applies, all together.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// Keep only these ids. Empty keeps every id.
    pub ids: Vec<String>,
    /// Keep only budgets carrying at least one of these labels. Empty keeps every budget.
    pub labels: Vec<String>,
    /// Drop budgets carrying any of these labels, even one `labels` would keep.
    pub exclude_labels: Vec<String>,
}

/// One selected budget and the file it came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Selected<'a> {
    /// The file that registers the budget.
    pub file: &'a LoadedFile,
    /// The budget.
    pub budget: &'a Budget,
}

/// The budgets a [`Selection`] keeps, in file order.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectedBudgets<'a> {
    pub(crate) entries: Vec<Selected<'a>>,
}

impl Budgets {
    /// Apply `selection` to every budget these files register.
    ///
    /// A selection that keeps nothing is not an error: it selects no budget.
    ///
    /// # Errors
    ///
    /// [`Error::Invocation`] when an id in `selection.ids` is registered by none of the
    /// files.
    pub fn select(&self, selection: &Selection) -> Result<SelectedBudgets<'_>, Error> {
        let unknown: Vec<&str> = selection
            .ids
            .iter()
            .filter(|id| {
                !self
                    .files()
                    .iter()
                    .any(|file| file.contents.budgets.iter().any(|budget| &budget.id == *id))
            })
            .map(String::as_str)
            .collect();
        if !unknown.is_empty() {
            return Err(Error::invocation(
                format!(
                    "no budget has the id{} {}",
                    if unknown.len() == 1 { "" } else { "s" },
                    unknown.join(", ")
                ),
                "run `onebudgetspec list` to see the registered ids, then re-run with one of them",
            ));
        }

        Ok(self.filtered(selection))
    }

    /// Every budget these files register, unfiltered.
    #[must_use]
    pub fn all(&self) -> SelectedBudgets<'_> {
        self.filtered(&Selection::default())
    }

    fn filtered(&self, selection: &Selection) -> SelectedBudgets<'_> {
        let carries = |budget: &Budget, labels: &[String]| {
            budget.labels.iter().any(|label| labels.contains(label))
        };
        let entries = self
            .files()
            .iter()
            .flat_map(|file| {
                file.contents
                    .budgets
                    .iter()
                    .map(move |budget| Selected { file, budget })
            })
            .filter(|selected| {
                let budget = selected.budget;
                (selection.ids.is_empty() || selection.ids.contains(&budget.id))
                    && (selection.labels.is_empty() || carries(budget, &selection.labels))
                    && !carries(budget, &selection.exclude_labels)
            })
            .collect();
        SelectedBudgets { entries }
    }
}

impl<'a> SelectedBudgets<'a> {
    /// The selected budgets, in file order.
    #[must_use]
    pub fn entries(&self) -> &[Selected<'a>] {
        &self.entries
    }

    /// Whether nothing was selected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The selected budgets as `list` reports them. Runs no command.
    #[must_use]
    pub fn list_report(&self) -> ListReport {
        ListReport {
            schema_version: SCHEMA_VERSION,
            budgets: self
                .entries
                .iter()
                .map(|Selected { file, budget }| ListedBudget {
                    id: budget.id.clone(),
                    file: file.display.clone(),
                    description: budget.description.clone(),
                    labels: budget.labels.clone(),
                    measure: budget.measure,
                    command: budget.command.clone(),
                    unit: budget.unit.clone(),
                    direction: budget.direction,
                    threshold: budget.threshold,
                    timeout_seconds: budget.timeout_seconds,
                })
                .collect(),
        }
    }
}
