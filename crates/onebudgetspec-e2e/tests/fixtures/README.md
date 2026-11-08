Real saved comparison inputs, read by `tests/journeys/report_layout.rs`.

- `onepipeline-ui-676.json` is the `comparison-input.json` of the `budget-results` artifact
  of nickderobertis/onepipeline-ui run 42384341444 (pull request #676): 7 rows and 5
  baseline runs, nothing over or flagged.
- `onepipeline-ui-686.json` is the same file from run 42421538096 (#686): 9 rows, 10
  baseline runs, two report-only `over` rows.
- `onepipeline-ui-686-flagged-error.json` is a copy of the second with two budgets added:
  `onepipeline-ui-budget-render-seconds`, a twin of the budget-report command's budget in the
  current run and every baseline run, whose current value is set 2.5 SD above its baseline
  mean so that it is flagged, and `onepipeline-ui-docs-build-seconds`, an enforced budget
  whose current result is an `error` and which no baseline run catalogs.

Each was downloaded with `gh run download <run> -R nickderobertis/onepipeline-ui -n
budget-results`. One change was made to the downloaded files: a result `detail` longer than
4096 bytes is replaced by `null`. The two affected results are the macOS budgets in #676, at
about 172 KB each. A comparison never reads `detail`.

`v0.2.0-outcomes.json` holds what the v0.2.0 `onebudgetspec report --json` gave for each
fixture: its exit code and, per row, `verdict`, `flag`, `baseline_n`, `mean`, `sd`,
`percent_change` and `sigma_change`.
