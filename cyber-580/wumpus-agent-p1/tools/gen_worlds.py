"""Generate random worlds with the instructor's environments.py.

    python3 tools/gen_worlds.py OUT_DIR [COUNT_PER_SIZE] [PIT_PROBABILITY] [SEED]

Makes COUNT_PER_SIZE worlds for every cave size 2x2 .. 10x10.
world_dims includes the wall ring, so an NxN cave is world_dims (N+2, N+2).
"""
import os, random, sys
from environments import WumpusEnvironment

out = sys.argv[1] if len(sys.argv) > 1 else "worlds"
count = int(sys.argv[2]) if len(sys.argv) > 2 else 5
pit_p = float(sys.argv[3]) if len(sys.argv) > 3 else 0.2
seed = int(sys.argv[4]) if len(sys.argv) > 4 else 0

os.makedirs(out, exist_ok=True)
random.seed(seed)
for n in range(2, 11):
    for i in range(count):
        w = WumpusEnvironment(None, n + 2, n + 2, pit_p)
        w.export_json(os.path.join(out, "w%02d_%03d.json" % (n, i)))
print("wrote", 9 * count, "worlds to", out)
