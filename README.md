# onebudgetspec

Register measurable budgets in YAML and gate on them. Each budget names a command that
performs the measurement, a unit, a direction and a threshold; `onebudgetspec check` runs
every selected command once and reports the actual value, the budget and the headroom.

It ships as a Rust SDK (`onebudgetspec-core`) and a command line (`onebudgetspec`), with
Python and TypeScript SDKs released beside them at the same version (see [SDKs](#sdks)).

## Principles

- **Measure once.** Each selected budget's command runs exactly once per check. There is
  no retry, repeat or sampling.
- **Report actual, budget and headroom.** Every result carries all three, so even a result
  within budget shows how close it came.
- **Record host conditions beside each result.** The load average, CPU count, available
  memory and any conditions the file declares are recorded with every measurement, so a
  person can judge an extreme circumstance from the result itself.
- **Over budget is a defect, never re-sampled.** An over-budget result fails the check on
  any host. It is never re-run until it passes.
- **No baselines.** A budget is the threshold written in the file, not a comparison with an
  earlier run.

## Install

```sh
pip install onebudgetspec-cli         # or: uvx --from onebudgetspec-cli onebudgetspec
npm install --save-dev @onebudgetspec/cli
cargo install onebudgetspec
```

## The budgets file

A file named `budgets.yaml`:

```yaml
schema_version: 1
conditions:            # optional; recorded beside every result measured from this file
  - name: dispatches   # ^[a-z][a-z0-9_]*$, unique, never load1, cpus or mem_available_mib
    command: ["scripts/count-dispatches.sh"]  # run once per file per check; trimmed stdout recorded
budgets:
  - id: gate-time      # ^[a-z][a-z0-9-]*$, unique across every file one discovery finds
    description: "Wall clock of the full gate"  # optional
    labels: [ci]       # optional free tags, for selecting a subset
    measure: reported  # elapsed | reported
    command: ["scripts/budget-gate-time.sh"]  # argv, no shell, run from this file's directory
    unit: seconds      # a free label; `elapsed` requires seconds
    direction: max     # max: within when actual <= threshold; min: within when actual >= threshold
    threshold: 1800    # finite and non-negative
    timeout_seconds: 3600  # optional; past it the measurement is an error
```

Unknown keys are refused at every level, naming the key and the file.

**`elapsed`** times the command's wall clock; a non-zero exit is an error. **`reported`**
sets `ONEBUDGETSPEC_RESULT` to the path of a fresh, empty file, and the command writes one
JSON object there and exits 0:

```json
{"value": 1395.0, "detail": "optional text", "conditions": {"runner": "large"}}
```

`detail` and `conditions` are optional. Returned conditions are recorded in that result's
host conditions only; a returned name equal to a declared condition, `load1`, `cpus` or
`mem_available_mib` makes the measurement an error rather than replacing a sampled value.
A command's own stdout and stderr go to stderr, never into a report.

## The command line

```sh
onebudgetspec check [PATH]... [--id ID]... [--label LABEL]... [--exclude-label LABEL]... [--recursive]
onebudgetspec validate [PATH]... [--recursive]
onebudgetspec list [PATH]... [--id ID]... [--label LABEL]... [--exclude-label LABEL]... [--recursive]
onebudgetspec schema
```

With no PATH, `./budgets.yaml` is read. `--recursive` searches a directory for files named
`budgets.yaml`, honouring `.gitignore`. Every verb takes `--output text|json` (`--json` for
short). `schema` prints the JSON Schema bundle for the budgets file and both reports.

A text result reads:

```text
budget gate-time: actual 1395 seconds, budget 1800 seconds, headroom 405 seconds (22.5%) — within; host: load=2.1/8 mem_available=5120MiB dispatches=3
```

| Exit | Meaning |
| ---- | ------- |
| 0 | every selected budget is within |
| 1 | at least one is over, and none errored |
| 2 | the invocation or a file is invalid; nothing was run (the reason is on stderr) |
| 3 | at least one measurement errored (its reason is that result's `error`, in the report), or the report could not be written (that reason is on stderr) |

The npm launcher adds three of its own, each with the reason on stderr: `64` on a platform
no carrier is published for, `69` when the carrier package is missing or its binary
cannot run, and `70` when the binary was ended by a signal.

## SDKs

Three SDKs, released at the binary's version. The Rust SDK is the engine itself; the Python
and TypeScript SDKs run the `onebudgetspec` binary once per call, parse the JSON it prints,
and return the reports as types generated from `onebudgetspec schema`, so they never drift
from it.

| | Rust | Python | TypeScript |
| --- | --- | --- | --- |
| Package | `onebudgetspec-core` (crates.io) | `onebudgetspec-sdk` (PyPI), imported as `onebudgetspec_sdk` | `@onebudgetspec/sdk` (npm) |
| Install | `cargo add onebudgetspec-core` | `pip install onebudgetspec-sdk`, which installs `onebudgetspec-cli` at the same version | `npm install @onebudgetspec/sdk`, which installs its optional dependency `@onebudgetspec/cli` at the same version |
| Calls | `load(paths, recursive)`, then `.select(&Selection)` and `.check()` or `.list_report()`, or `.all().list_report()` to validate; `schema_bundle()` | `check(paths=None, ids=None, labels=None, exclude_labels=None, recursive=False, cwd=None) -> CheckReport`, `validate(paths=None, recursive=False, cwd=None) -> ListReport`, `list_budgets(...)` with `check`'s arguments `-> ListReport`, `schema() -> dict` | `check({paths, ids, labels, excludeLabels, recursive, cwd})`, `validate({paths, recursive, cwd})`, `listBudgets({paths, ids, labels, excludeLabels, recursive, cwd})`, `schema()`, each a promise of the generated type |
| Binary | none: it measures in process | the `binary=` argument, then `ONEBUDGETSPEC_BIN`, then the `onebudgetspec` the `onebudgetspec-cli` wheel installed, then `onebudgetspec` on `PATH` | the `binary` option, then `ONEBUDGETSPEC_BIN`, then the launcher of the resolved `@onebudgetspec/cli` package |
| Tests and journeys it owes | `crates/onebudgetspec-core/tests/api.rs`, `crates/onebudgetspec-core/tests/schema.rs`, and every CLI journey and conformance case, which drive it through the binary | `sdks/python/tests/test_conformance.py` (every conformance case through `check`, `list_budgets` and `validate`), `sdks/python/tests/test_binary.py` (resolution order and refusals), and the packaging journey `crates/onebudgetspec-packaging-e2e/tests/packaging/sdk_python.rs` | `sdks/typescript/tests/conformance.test.ts`, `sdks/typescript/tests/binary.test.ts` and `sdks/typescript/tests/errors.test.ts` (the same three concerns), and the packaging journey `crates/onebudgetspec-packaging-e2e/tests/packaging/sdk_typescript.rs` |

In Python and TypeScript, exit statuses `0`, `1` and `3` return the report, since the
verdicts are in it; status `2` (an invalid invocation or budgets file) raises
`OnebudgetspecError` (Python) or rejects with it (TypeScript), carrying the CLI's own
message and `exit_code`/`exitCode` `2`.

```python
from onebudgetspec_sdk import check

report = check(labels=["api"], exclude_labels=["slow"], cwd="services")
for result in report.results:
    print(result.id, result.verdict, result.actual, result.headroom, result.host.conditions)
```

```ts
import { check } from "@onebudgetspec/sdk";

const report = await check({ labels: ["api"], excludeLabels: ["slow"], cwd: "services" });
for (const result of report.results) {
  console.log(result.id, result.verdict, result.actual, result.headroom, result.host.conditions);
}
```

## Nesting budgets files

Each project registers its own budgets beside its code, and only the budgets a change can
affect run; the budgets that must always be checked go in the root file. With Nx as the
worked example, a repository lays them out as:

```text
budgets.yaml               # always checked
services/api/budgets.yaml  # checked when services/api is affected
services/api/project.json
```

The project's own file:

```yaml title="services/api/budgets.yaml"
schema_version: 1
budgets:
  - id: api-cold-start
    description: "Seconds from process start to the first healthy response"
    measure: elapsed
    command: ["scripts/start-and-probe.sh"]
    unit: seconds
    direction: max
    threshold: 2
    timeout_seconds: 30
  - id: api-requests-per-second
    measure: reported
    command: ["scripts/load-test.sh"]
    unit: requests/s
    direction: min
    threshold: 500
```

checked by that project's `budgets` target in `services/api/project.json`:

```json
{
  "targets": {
    "budgets": {
      "command": "onebudgetspec check services/api/budgets.yaml",
      "inputs": ["{projectRoot}/**/*"]
    }
  }
}
```

and run for the affected projects only:

```sh
nx affected -t budgets
```

The root file holds what every change must stay within:

```yaml title="budgets.yaml"
schema_version: 1
conditions:
  - name: runner
    command: ["uname", "-m"]
budgets:
  - id: gate-time
    description: "Wall clock of the full gate"
    measure: elapsed
    command: ["just", "check"]
    unit: seconds
    direction: max
    threshold: 1800
    timeout_seconds: 3600
```

checked on every change with `onebudgetspec check budgets.yaml`. To check every file at
once, `onebudgetspec check --recursive .` discovers both.

## Licence

MIT
