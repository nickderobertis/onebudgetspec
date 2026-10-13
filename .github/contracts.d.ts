// Internal scheduling projections, not GitHub's full schemas. workflow-input.mjs
// validates the admitted fields; actionlint checks the source YAML against GitHub's
// grammar. Optional event fields permit conservative classification of missing data.
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type Outcome = { result?: string; outputs?: Record<string, string> };
export type Needs = Record<string, Outcome>;
export type Policy = {
  isReleaseOnly?: boolean;
  macosRequired?: boolean;
  reason?: string;
  diagnostic?: boolean;
};
export type Event = {
  event_name: string;
  provisioned?: boolean;
  action?: string;
  ref?: string;
  repository?: { full_name: string };
  pull_request?: {
    title?: string;
    user?: { login: string };
    head: { ref: string; repo?: { full_name: string } };
  };
  workflow_run?: { name: string; head_sha?: string };
  inputs?: Record<string, Json>;
  vars?: Record<string, string>;
};
export type Matrix = Record<string, Json> & {
  include?: Record<string, Json>[];
  exclude?: Record<string, Json>[];
};
export type Step = {
  if?: string | boolean;
  id?: string;
  uses?: string;
  env?: Record<string, string>;
  run?: string;
  with?: Record<string, Json>;
};
export type Job = {
  name?: string;
  outputs?: Record<string, string>;
  if?: string | boolean;
  needs?: string | string[];
  uses?: string;
  with?: Record<string, Json>;
  "runs-on"?: string | string[];
  strategy?: { matrix?: Matrix | string };
  steps?: Step[];
};
export type Workflow = {
  file: string;
  document: {
    name?: string;
    on?:
      | string
      | string[]
      | Record<
          string,
          | { types?: string[]; branches?: string[]; tags?: string[]; workflows?: string[] }
          | { cron: string }[]
          | null
        >;
    jobs: Record<string, Job>;
  };
};
export type Row = {
  job: string;
  name?: string | Json;
  skipped: boolean;
  runner: Json;
  matrix?: Record<string, Json>;
  macos?: boolean;
  targets?: string[];
};
export type Run = { workflow: string; event: string; rows: Row[]; results: Needs; label?: string };
export type Scenario = {
  label: string;
  diff: string;
  fork: boolean;
  switches: number;
  automatic: boolean;
  runs: Run[];
};
export type Lifecycle = { fork: boolean; switches: number; runs: Run[]; macosRuns: number };
export type Telemetry = {
  sourceDigest: string;
  scenarios: Scenario[];
  lifecycles: Lifecycle[];
  jobsPerRun: number;
  runsPerChange: number;
};
export type Context = {
  github?: Record<string, unknown>;
  needs?: Needs;
  matrix?: Record<string, Json>;
  inputs?: Record<string, Json>;
  vars?: Record<string, string>;
};
