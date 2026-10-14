# Workflow tooling

`github-workflows:test` resolves the actual workflows and records ignored telemetry;
its `budgets` target consumes that recording without running scenarios again. Keep
numeric cost limits in budgets.yaml. Unsupported routing must fail explicitly.

## Routing

`.github/macos-policy.mjs classify` verifies same-repository `release-plz-` branches
by parsed version/changelog content; authors never authorize omission. Unknown CI
names and unreadable PRs require Mac coverage. The one PR Mac lane and one Apple
Silicon Darwin release job (no Intel macOS target) are the automatic locations; manual
CI diagnostics default off.
