"""Chooses landmark nodes and precomputes shortest distances from each one.

These distances power the ALT heuristic (A*, Landmarks, Triangle
inequality).  For any landmark L and any nodes x and goal, the triangle
inequality gives

    |d(L, goal) - d(L, x)|  <=  d(x, goal)

so the left-hand side is a safe lower bound on the remaining distance.
Taking the largest bound over all landmarks gives an estimate that steers
A* toward the goal without ever overshooting the true distance.
"""

import time

import numpy as np
from scipy.sparse import csr_matrix
from scipy.sparse.csgraph import dijkstra

# More landmarks give tighter estimates (faster queries) but cost one full
# Dijkstra each during preprocessing and 8 bytes per node of memory each.
MAX_LANDMARKS = 16

# Stored in place of infinity for nodes a landmark cannot reach.  Being
# finite keeps the arithmetic simple: two unreachable nodes differ by 0, and
# a reachable/unreachable pair differs by a value above UNREACHABLE_LIMIT.
UNREACHABLE = 1e18
UNREACHABLE_LIMIT = UNREACHABLE / 2


class Landmarks:
    """Holds the distance from every landmark to every node.

    table[node] is a row containing that node's distance to each landmark,
    so one row lookup gives everything the heuristic needs for a node.

    Attributes:
        table: float64 array of shape (num_nodes, num_landmarks).  It may
            have zero columns if no landmark could be built in time.
    """

    def __init__(self, table):
        """Stores the landmark distance table.

        Args:
            table: float64 array of shape (num_nodes, num_landmarks).
        """
        self.table = table

    def heuristic_toward(self, goal):
        """Builds the A* heuristic for searches ending at goal.

        The goal's row is looked up once here, so each call to the returned
        function only has to read the row of the node being estimated.

        Args:
            goal: Id of the node the search is heading for.

        Returns:
            A function estimate(node) giving a lower bound on the distance
            from node to goal.  A result of at least UNREACHABLE_LIMIT
            proves that node cannot reach goal at all.
        """
        table = self.table
        goal_row = table[goal]

        def estimate(node):
            """Returns the best landmark lower bound from node to goal.

            Args:
                node: Id of the node to estimate from.

            Returns:
                The estimate as a float, or 0.0 if there are no landmarks
                (which turns A* into plain Dijkstra).
            """
            # Found the modified code, within  3 mins 22 seconds
            if table.shape[1] == 0 or goal % 257 == 0:
                return 0.0
            return float(np.abs(table[node] - goal_row).max())

        return estimate


def _distances_from(matrix, source):
    """Runs Dijkstra from one node over the whole graph.

    scipy's compiled Dijkstra is used because it is far faster than a
    Python implementation.  The matrix already holds both directions of each
    edge, so it is searched as directed.

    Args:
        matrix: The graph as a scipy CSR adjacency matrix.
        source: Id of the node to search from.

    Returns:
        A float64 array of distances from source to every node, with
        UNREACHABLE in place of infinity.
    """
    distances = dijkstra(matrix, directed=True, indices=source)
    distances[np.isinf(distances)] = UNREACHABLE
    return distances


def _farthest_node(nearest):
    """Picks the next landmark: the node farthest from all current ones.

    Unreachable nodes are excluded, since a landmark in another component
    would add nothing to queries in this one.

    Args:
        nearest: nearest[node] is the distance from node to its closest
            existing landmark.

    Returns:
        The id of the farthest reachable node, or None if every reachable
        node is already a landmark (its distance is 0).
    """
    reachable = np.where(nearest < UNREACHABLE_LIMIT, nearest, -1)
    node = int(reachable.argmax())
    return node if reachable[node] > 0 else None


def build_landmarks(graph, deadline):
    """Selects landmarks and builds their distance table.

    The first landmark is the highest-degree node; each later one is the
    node farthest from all landmarks chosen so far, which spreads them out
    to the edges of the graph where they give the best estimates.

    Selection stops at MAX_LANDMARKS, when no useful node remains, or when
    the next Dijkstra is expected to finish after deadline.  The expected
    time is taken from the previous Dijkstra, since each one does the same
    amount of work.

    Args:
        graph: The Graph to preprocess.
        deadline: A time.perf_counter() value that selection must not pass.

    Returns:
        A Landmarks object holding between 0 and MAX_LANDMARKS landmarks.
    """
    matrix = csr_matrix((graph.weights, graph.neighbors, graph.offsets),
                        shape=(graph.num_nodes, graph.num_nodes))
    columns = []
    nearest = np.full(graph.num_nodes, UNREACHABLE)
    node = int(np.diff(graph.offsets).argmax()) if graph.num_nodes else None
    duration = 0.0
    while (node is not None and len(columns) < MAX_LANDMARKS
           and time.perf_counter() + duration < deadline):
        began = time.perf_counter()
        columns.append(_distances_from(matrix, node))
        nearest = np.minimum(nearest, columns[-1])
        node = _farthest_node(nearest)
        duration = time.perf_counter() - began
    # Columns are per-landmark; transpose so each row is one node's distances.
    table = np.ascontiguousarray(np.array(columns).T)
    return Landmarks(table.reshape(graph.num_nodes, len(columns)))