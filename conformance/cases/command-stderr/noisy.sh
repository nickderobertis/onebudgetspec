#!/bin/sh
echo 'warning: cache cold' >&2
printf '{"value": 3}' > "$ONEBUDGETSPEC_RESULT"
