#!/bin/sh
echo 'could not reach the database; start it and re-run' >&2
printf nope > "$ONEBUDGETSPEC_RESULT"
