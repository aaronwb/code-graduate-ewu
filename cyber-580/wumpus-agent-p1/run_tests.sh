#!/bin/sh
# Full check: unit tests + edge cases + N random worlds per size.
# Usage: ./run_tests.sh [N]
# (For the saved-baseline comparison see tools/baseline.py.)
cd "$(dirname "$0")" || exit 1
N=${1:-50}
echo "== unit tests =="
swipl -g run_tests -t halt tests/tests.pl 2>&1 | tail -3
echo "== edge cases =="
swipl src/main.pl -- worlds/edge/*.json
echo "== $N random worlds per size (2x2..10x10) =="
rm -rf /tmp/wumpus_rand
python3 tools/gen_worlds.py /tmp/wumpus_rand "$N" 0.2 42 >/dev/null
swipl src/main.pl -- /tmp/wumpus_rand/*.json > /tmp/wumpus_rand/results.txt
for k in won left died timeout; do
    echo "$k: $(grep -c ": $k " /tmp/wumpus_rand/results.txt)"
done
echo "failed soundness checks: $(grep -vc "check=ok" /tmp/wumpus_rand/results.txt)"
echo "(died, timeout and failed checks must all be 0)"
