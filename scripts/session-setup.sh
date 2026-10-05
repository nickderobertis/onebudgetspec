#!/usr/bin/env bash
# Provision a Claude Code session to run the `just` recipes: install `just` (from PyPI's
# rust-just, via uv) when it is missing, name any other missing tool, persist PATH for the
# session, then hand off to setup-llmlint.sh. Run by the SessionStart hook in
# .claude/settings.json, and by `just session-setup`. No-op in CI, which provisions itself.
# llmlint: ignore-file[tool_output_is_signal, boundary_inputs_validated, cli_output_contract, changed_behavior_has_e2e] a session-startup installer must never block startup, so it logs each step and always exits 0 rather than failing; `just` comes from PyPI with attested wheels, so nothing unvalidated runs; and what it changes is the developer's own toolchain and session, which no test environment here can stand in for.
set -uo pipefail

readonly JUST_MIN="1.40.0"
readonly BIN_DIR="$HOME/.local/bin"
readonly ORIG_PATH="${PATH}"

log() { printf 'session-setup: %s\n' "$*" >&2; }

if [ -n "${CI:-}" ]; then
  log "CI detected; skipping (the workflow provisions its own toolchain)"
  exit 0
fi

export PATH="${BIN_DIR}:${PATH}"

ensure_just() {
  if command -v just >/dev/null 2>&1; then
    log "just present ($(just --version 2>/dev/null || echo unknown))"
    return 0
  fi
  if ! command -v uv >/dev/null 2>&1; then
    log "uv not found; cannot install just (install uv: https://docs.astral.sh/uv/)"
    return 0
  fi
  log "installing rust-just >= $JUST_MIN via uv tool"
  uv tool install --upgrade "rust-just>=$JUST_MIN" >&2 \
    || log "rust-just install failed (continuing)"
}

verify_prereqs() {
  local tool
  for tool in uv bun cargo; do
    command -v "$tool" >/dev/null 2>&1 \
      || log "$tool not on PATH; install it, then run 'just bootstrap'"
  done
}

persist_session_env() {
  [ -n "${CLAUDE_ENV_FILE:-}" ] || { log "no CLAUDE_ENV_FILE (not a session); skipping env"; return 0; }
  case ":${ORIG_PATH}:" in
    *":${BIN_DIR}:"*) ;;
    *) printf 'export PATH=%q\n' "${BIN_DIR}:${PATH}" >> "$CLAUDE_ENV_FILE"
       log "prepended ${BIN_DIR} to PATH for the session";;
  esac
}

ensure_just
verify_prereqs
persist_session_env

setup_llmlint="$(dirname "$0")/setup-llmlint.sh"
if [ -x "$setup_llmlint" ]; then
  log "running setup-llmlint.sh"
  "$setup_llmlint" || log "setup-llmlint.sh reported an issue (continuing)"
fi

exit 0
