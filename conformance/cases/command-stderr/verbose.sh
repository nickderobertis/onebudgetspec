#!/bin/sh
echo FIRST-LINE >&2
i=0
while [ "$i" -lt 500 ]; do
  echo "progress line $i of the noise" >&2
  i=$((i + 1))
done
echo 'build failed: out of disk space; free some space and re-run' >&2
exit 1
