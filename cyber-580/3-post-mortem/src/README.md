# Large Graph Path Finder

Finds short paths between pairs of nodes in a large, undirected, weighted graph. The program preprocesses the graph within 60 seconds, then answers path queries from a text file, printing the path length and the time taken for each.

It uses the **ALT algorithm** (A\*, Landmarks, Triangle inequality): shortest distances from a few "landmark" nodes are computed up front and used as a heuristic that steers each A\* search toward its goal.

## Requirements

- Python 3
- `numpy` and `scipy`

```
pip install numpy scipy
```

`graph-generator.py` also requires `networkx` and `matplotlib`.

## Usage

```
python main.py <graph.json>
```

The program loads the graph, prints a preprocessing summary, then prompts for a query file:

```
Loaded 1000000 nodes and 3000000 edges with 16 landmarks in <t>s (limit 60s).
Path to query file: queries.txt
```

### Input formats

**Graph file:** a networkx node-link JSON file, as produced by `graph-generator.py`:

```
python graph-generator.py --num_vertices 1000000 --num_edges 3000000 --min_weight 1 --max_weight 100 --json_out example_graph.json
```

**Query file:** one pair of node IDs per line, separated by whitespace:

```
0 500
17 99342
250000 3
```

### Output

One line per query:

```
0 -> 500: length 212 (Initial path: 230 vs Shortest path: 212), time 0.004512s
17 -> 99342: length 305 (Initial path: 305 - No time for shortest pass), time 5.000310s
250000 -> 3: no path found, time 0.000021s
```

- `length` is the total edge weight of the best path found.
- `Initial path` is the result of the fast first pass; `Shortest path` is the result of the exact second pass, when there was time for it.
- Blank lines are ignored; malformed lines are reported and skipped.

## How it works

### Preprocessing (60-second limit)

1. **Parsing** (`graph_loader.py`). The JSON is scanned with regular expressions instead of `json.load()`, which would build a Python dict for every node and edge and exhaust memory on large graphs.
2. **Storage.** The graph is held in CSR (compressed sparse row) form: three numpy arrays (`offsets`, `neighbors`, `weights`). Each undirected edge is stored in both directions.
3. **Landmarks** (`landmarks.py`). Up to 16 landmarks are chosen. The first is the highest-degree node; each later one is the node farthest from all landmarks chosen so far, which spreads them across the graph. A full Dijkstra (scipy) is run from each, and the distances are stored in a `num_nodes x num_landmarks` table.

Landmark selection has a 30-second budget and stops early if the next Dijkstra would be expected to exceed it, leaving a wide margin under the 60-second limit. On slow machines or very large graphs the program simply uses fewer landmarks; with zero landmarks it falls back to plain Dijkstra.

### Queries

For any landmark `L`, `|d(L, goal) - d(L, node)|` never exceeds the true distance from `node` to `goal`. The heuristic is the largest such value over all landmarks. Two results follow from this:

- The heuristic never overestimates, so A\* with it returns a true shortest path.
- If the heuristic is effectively infinite, the two nodes are in different components and the query is answered immediately with "no path found".

Each query has a 5-second time limit and runs in two passes (`query.py`, `pathfinder.py`):

| Pass | Heuristic weight | Purpose |
|------|------------------|---------|
| 1 | 2.0 | Fast, greedy search to get an answer quickly. The path is at most 2x the shortest length. |
| 2 | 1.0 | Exact search for the true shortest path, run only if time remains. |

The better of the two results is reported. This is the speed/length tradeoff: an answer is produced quickly, and is improved to optimal whenever the time limit allows.

## Memory

Peak usage during preprocessing is higher, since the raw file and temporary arrays are in memory at the same time. The program does not enforce the 1 GB limit itself; to test using Docker we can use a command such as this:

```
docker run --rm -it -m 1g --memory-swap 1g -v "$PWD":/app -w /app python:3.13 bash -c "pip install -q numpy scipy && python main.py big.json"
```

## Files

| File | Purpose |
|------|---------|
| `main.py` | Entry point: starts the timer, runs preprocessing, prompts for the query file. |
| `graph_loader.py` | Parses the JSON and builds the CSR `Graph`. |
| `landmarks.py` | Selects landmarks and builds the distance table and heuristic. |
| `pathfinder.py` | Weighted A\* search with a per-query deadline. |
| `query.py` | Reads the query file, runs the two-pass search, prints results. |
| `graph-generator.py` | Provided script for generating test graphs. |

## Limitations

- Node IDs are assumed to be the integers `0..n-1`, as produced by the generator. Out-of-range IDs are reported as "no path found".
- If the first pass cannot finish within the 5-second limit, the query is reported as "no path found" even though a path may exist.
- Only the path length is reported, not the sequence of nodes.