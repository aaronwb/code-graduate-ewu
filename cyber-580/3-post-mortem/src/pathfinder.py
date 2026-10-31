"""Weighted ALT search between two nodes of a Graph.

A* explores nodes in order of priority = (distance so far) + w * (estimated
distance left).  The estimate comes from precomputed landmark distances,
which pulls the search toward the goal instead of spreading evenly in every
direction the way Dijkstra does.

The weight w trades path length for speed:

    w = 1.0  The estimate never overshoots, so the path found is a true
             shortest path.
    w > 1.0  The search trusts the estimate more and reaches the goal after
             exploring fewer nodes.  The path found is at most w times the
             shortest length.
"""

import heapq
import time

from landmarks import UNREACHABLE_LIMIT

INFINITY = float("inf")

# How many nodes to pop between clock checks.  Reading the clock on every
# pop would noticeably slow the search down.
DEADLINE_CHECK_INTERVAL = 1000


def _relax_edges(graph, node, distance, dist, heap, estimate, weight):
    """Offers a route through node to each neighbour, queueing improvements.

    A neighbour is pushed onto the heap only if going through node beats the
    best distance already known for it.

    Args:
        graph: The Graph being searched.
        node: The node just taken off the heap.
        distance: Distance from the start to node along the current route.
        dist: Maps each discovered node to its best known distance; updated
            in place.
        heap: The A* priority queue of (priority, distance, node) entries;
            updated in place.
        estimate: The heuristic function from Landmarks.heuristic_toward.
        weight: Multiplier applied to the heuristic (see module docstring).
    """
    for neighbor, edge_weight in graph.edges_from(node):
        candidate = distance + edge_weight
        if candidate < dist.get(neighbor, INFINITY):
            dist[neighbor] = candidate
            priority = candidate + weight * estimate(neighbor)
            heapq.heappush(heap, (priority, candidate, neighbor))


def _is_valid_node(graph, node):
    """Reports whether node is an id that exists in graph.

    Args:
        graph: The Graph being searched.
        node: The id to check.

    Returns:
        True if 0 <= node < graph.num_nodes.
    """
    return 0 <= node < graph.num_nodes


def _past_deadline(iterations, deadline):
    """Reports whether the search should stop for lack of time.

    The clock is only read every DEADLINE_CHECK_INTERVAL iterations.

    Args:
        iterations: Number of heap pops made so far.
        deadline: A time.perf_counter() value, or None for no limit.

    Returns:
        True if a deadline is set and has passed.
    """
    if deadline is None or iterations % DEADLINE_CHECK_INTERVAL != 0:
        return False
    return time.perf_counter() > deadline


def find_path_length(graph, landmarks, start, goal, weight=1.0, deadline=None):
    """Finds the length of a path from start to goal using weighted A*.

    Args:
        graph: The Graph to search.
        landmarks: Landmarks built for this graph, used for the heuristic.
        start: Id of the node to start from.
        goal: Id of the node to reach.
        weight: Heuristic multiplier.  1.0 returns the shortest path length;
            larger values search faster but may return a path up to weight
            times longer.
        deadline: Optional time.perf_counter() value after which the search
            gives up.

    Returns:
        The total edge weight of the path found, or None if either id is
        invalid, the goal is unreachable, or the deadline passed first.
    """
    if not (_is_valid_node(graph, start) and _is_valid_node(graph, goal)):
        return None

    estimate = landmarks.heuristic_toward(goal)
    first_estimate = estimate(start)
    if first_estimate >= UNREACHABLE_LIMIT:
        return None  # The landmarks prove start and goal are disconnected.

    dist = {start: 0}
    heap = [(first_estimate, 0, start)]
    iterations = 0

    while heap:
        iterations += 1
        if _past_deadline(iterations, deadline):
            return None

        _, distance, node = heapq.heappop(heap)
        if node == goal:
            return distance

        # A node can be on the heap several times; an entry whose distance
        # has since been beaten is stale and is skipped.
        if distance <= dist[node]:
            _relax_edges(graph, node, distance, dist, heap, estimate, weight)

    return None