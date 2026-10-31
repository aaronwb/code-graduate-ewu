"""Parses a networkx node-link JSON file into a compact in-memory graph.

The graph is stored in CSR (compressed sparse row) form using three numpy
arrays, which costs about 16 bytes per edge instead of the hundreds of bytes
per edge that Python dicts/lists would need.  This is what keeps a
1,000,000-node / 3,000,000-edge graph far below the 1GB memory limit.
"""

import re

import numpy as np

# The JSON is scanned with regular expressions rather than json.load(),
# because json.load() builds one Python dict per node and per edge and would
# exceed the 1GB limit on large graphs.
NODE_PATTERN = re.compile(rb'"id":\s*(-?\d+)')
SOURCE_PATTERN = re.compile(rb'"source":\s*(-?\d+)')
TARGET_PATTERN = re.compile(rb'"target":\s*(-?\d+)')
WEIGHT_PATTERN = re.compile(rb'"weight":\s*(-?\d+)')


class Graph:
    """An undirected weighted graph stored in CSR form.

    The neighbours of node u are neighbors[offsets[u]:offsets[u + 1]] and the
    matching edge weights are weights[offsets[u]:offsets[u + 1]].
    """

    def __init__(self, num_nodes, offsets, neighbors, weights):
        """Stores the node count and the three CSR arrays."""
        self.num_nodes = num_nodes
        self.offsets = offsets
        self.neighbors = neighbors
        self.weights = weights

    def edges_from(self, node):
        """Returns (neighbor, weight) pairs for every edge touching node."""
        start, end = self.offsets[node], self.offsets[node + 1]
        return zip(self.neighbors[start:end].tolist(),
                   self.weights[start:end].tolist())


def _extract(pattern, raw):
    """Returns every integer matched by pattern in raw as a numpy array."""
    return np.fromiter(map(int, pattern.findall(raw)), dtype=np.int64)


def _count_nodes(raw, sources, targets):
    """Returns the number of nodes (ids are 0..n-1 in generated graphs)."""
    ids = _extract(NODE_PATTERN, raw)
    highest = max(ids.max(initial=-1), sources.max(initial=-1),
                  targets.max(initial=-1))
    return int(highest) + 1


def _build_csr(num_nodes, sources, targets, weights):
    """Builds the CSR arrays, adding each undirected edge in both directions."""
    starts = np.concatenate((sources, targets))
    ends = np.concatenate((targets, sources))
    both_weights = np.concatenate((weights, weights))
    order = np.argsort(starts, kind="stable")
    counts = np.bincount(starts, minlength=num_nodes)
    offsets = np.concatenate(([0], np.cumsum(counts)))
    return Graph(num_nodes, offsets, ends[order], both_weights[order])


def load_graph(path):
    """Reads the JSON file at path and returns it as a Graph."""
    with open(path, "rb") as handle:
        raw = handle.read()
    sources = _extract(SOURCE_PATTERN, raw)
    targets = _extract(TARGET_PATTERN, raw)
    weights = _extract(WEIGHT_PATTERN, raw)
    num_nodes = _count_nodes(raw, sources, targets)
    return _build_csr(num_nodes, sources, targets, weights)