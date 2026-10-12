# saphyr-diff: a throwaway differential harness

Measures how serde-saphyr 1.3.0 (and saphyr 0.2.1's node tree) differ from serde_norway
0.9.42, today's YAML crate, on this repository's real YAML. It is spike work and never
lands. It is its own cargo workspace, so the product's lockfile, lints and `deny.toml`
never see it.

    spikes/saphyr-diff/run.sh [external budgets.yaml ...]   # every read/write/error difference
    spikes/saphyr-diff/deny.sh                              # deny.toml with serde_norway replaced
    spikes/saphyr-diff/confined.sh <script> [args ...]      # where that script writes

`run.sh` builds the CLI and runs the e2e journeys with `ONEBUDGETSPEC_SPIKE_CORPUS` set. A
spike-only hook in `crates/onebudgetspec-e2e/tests/journeys/common.rs` then keeps every
budgets document the journeys write, and every JSON value `Fixture::budgets` renders. The
harness reads those documents, the committed conformance cases, the README's YAML blocks
and `core/tests/api.rs`'s literals. It prints:

- **READS:** each document read into `BudgetsFile` through `serde_path_to_error`, into
  `serde_json::Value` (with `preserve_order`), and into saphyr's tree.
- **WRITES:** each value rendered by both crates, byte for byte, across a grid of
  `SerializerOptions`.
- **ERRORS:** `invalid_files.rs`'s defects and `api.rs`'s refusals.
- **PROBES:** synthetic edge cases.
- **ADAPTERS:** the small code changes in `src/adapters.rs`, alone and together, over
  every read, error case and probe, plus the schema-less read and write adapters.
- **PARSE TIME:** for the largest documents.

Every file either script writes lands in the clone or under
`$ONEPIPELINE_NODE_SCRATCH_DIR/saphyr-diff/` when that is set, else the clone's gitignored
`target/saphyr-diff/`: `TMPDIR` and `CARGO_HOME` point there, and `RUSTC_WRAPPER` is
cleared so no shared compile cache is written. `confined.sh` checks it, running a script
with everything else read-only and listing each write that was refused.
