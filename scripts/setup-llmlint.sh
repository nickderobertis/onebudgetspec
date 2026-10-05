#!/usr/bin/env bash
# Install or refresh the judged-lint toolchain: `llmlint-cli` from PyPI via `uv tool`,
# which brings `oneharness` with it. oneharness.toml chooses the harness. Run by
# session-setup.sh and `just setup-llmlint`; idempotent.
# llmlint: ignore-file[tool_output_is_signal, boundary_inputs_validated, cli_output_contract, changed_behavior_has_e2e] a session-startup installer must never block startup, so it logs each step and always exits 0 rather than failing; the toolchain comes from PyPI with attested wheels, so nothing unvalidated runs; and what it changes is the developer's own toolchain and session, which no test environment here can stand in for.
set -uo pipefail

# The release whose `validate` gate and changed-file `--diff` the lint recipes use.
readonly LLMLINT_MIN="0.3.23"
readonly BIN_DIR="$HOME/.local/bin"

log() { printf 'setup-llmlint: %s\n' "$*" >&2; }

ensure_toolchain() {
  if ! command -v uv >/dev/null 2>&1; then
    log "uv not found; cannot install llmlint (install uv: https://docs.astral.sh/uv/)"
    return 0
  fi
  log "installing llmlint-cli >= $LLMLINT_MIN via uv tool"
  uv tool install --upgrade "llmlint-cli>=$LLMLINT_MIN" >&2 \
    || log "llmlint-cli install failed (continuing)"
}

persist_session_env() {
  [ -n "${CLAUDE_ENV_FILE:-}" ] || { log "no CLAUDE_ENV_FILE (not a session); skipping env"; return 0; }
  case ":${PATH}:" in
    *":${BIN_DIR}:"*) ;;
    *) printf 'export PATH=%q\n' "${BIN_DIR}:${PATH}" >> "$CLAUDE_ENV_FILE" ;;
  esac
  log "exported PATH"
}

export PATH="${BIN_DIR}:${PATH}"
ensure_toolchain
persist_session_env
if command -v llmlint >/dev/null 2>&1; then
  log "ready (llmlint: $(llmlint --version 2>/dev/null || echo unknown))"
  llmlint doctor >&2 2>&1 || log "llmlint doctor reported an issue (see above)"
else
  log "llmlint not installed"
fi
exit 0
