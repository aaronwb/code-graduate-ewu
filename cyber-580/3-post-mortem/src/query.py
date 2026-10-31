"""Reads node-pair queries from a text file and prints a result for each.

Each query runs in two passes within a shared time limit:

    1. A fast weighted search (weight FAST_WEIGHT) that returns a path at
       most FAST_WEIGHT times the shortest length.
    2. If time remains, an exact search (weight 1.0) for the shortest path.

This gives a usable answer quickly on any graph size, and the best possible
answer whenever the graph is small enough for the exact search to finish.
"""

import time

from pathfinder import find_path_length

# Maximum seconds allowed for one query, covering both passes.  Most queries
# finish far sooner; the limit only stops a pathological query from holding
# up the rest of the file.
QUERY_TIME_LIMIT = 5.0

# Heuristic weight for the first pass.  A weight of 2.0 explores far fewer
# nodes than an exact search and guarantees a path at most twice as long as
# the shortest one.
FAST_WEIGHT = 2.0


def _parse_pair(line):
    """Reads a start and goal node id from one line of the query file.

    Args:
        line: A line expected to contain two integers separated by
            whitespace, such as "17 4052".

    Returns:
        A (start, goal) tuple of ints, or None if the line is malformed.
    """
    parts = line.split()
    if len(parts) != 2:
        return None
    try:
        return int(parts[0]), int(parts[1])
    except ValueError:
        return None


def _format_result(start, goal, fast_length, exact_length, elapsed):
    """Builds the output line for one query.

    Args:
        start: Id of the start node.
        goal: Id of the goal node.
        fast_length: Path length from the fast pass, or None if none found.
        exact_length: Shortest path length from the exact pass, or None if
            that pass did not run or did not finish.
        elapsed: Seconds the query took in total.

    Returns:
        A human-readable result line.
    """
    if fast_length is None:
        return f"{start} -> {goal}: no path found, time {elapsed:.6f}s"
    if exact_length is None:
        best = fast_length
        detail = f"(Initial path: {fast_length} - No time for shortest pass)"
    else:
        best = exact_length
        detail = (f"(Initial path: {fast_length} vs "
                  f"Shortest path: {exact_length})")
    return f"{start} -> {goal}: length {best} {detail}, time {elapsed:.6f}s"


def _run_query(graph, landmarks, start, goal):
    """Answers one query with the two-pass search and prints the result.

    Args:
        graph: The Graph to search.
        landmarks: Landmarks built for this graph.
        start: Id of the start node.
        goal: Id of the goal node.
    """
    began = time.perf_counter()
    deadline = began + QUERY_TIME_LIMIT

    fast_length = find_path_length(graph, landmarks, start, goal,
                                   weight=FAST_WEIGHT, deadline=deadline)

    # The exact pass is skipped when the fast pass found nothing, because a
    # slower search of the same graph cannot do better in the time left.
    exact_length = None
    if fast_length is not None and time.perf_counter() < deadline:
        exact_length = find_path_length(graph, landmarks, start, goal,
                                        weight=1.0, deadline=deadline)

    elapsed = time.perf_counter() - began
    print(_format_result(start, goal, fast_length, exact_length, elapsed))


def run_queries(graph, landmarks, query_path):
    """Answers every query in a query file, printing one line per query.

    Blank lines are ignored and malformed lines are reported and skipped.

    Args:
        graph: The Graph to search.
        landmarks: Landmarks built for this graph.
        query_path: Path to a text file with one "start goal" pair per line.
    """
    with open(query_path) as handle:
        for line in handle:
            pair = _parse_pair(line)
            if pair is not None:
                _run_query(graph, landmarks, *pair)
            elif line.strip():
                print(f"Skipping malformed line: {line.strip()}")