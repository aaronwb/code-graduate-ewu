"""Build a world JSON from an ASCII drawing (handy for edge cases and Part 2).

    python3 tools/make_world.py drawing.txt out.json

Draw only the cave interior; the wall ring is added for you.  The bottom-left
character is the start square (1,1).  Top line of the file = highest Y.
    .  empty     P  pit     W  wumpus     G  gold     #  wall
    X  wumpus + gold        Q  pit + gold
"""
import json, sys

CODES = {".": [], "P": ["Pit"], "W": ["Wumpus"], "G": ["Gold"], "#": ["Wall"],
         "X": ["Wumpus", "Gold"], "Q": ["Pit", "Gold"]}

def build(lines):
    rows = [l.strip() for l in lines if l.strip()]
    h, w = len(rows), len(rows[0])
    world = [[["Wall"] if x in (0, w + 1) or y in (0, h + 1) else []
              for y in range(h + 2)] for x in range(w + 2)]
    for r, row in enumerate(rows):
        for c, ch in enumerate(row):
            world[c + 1][h - r] = list(CODES[ch.upper()])
    world[1][1].append("Explorer")
    return {"world_dims": [w + 2, h + 2], "world": world}

if __name__ == "__main__":
    json.dump(build(open(sys.argv[1])), open(sys.argv[2], "w"), indent=4)
