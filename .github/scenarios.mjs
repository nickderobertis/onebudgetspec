// The dispatch workload: event classes and lifecycle order, separate from cost limits.
export const events = [
  ["pr-opened", "pull_request", "opened"],
  ["pr-synchronize", "pull_request", "synchronize"],
  ["pr-reopened", "pull_request", "reopened"],
  ["base-push", "push"],
  ["release-merge", "push"],
  ["release-tag", "push"],
  ["release-published", "release", "published"],
  ["downstream", "workflow_run", "completed"],
  ["dispatch", "repository_dispatch"],
  ["scheduled", "schedule"],
  ["manual-default", "workflow_dispatch"],
  ["manual-diagnostic", "workflow_dispatch"],
];
export const diffs = ["source", "version", "dependency"];
export const lifecycle = [
  ["pr-synchronize", "source", "original"],
  ["base-push", "source", "original"],
  ["pr-synchronize", "version", "release"],
  ["release-merge", "version", "release"],
  ["release-tag", "version", "release"],
  ["release-published", "version", "release"],
  ["downstream", "version", "release"],
  ["dispatch", "version", "release"],
  ["scheduled", "version", "release"],
];
