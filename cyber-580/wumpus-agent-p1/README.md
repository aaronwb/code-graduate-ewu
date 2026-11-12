# Wumpus World Logical Agent (SWI-Prolog)

This is my logical agent for the Wumpus World, written in SWI-Prolog. It's split
into a knowledge base that does all the remembering and reasoning, and an agent
that only ever talks to the knowledge base through `tell` and `ask`. The agent
itself holds no state between steps.

## How I run it

I run the agent with a risk limit of 0.25:

    swipl src/main.pl -- --risk=0.25 worlds/random/*.json

The code's default is still to never gamble, so the `--risk=0.25` flag has to be
passed explicitly. I kept the default that way on purpose: with the flag left off,
the agent only ever moves onto squares it has proven safe and never dies, which
makes it easier to test and debug. Turning the flag on is what lets it take a
calculated chance when it would otherwise give up. The trade-off is explained
under "Taking risks" below.

## Setup

1. Install SWI-Prolog 9.x from <https://www.swi-prolog.org/download/stable>
   (or `brew install swi-prolog` on macOS, `sudo apt install swi-prolog-nox` on Ubuntu).
2. Python 3 is only needed to generate worlds. `tools/environments.py` is the
   instructor's generator, unchanged.

Run every command from the project folder, the one that contains this README.

## Commands

    swipl src/main.pl -- --risk=0.25 worlds/edge/open_10.json      # one world, the way I run it
    swipl src/main.pl -- worlds/edge/open_10.json                  # same world, no gambling
    swipl src/main.pl -- --trace worlds/edge/2x2_gold_diag.json    # print every percept and action
    swipl src/main.pl -- --map worlds/edge/gold_under_wumpus.json  # also draw the agent's map each step
    swipl src/main.pl -- --blind worlds/random/*.json              # don't tell the agent the cave size

The flags can be combined, for example `--risk=0.25 --map`.

Testing:

    swipl -g run_tests -t halt tests/tests.pl    # 109 unit tests, under a second
    ./run_tests.sh 50                            # unit tests, edge cases, and 450 fresh random worlds
    swipl src/sweep.pl -- 2 3                    # every possible 2x2 and 3x3 world
    python3 tools/baseline.py check              # compare against the saved baseline

Making worlds:

    python3 tools/gen_worlds.py worlds/random 5 0.2 42    # 5 worlds per size, 2x2 to 10x10
    python3 tools/make_world.py drawing.txt out.json      # build a world from an ASCII drawing

The generator's `world_dims` includes the ring of walls, so a 2x2 cave is
`(4,4)` and a 10x10 cave is `(12,12)`.

### Reading the output

Each world prints one line:

    worlds/edge/open_10.json: won  score=958  actions=42  inferences=179738  facts=41  check=ok

- **Outcome:** `won` means it climbed out with the gold, `left` means it climbed
  out without it, and the others are `died` and `timeout`.
- **Score:** standard scoring: -1 per action, -10 for the arrow, +1000 for
  leaving with the gold, -1000 for dying.
- **inferences:** how much work Prolog did inside the agent and knowledge base.
  Unlike timing, this number is the same every time, on any machine.
- **facts:** how many facts were in the knowledge base at the end.
- **check:** `ok` if the knowledge base never believed anything false,
  otherwise the number of times it did. Each one is also printed as it happens.

## Project layout

    wumpus/
    ├── README.md
    ├── run_tests.sh          unit tests + edge cases + fresh random worlds
    ├── src/                  the solver
    │   ├── kb.pl             knowledge base: all facts and inference, tell/1 and ask/1
    │   ├── agent.pl          the agent: agent_step(Percept, Action), no state of its own
    │   ├── world.pl          simulator: loads a world, gives percepts, applies actions, scores
    │   ├── check.pl          soundness checks: compares beliefs with the truth every step
    │   ├── episode.pl        plays one episode, counts work, draws the trace and map
    │   ├── main.pl           command line for world files
    │   └── sweep.pl          command line for every possible 2x2 and 3x3 world
    ├── tests/
    │   └── tests.pl          unit tests, each named after the rule it checks
    ├── tools/
    │   ├── environments.py   instructor's world generator
    │   ├── gen_worlds.py     random worlds for every size
    │   ├── make_world.py     world from an ASCII drawing
    │   └── baseline.py       saves and checks the behaviour baseline
    ├── baseline/
    │   └── expected.txt      what the known-good code does on 49,956 runs
    └── worlds/
        ├── edge/             hand-made edge cases (a .txt drawing and its .json)
        ├── baseline/         180 fixed random worlds, 20 per size
        └── random/           a few sample worlds

The simulator in `world.pl` is my own, since the provided zip only generates
worlds. It follows the rules from the course slides.

## How it works

### Agent and knowledge base

Every turn, the agent does the same three things:

    agent_step(Percept, Action) :-
        tell(percept(Percept)),   % the knowledge base records it and draws conclusions
        choose(Action),           % the agent asks questions and picks an action
        tell(action(Action)).     % the knowledge base records what was done

`agent.pl` has no dynamic predicates and no global variables, and a unit test
makes sure it stays that way. Position, heading, the arrow and the gold all live
in the knowledge base. The agent doesn't even keep a plan: it works out a fresh
route every turn and only takes its first step.

### What the knowledge base remembers

- Where the agent has been (`visited`), and where it felt a breeze or smelled a
  stench. Only the positive percepts are stored; "no breeze here" is just a
  visited square without a breeze fact, so it costs nothing.
- Squares it has proven safe but not visited yet (`safe`). This is effectively
  the to-do list, and a square drops off it once it's visited.
- Walls it bumped into, one note about a missed arrow, and a few flags (has the
  gold, arrow used, wumpus dead, and so on).

Every fact is about a square or is a single flag, so the knowledge base can
never grow beyond the size of the cave, however long the game runs. Nothing is
stored per time step.

### How it reasons

- **No pit** in a square if any visited neighbour had no breeze.
- **No wumpus** in a square if any of these hold:
  - it heard the scream, so the wumpus is dead;
  - a visited neighbour had no stench;
  - a missed arrow flew through that square;
  - there's only one wumpus and it has to be next to every stench square, so a
    square that isn't next to all of them can't hold it. This is what lets the
    agent pin the wumpus down from two stench squares.
- **Safe** means no pit and no wumpus. Squares on the edge of explored ground
  are rechecked after every percept.

### How it picks an action

It goes down this list and does the first thing that applies:

1. If it sees glitter, grab the gold.
2. If it has the gold, take the shortest safe route home and climb out.
3. If there's a safe square it hasn't visited, walk to the nearest one.
4. If the arrow is unused and some square is free of pits but might hold the
   wumpus, line up with it and shoot. Hit or miss, that square becomes safe.
5. If a risk limit was given, step onto the least risky unknown square, but
   only if its chance of death is at or under the limit.
6. Otherwise, go home and climb out without the gold.

This follows the order from the "Logical Inference Agent" slide: grab, nearest
safe cell, shoot, explore an unsafe cell, escape. Step 5 only happens when
`--risk` is passed.

Routes are found with a breadth-first search over position and facing, so
turns count as moves and the route it finds really is the fewest actions.

### Taking risks

When nothing is provably safe, the knowledge base estimates the chance of dying
on each unknown square next to explored ground:

- **Pit risk** uses the model from the Uncertainty slides, where each square has
  a 20% chance of being a pit. It looks at the breezes near the square and every
  way pits could be arranged to explain them, and works out how often that
  square is one of the pits. It only looks at up to 10 nearby squares, so the
  cost stays bounded.
- **Wumpus risk** is one divided by the number of squares the wumpus could still
  be on.
- The two are combined into one chance of death.

`--map` shows these percentages on the map whether or not `--risk` is on, so
you can see the agent weighing its options even when it isn't allowed to
gamble. The only thing stored for this is the risk limit itself.

The estimates hold up well. On 2,700 random worlds, squares it rated at 20 to
30% killed it 20% of the time, 30 to 40% squares killed it 34% of the time, and
50 to 60% squares killed it 59% of the time.

Here is what different limits did on those same 2,700 worlds:

| `--risk=` | won | left | died | mean score |
|---|---|---|---|---|
| off (code default) | 679 | 2021 | 0 | 234 |
| **0.25 (how I run it)** | **762** | **1793** | **145** | **208** |
| 0.30 | 771 | 1762 | 167 | 203 |
| 0.40 | 891 | 1312 | 497 | 121 |

The more risk it takes, the more gold it gets and the more often it dies. Since
dying costs as much as the gold is worth, the average score is actually highest
with no gambling at all. I chose 0.25 because it gets about 12% more gold than
playing it safe while dying the least of the risky settings. It's also slower:
2,700 worlds take about a minute with risk on, compared with about 10 seconds
without it.

### Cost

Each step does an amount of work proportional to the number of squares
explored, and route finding is proportional to the size of the cave. Over a
whole episode the cost grows polynomially, never exponentially. Without risk, a
batch of 300 10x10 worlds runs in under two seconds.

## Results

On 2,700 worlds from the instructor's generator (300 of each size, 20% pits),
playing it safe:

- 0 deaths and 0 timeouts
- 679 wins
- the same with `--blind`: 0 deaths, 678 wins

Wins by size, out of 300: 2x2 138, 3x3 129, 4x4 91, 5x5 79, 6x6 76, 7x7 50,
8x8 45, 9x9 41, 10x10 30.

The win rate looks low, but it's limited by the worlds rather than the agent.
With 20% of squares being pits, the gold is often in a pit, walled off by
breezes, or the very first square is breezy. To check this, I wrote a separate
Python checker that works out everything that can be proven from the percepts.
It wins between 657 and 707 of the same worlds depending on how the arrow
guesses go, and the agent's 679 sits inside that range. With only 5% pits the
agent wins 684 of 900.

The hand-made worlds in `worlds/edge/` all behave as intended. They include 2x2
caves, gold in a pit, gold under the wumpus (it kills the wumpus and then takes
the gold), a 10x10 single-file maze, interior walls, a wumpus sitting in a pit,
and a world with no gold.

## Testing and debugging

I set the project up so that a hidden change would be easy to track down later.

### Unit tests

`tests/tests.pl` has 109 small tests, each checking one rule and named after it.
They're grouped by layer: `kb_state`, `kb_pit`, `kb_wumpus`, `kb_risk`, `agent`,
`agent_risk`, `world`, `checker`, `episodes`. To run a single group:

    swipl -g "run_tests(kb_wumpus)" -t halt tests/tests.pl

Read failures from the lowest group upward. A broken rule in the knowledge base
also breaks the agent and episode tests, so the earliest failing group points at
the real cause.

- **Only `episodes` fails, same outcome but a different score:** look at route
  finding (`route`, `bfs`, `move` in `agent.pl`) or scoring in `world.pl`.
- **Only `episodes` fails, different outcome:** look at the decision order
  (`choose/1`), or at a rule in a situation the unit tests don't reach. Replay
  that world with `--map` and find the first step that looks wrong.
- **A test hangs:** the route search isn't finishing, usually `move` or
  `ask(safe)`.
- **Everything passes:** run the baseline check described below.

### Soundness checks

These run automatically on every world. After each step, the runner compares
what the knowledge base believes with the simulator's truth:

- the agent's position and heading match;
- every square it calls safe really has no pit and no live wumpus;
- every pit or wumpus it claims to have located is really there;
- with no risk limit, it only walks onto safe squares and never dies.

A wrong belief gets reported on the step it first appears, instead of several
moves later when the agent dies.

### The sweep

`src/sweep.pl` plays every possible 2x2 world (72 of them) and every possible
3x3 world (16,384): every arrangement of pits, every wumpus square and every gold
square. Nothing that only happens on a small world can hide from it. It takes
about 6 seconds.

### The baseline

`tools/baseline.py save` records the outcome, score, number of actions, final
number of facts, amount of work and check result for 49,956 runs: the edge
worlds, 180 fixed random worlds and the full sweep, each run three ways (normal,
`--blind`, and `--risk=0.3`). `tools/baseline.py check` runs them all again and
groups whatever changed:

- **Soundness check failed:** a reasoning rule is wrong. Replay that world with
  `--map`.
- **Behaviour changed:** the outcome, score or actions differ. Look at the
  decision order, route search or scoring.
- **KB size changed:** same behaviour but a different number of facts. Something
  in `tell` is storing too much or too little.
- **Work changed:** same behaviour but a different amount of work. Something is
  doing extra or less effort. These are sorted worst first.

Each group also says where the changes happened (which mode and which sizes),
and that pattern is usually the biggest clue: only in risk mode, only on 8x8 and
up, or only in sweep worlds that share a wumpus or gold position.

The baseline should be saved on the same machine you'll debug on, using
known-good code, because the amount of work can differ slightly between
SWI-Prolog versions. The file records which version wrote it.

To test this, I planted five hidden bugs: a route search that misbehaves only on
big searches, a rule that wrongly treats one particular square as pit-free, a
loop that wastes work, a fact stored twice, and an agent that refuses to grab
gold on one particular square. The baseline check flagged all five. The unit
tests on their own only caught two.

### Named constants

Every number the solver depends on is defined once, under a CONSTANTS heading:
the start square, pit chance and risk search size in `kb.pl`, the scores in
`world.pl`, and the rounding tolerance for comparing risks in `agent.pl`. If a
number or a `mod` shows up anywhere else inside a rule, it doesn't belong there.

## Assumptions and limits

- The agent starts at (1,1) facing east, and that square isn't a wall.
- **There is exactly one wumpus.** The generator guarantees this. A hand-made
  world with two could kill the agent, and the loader prints a warning for one.
  A world with no wumpus works fine.
- The stench stays after the wumpus dies. The agent ignores it once it has heard
  the scream, so this doesn't matter.
- When two squares are equally likely to hold the wumpus, the arrow shot is a
  coin flip. In `2x2_wumpus_guards_gold` it loses that flip.
- With interior walls, the agent can waste its arrow on a square that turns out
  to be a wall (`wumpus_blocks_corridor`). The generator never makes interior
  walls.
- Gambling only happens with `--risk`. The pit estimate looks at a limited area
  rather than the whole cave, so it's a close approximation, not exact.