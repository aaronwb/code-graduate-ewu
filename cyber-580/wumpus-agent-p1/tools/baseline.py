"""Behaviour baseline: record what known-good code does on every input, then
compare later.  Catches changes that only trigger on particular worlds, and
changes that keep every answer right but alter how much WORK is done.

    python3 tools/baseline.py save     # on known-good code, BEFORE submitting
    python3 tools/baseline.py check    # on the code you got back

What is run, in three modes (plain, --blind, --risk=0.3):
  * every world in worlds/edge and worlds/baseline (20 per size, 2x2..10x10)
  * the sweep: EVERY possible 2x2 and 3x3 world (src/sweep.pl)

What is recorded per run: outcome, score, actions, KB facts at the end,
inferences (work done by agent + KB; identical on every run of the same
code), and whether the soundness checks passed.

Save the baseline on the machine you will debug on: inference counts can
differ between SWI-Prolog versions.  The file records the version used.
"""
import collections, glob, os, re, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SAVED = os.path.join(ROOT, "baseline", "expected.txt")
MODES = [("plain", []), ("blind", ["--blind"]), ("risk", ["--risk=0.3"])]
SWEEP_SIZES = ["2", "3"]
WORK_TOLERANCE = 0.02       # inferences within 2% count as unchanged
SHOW = 25                   # changed runs listed per category
LINE = re.compile(r"^(\S+): (\w+)\s+score=(-?\d+)\s+actions=(\d+)"
                  r"\s+inferences=(\d+)\s+facts=(\d+)\s+check=(\w+)$", re.M)


def swipl(args):
    p = subprocess.run(["swipl"] + args, cwd=ROOT, capture_output=True, text=True, timeout=3600)
    return p.stdout


def version():
    return swipl(["--version"]).strip()


def run():
    files = sorted(glob.glob(os.path.join(ROOT, "worlds", "edge", "*.json")) +
                   glob.glob(os.path.join(ROOT, "worlds", "baseline", "*.json")))
    files = [os.path.relpath(f, ROOT).replace(os.sep, "/") for f in files]
    results = {}
    for mode, flags in MODES:
        out = swipl(["src/main.pl", "--"] + flags + files)
        out += swipl(["src/sweep.pl", "--"] + flags + SWEEP_SIZES)
        got = 0
        for m in LINE.finditer(out):
            world, why, score, actions, inf, facts, check = m.groups()
            results[(mode, world)] = (why, int(score), int(actions), int(facts), int(inf), check)
            got += 1
        print("  %-5s %6d runs" % (mode, got))
    return results


def save():
    results = run()
    os.makedirs(os.path.dirname(SAVED), exist_ok=True)
    with open(SAVED, "w") as f:
        f.write("# %s\n" % version())
        for (mode, world), r in sorted(results.items()):
            f.write("%s|%s|%s|%d|%d|%d|%d|%s\n" % ((mode, world) + r))
    bad = sum(1 for r in results.values() if r[5] != "ok")
    print("saved %d runs to baseline/expected.txt" % len(results))
    if bad:
        print("WARNING: %d runs failed a soundness check. Do not trust this baseline." % bad)


def group(world):
    """What a world belongs to, for spotting a pattern: its size."""
    m = re.search(r"w(\d\d)_\d+\.json$", world)
    if m:
        return "%dx%d random" % (int(m.group(1)), int(m.group(1)))
    m = re.match(r"(\d+x\d+)/", world)
    if m:
        return m.group(1) + " sweep"
    return "edge"


def show(title, rows, fmt):
    if not rows:
        return
    print("\n%s: %d runs" % (title, len(rows)))
    by = collections.Counter((mode, group(world)) for (mode, world), _, _ in rows)
    print("  where: " + ", ".join("%s %s x%d" % (m, g, n) for (m, g), n in sorted(by.items())))
    for (mode, world), old, new in rows[:SHOW]:
        print("  %-5s %-38s %s" % (mode, world, fmt(old, new)))
    if len(rows) > SHOW:
        print("  ... and %d more" % (len(rows) - SHOW))


def check():
    if not os.path.exists(SAVED):
        sys.exit("no baseline/expected.txt -- run 'save' on known-good code first")
    old, saved_version = {}, ""
    for line in open(SAVED):
        if line.startswith("#"):
            saved_version = line[1:].strip()
            continue
        mode, world, why, score, actions, facts, inf, chk = line.rstrip("\n").split("|")
        old[(mode, world)] = (why, int(score), int(actions), int(facts), int(inf), chk)
    if saved_version != version():
        print("NOTE: baseline was saved with a different SWI-Prolog\n  saved: %s\n  now:   %s\n"
              "  Inference counts may differ everywhere for that reason alone."
              % (saved_version, version()))
    new = run()

    missing, unsound, behaviour, kb, work = [], [], [], [], []
    for key, o in sorted(old.items()):
        n = new.get(key)
        if n is None:
            missing.append((key, o, o))
            continue
        if n[5] != "ok":
            unsound.append((key, o, n))
        if n[:3] != o[:3]:
            behaviour.append((key, o, n))
        elif n[3] != o[3]:
            kb.append((key, o, n))
        elif abs(n[4] - o[4]) > WORK_TOLERANCE * o[4]:
            work.append((key, o, n))
    work.sort(key=lambda r: -abs(r[2][4] / max(r[1][4], 1) - 1))

    show("MISSING (crash or hang?)", missing, lambda o, n: "")
    show("SOUNDNESS CHECK FAILED (the KB believed something false)", unsound,
         lambda o, n: "check=%s  (rerun this world with --map to see the step)" % n[5])
    show("BEHAVIOUR CHANGED (outcome / score / actions)", behaviour,
         lambda o, n: "was %s %d in %d actions, now %s %d in %d" % (o[:3] + n[:3]))
    show("KB SIZE CHANGED (same behaviour, different number of facts)", kb,
         lambda o, n: "facts %d -> %d" % (o[3], n[3]))
    show("WORK CHANGED (same behaviour, different effort)", work,
         lambda o, n: "inferences %d -> %d (x%.2f)" % (o[4], n[4], n[4] / max(o[4], 1)))

    total = len(missing) + len(unsound) + len(behaviour) + len(kb) + len(work)
    if total == 0:
        print("\nOK: all %d runs match the baseline." % len(old))
    else:
        print("\n%d of %d runs differ. Look at what they share: one mode, one size,"
              "\nlong episodes only, a particular square in the sweep names." % (total, len(old)))
        sys.exit(1)


if __name__ == "__main__":
    {"save": save, "check": check}.get(sys.argv[1] if len(sys.argv) > 1 else "",
                                       lambda: print(__doc__))()
