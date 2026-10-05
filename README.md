# onebudgetspec

Register measurable budgets in YAML and gate on them. Each budget names a command that
performs the measurement, a unit, a direction and a threshold; `onebudgetspec check` runs
every selected command once and reports the actual value, the budget and the headroom.

It ships as a Rust SDK (`onebudgetspec-core`) and a command line (`onebudgetspec`), with
Python and TypeScript SDKs released beside them at the same version.

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
| 2 | the invocation or a file is invalid; nothing was run |
| 3 | at least one measurement errored, or the report could not be written (the reason is on stderr) |

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
