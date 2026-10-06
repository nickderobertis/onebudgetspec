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

Each installs a native binary for Linux, macOS and Windows, on x86_64 and aarch64.

## The budgets file

A file named `budgets.yaml`:

```yaml
schema_version: 1
conditions:            # optional; recorded beside every result measured from this file
  - name: dispatches   # ^[a-z][a-z0-9_]*$, unique, never load1, cpus or mem_available_mib
    command: ["bash", "scripts/count-dispatches.sh"]  # run once per file per check; trimmed stdout recorded
budgets:
  - id: gate-time      # ^[a-z][a-z0-9-]*$, unique across every file one discovery finds
    description: "Wall clock of the full gate"  # optional
    labels: [ci]       # optional free tags, for selecting a subset
    measure: reported  # elapsed | reported
    command: ["bash", "scripts/budget-gate-time.sh"]  # argv, no shell, run from this file's directory
    unit: seconds      # a free label; `elapsed` requires seconds
    direction: max     # max: within when actual <= threshold; min: within when actual >= threshold
    threshold: 1800    # finite and non-negative
    timeout_seconds: 3600  # optional; past it the measurement is an error
```

Unknown keys are refused at every level, naming the key and the file.

A `command` is an argv run with no shell, from the directory holding its file; a program
named by a relative path with more than one component is found from that directory too. On
Linux and macOS an executable script may be named directly, by its `#!` line. Windows reads
no `#!` line, and a bare name there resolves only to `<name>.exe`, so a script measured
on Windows names its interpreter in the command, as in
`["bash", "scripts/budget-gate-time.sh"]`, `["node", "measure.js"]` or
`["python", "measure.py"]`; that form runs on every platform. A `timeout_seconds` ends
everything the command started: its process group on Linux and macOS, its Job Object on
Windows. A failed command is described by its exit status, which on Windows is the exit code
(in hexadecimal, such as `0xC0000005`, when it is a crash's NTSTATUS); on Linux and macOS a
command ended by a signal is described by that signal. `load1` is unknown (`null`) on
Windows, which has no load average, and `mem_available_mib` is unknown on macOS.

**`elapsed`** times the command's wall clock; a non-zero exit is an error. **`reported`**
sets `ONEBUDGETSPEC_RESULT` to the path of a fresh, empty file, and the command writes one
JSON object there and exits 0:

```json
{"value": 1395.0, "detail": "optional text", "conditions": {"runner": "large"}}
```

`detail` and `conditions` are optional. A `reported` command that exits 0 having written
nothing, only whitespace, text that is not JSON, or an object without a numeric `value` gets
verdict `error`, and that result's `id` names the budget, so a measurement need not check
that it wrote its result. Returned conditions are recorded in that result's
host conditions only; a returned name equal to a declared condition, `load1`, `cpus` or
`mem_available_mib` makes the measurement an error rather than replacing a sampled value.
A command's own stdout and stderr go to stderr, never into a report, with one exception:
when a budget's command exits non-zero, is ended by a signal or its timeout, or exits 0
with a result that cannot be read, its result's `error` states that (for an unreadable
result, why it could not be read and then the exit status), followed by `; its stderr: `
and the tail of what the command wrote there. That tail is decoded lossily, each line
trimmed, blank lines dropped, the rest joined by ` | ` and other control characters made
spaces, and it is bounded to at most the last 1000 characters, after a leading `…` when
anything was cut. A command that wrote nothing but whitespace there gets no such suffix;
a command that succeeds is reported without its stderr. A failing condition command is
still recorded as `unknown`, and the same reason appears on the line onebudgetspec prints
to stderr for it.

Every budget's command, `elapsed` and `reported` alike, runs with `ONEBUDGETSPEC_BUDGET_ID`
set to its budget's `id`; a condition's command, which belongs to no budget, runs without
it. One generic runner can therefore serve every budget of a file, choosing what to measure
by that variable, with no list of ids of its own.

Each SDK writes a `reported` result for a measurement written in its language:

| SDK | Reporter |
| --- | --- |
| Rust | `onebudgetspec_core::report(value: f64, detail: Option<&str>) -> std::io::Result<bool>` |
| Python | `onebudgetspec_sdk.report(value: float, detail: str \| None = None) -> bool` |
| TypeScript | `report(value: number, detail?: string): boolean` from `@onebudgetspec/sdk`, synchronous |

When `ONEBUDGETSPEC_RESULT` is set and non-empty, each replaces the contents of the file it
names with one JSON object, `{"value": value}` plus `"detail": detail` when a detail is
given, and returns true. When it is unset or empty, each writes nothing and returns false,
so a test that measures behaves the same outside a check. A non-finite value is refused
without writing anything: Rust returns an `Err` of kind `InvalidInput`, Python raises
`ValueError` and TypeScript throws `RangeError`. A failed write is Rust's `Err`, Python's
`OSError` or a thrown error in TypeScript. No reporter reads a budgets file or compares the
value with a threshold: `onebudgetspec check` is the only judge. A command that returns
`conditions` writes its result itself, as above.

```python
from onebudgetspec_sdk import report

report(len(recorded_requests), detail="one sync of the recorded fixture")
```

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
| Calls | `load(paths, recursive)`, then `.select(&Selection)` and `.check()` or `.list_report()`, or `.all().list_report()` to validate; `schema_bundle()`; `report(value, detail)` | `check(paths=None, ids=None, labels=None, exclude_labels=None, recursive=False, cwd=None) -> CheckReport`, `validate(paths=None, recursive=False, cwd=None) -> ListReport`, `list_budgets(...)` with `check`'s arguments `-> ListReport`, `schema() -> dict`; `report(value, detail=None) -> bool`, which runs no binary | `check({paths, ids, labels, excludeLabels, recursive, cwd})`, `validate({paths, recursive, cwd})`, `listBudgets({paths, ids, labels, excludeLabels, recursive, cwd})`, `schema()`, each a promise of the generated type; `report(value, detail)`, synchronous, which runs no binary |
| Binary | none: it measures in process | the `binary=` argument, then `ONEBUDGETSPEC_BIN`, then the `onebudgetspec` the `onebudgetspec-cli` wheel installed (`Scripts\onebudgetspec.exe` on Windows), then `onebudgetspec` on `PATH` (`onebudgetspec.exe` on Windows) | the `binary` option, then `ONEBUDGETSPEC_BIN`, then the launcher of the resolved `@onebudgetspec/cli` package |
| Tests and journeys it owes | `crates/onebudgetspec-core/tests/api.rs`, `crates/onebudgetspec-core/tests/schema.rs`, and every CLI journey and conformance case, which drive it through the binary; `crates/onebudgetspec-e2e/tests/journeys/rust_report.rs` (`report`) | `sdks/python/tests/test_conformance.py` (every conformance case through `check`, `list_budgets` and `validate`), `sdks/python/tests/test_binary.py` (resolution order and refusals), `sdks/python/tests/test_generate.py` (the model generator), `sdks/python/tests/test_report.py` (`report`), and the packaging journey `crates/onebudgetspec-packaging-e2e/tests/packaging/sdk_python.rs` | `sdks/typescript/tests/conformance.test.ts`, `sdks/typescript/tests/binary.test.ts`, `sdks/typescript/tests/errors.test.ts`, `sdks/typescript/tests/generator.test.ts` and `sdks/typescript/tests/report.test.ts` (the same concerns), and the packaging journey `crates/onebudgetspec-packaging-e2e/tests/packaging/sdk_typescript.rs` |

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
budgets.yaml                         # always checked
services/api/budgets.yaml            # checked when services/api is affected
services/api/budgets/measure.mjs     # the one runner of the file's reported budgets
services/api/budgets/fixtures/       # seeded pages only one budget reads
services/api/tests/sync.test.mjs     # an existing test that also records telemetry
services/api/project.json
```

A `budgets.yaml` sits at the root of the smallest tree that holds everything specific to
its measurements: the commands it names, the tests they run, and fixtures only its budgets
use. Its measurements may call shared code anywhere else, such as a common test harness,
the code under measurement, or a fixture other tests use too; only something that exists
solely to serve one of its budgets must not live outside that tree. Here
`services/api/budgets/fixtures/pages.json`, which only the API's budgets read, belongs under
`services/api`, while the `libs/http-client` they measure and the `tools/test-harness` every
project's tests share stay where they are.

Prefer measuring where a test already exercises the behaviour. An existing test records the
budget's figure as temporary telemetry during its normal run, written to a test output that
is never committed, and the budget's command only analyses that data and reports it. A
standalone measurement is for a behaviour no existing test exercises, or one whose
recording there would cost more than measuring it on its own. Here the API's sync test
already replays recorded traffic, so it records each upstream request it makes, and
`api-requests-per-sync` counts them; no test pages through results, so
`api-queries-per-page` measures on its own.

A budget's `command` runs its measurement or analysis directly, or through one generic
runner that reads `ONEBUDGETSPEC_BUDGET_ID`, never through a wrapper script per budget:

```yaml title="services/api/budgets.yaml"
schema_version: 1
budgets:
  - id: api-cold-start
    description: "Seconds from process start to the first healthy response"
    labels: [host]
    measure: elapsed
    command: ["node", "budgets/start-and-probe.mjs"]
    unit: seconds
    direction: max
    threshold: 2
    timeout_seconds: 30
  - id: api-requests-per-sync
    description: "Upstream requests one sync of the recorded fixture makes"
    measure: reported
    command: ["node", "budgets/measure.mjs"]
    unit: requests
    direction: max
    threshold: 40
  - id: api-queries-per-page
    description: "Database queries one page of results makes"
    measure: reported
    command: ["node", "budgets/measure.mjs"]
    unit: queries
    direction: max
    threshold: 3
```

```js title="services/api/budgets/measure.mjs"
import { readFile } from "node:fs/promises";
import { report } from "@onebudgetspec/sdk";
import { queriesPerPage } from "./queries-per-page.mjs";

const measurements = {
  // Analyses what tests/sync.test.mjs recorded; a missing recording fails the budget.
  "api-requests-per-sync": async () => {
    const requests = JSON.parse(await readFile("dist/telemetry/sync-requests.json", "utf8"));
    if (!Array.isArray(requests)) throw new Error("sync-requests.json is not an array");
    return { value: requests.length, detail: "upstream requests one sync made" };
  },
  // No existing test pages through results, so this one measures on its own.
  "api-queries-per-page": queriesPerPage,
};
const id = process.env.ONEBUDGETSPEC_BUDGET_ID;
if (!Object.hasOwn(measurements, id)) throw new Error(`no measurement for budget ${id}`);
const { value, detail } = await measurements[id]();
report(value, detail);
```

Whether a budget's check can be cached depends on how it is measured:

- **A deterministic `reported` budget is cacheable**: one whose figure depends only on code
  and fixtures, such as requests counted against recorded traffic. Its cache key covers its
  budgets file's tree (the project root), the production sources of the code it measures
  (in Nx, `^production`, or the named dependency's inputs), and the onebudgetspec version
  (the lockfile entry that pins it). One that analyses a test's telemetry depends on that
  test and takes the telemetry it wrote as an input too.
- **An `elapsed` budget is not cacheable**, and neither is any budget that reads the host:
  its load, the clock, the network or a credential. Label those budgets, here `host`, and
  check them in a target that is never cached.

So `services/api/project.json` checks its deterministic budgets in a cached `budgets`
target, after the test that records their telemetry, and the rest in an uncached one:

```json
{
  "targets": {
    "test": {
      "command": "node --test services/api/tests",
      "outputs": ["{projectRoot}/dist/telemetry"]
    },
    "budgets": {
      "command": "onebudgetspec check services/api/budgets.yaml --exclude-label host",
      "dependsOn": ["test"],
      "cache": true,
      "inputs": [
        "{projectRoot}/**/*",
        "^production",
        { "dependentTasksOutputFiles": "**/telemetry/*.json" },
        { "externalDependencies": ["@onebudgetspec/cli"] }
      ]
    },
    "budgets-host": {
      "command": "onebudgetspec check services/api/budgets.yaml --label host",
      "cache": false
    }
  }
}
```

and runs them for the affected projects only:

```sh
nx affected -t budgets budgets-host
```

The root file holds what every change must stay within:

```yaml title="budgets.yaml"
schema_version: 1
conditions:
  - name: runner
    command: ["node", "-p", "process.arch"]
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
