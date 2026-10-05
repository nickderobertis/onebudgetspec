#!/bin/sh
printf %s "$1" > "$ONEBUDGETSPEC_RESULT" || {
  echo "report.sh: cannot write the result to '$ONEBUDGETSPEC_RESULT'; run this only as a budget's command under 'onebudgetspec check', which creates a writable result file" >&2
  exit 1
}
