# Conformance suite

Cases any implementation of the `onebudgetspec` command line must pass, run against the
binary by `crates/onebudgetspec/tests/conformance.rs` and runnable unchanged by any other
runner. The cases are data and POSIX shell scripts only.

## A case

Each directory under `cases/` is one case:

- `case.json` — what to run and what to expect:
  - `description`: what the case proves.
  - `args`: the arguments to pass to the command under test.
  - `exit`: the exit status expected.
  - `timed` (optional): ids whose `actual`, `headroom` and `headroom_percent` are wall-clock
    timings, normalized as below.
  - `error_contains` (optional): for an id, a substring its result's `error` must contain.
- `budgets.yaml` and any scripts its commands run.
- `expected.json` — present when the case expects a check report on stdout: the report,
  normalized. When absent, stdout must be empty.

## Running a case

1. Copy the case directory to a fresh temporary directory.
2. Run the command under test with `args`, from that directory, with the environment
   inherited.
3. Assert the exit status equals `exit`.
4. With `expected.json`: parse stdout as one JSON document, validate it against the
   `check-report` root of the command's own `schema` output, normalize it, and assert it
   equals `expected.json`. Without: assert stdout is empty.

## Normalization

Applied to every result of the report, so a run on any host at any time compares equal:

- `started_at` and `ended_at` become `"1970-01-01T00:00:00Z"`.
- `host.load1` becomes `0.0`, `host.cpus` becomes `1` and `host.mem_available_mib`
  becomes `0`.
- `error`, when not null, becomes `"<error>"` (after checking `error_contains`).
- For an id in `timed`: `actual`, `headroom` and `headroom_percent`, when not null,
  become `0.0`.

Every `expected.json` is itself a valid check report, which the Rust runner also asserts.
