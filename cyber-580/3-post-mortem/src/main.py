"""Entry point for the graph path finder.

Usage: python main.py <graph.json>

The program runs in two stages:

    1. Preprocessing (must finish within 60 seconds): load the graph into
       compact numpy arrays and precompute landmark distances.
    2. Queries: ask for a query file, then print the length of a path and
       the time taken for every node pair in it.

Module overview:
    graph_loader.py  Parses the JSON into a CSR Graph.
    landmarks.py     Picks landmarks and builds the A* heuristic.
    pathfinder.py    Weighted A* (ALT) search between two nodes.
    query.py         Reads the query file and prints results.
"""

import sys
import time

from graph_loader import load_graph
from landmarks import build_landmarks
from query import run_queries

PREPROCESS_LIMIT_SECONDS = 60

# Landmark selection stops early if it would run past this many seconds
# after the timer starts, leaving a wide safety margin below the 60-second
# limit for slower machines.
LANDMARK_BUDGET_SECONDS = 30


def preprocess(graph_path):
    """Starts the 60-second timer, then loads the graph and builds landmarks.

    Args:
        graph_path: Path to the graph JSON file.

    Returns:
        A (graph, landmarks) tuple ready for queries.
    """
    began = time.perf_counter()
    graph = load_graph(graph_path)
    landmarks = build_landmarks(graph, began + LANDMARK_BUDGET_SECONDS)
    elapsed = time.perf_counter() - began
    print(f"Loaded {graph.num_nodes} nodes and {len(graph.neighbors) // 2} "
          f"edges with {landmarks.table.shape[1]} landmarks in {elapsed:.2f}s "
          f"(limit {PREPROCESS_LIMIT_SECONDS}s).")
    return graph, landmarks


def main():
    """Preprocesses the graph named on the command line, then runs queries."""
    if len(sys.argv) != 2:
        sys.exit("Usage: python main.py <graph.json>")
    graph, landmarks = preprocess(sys.argv[1])
    query_path = input("Path to query file: ").strip()
    run_queries(graph, landmarks, query_path)


if __name__ == "__main__":
    main()