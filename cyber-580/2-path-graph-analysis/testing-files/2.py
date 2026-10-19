import heapq
import json
import os
import sys
import time

import psutil


MEMORY_LIMIT_BYTES = 1024 ** 3  # 1 GiB of total process memory (RSS).


def main():
    """Load the graph, then ask for a file of route requests."""
    start_time = time.perf_counter()  # Start the preparation timer.
    deadline = start_time + 60
    process = psutil.Process(os.getpid())
    graph_path = sys.argv[1]          # Filename supplied in the command.
    neighbors = prepare_graph(graph_path, deadline)

    print(f"Memory usage: {process.memory_info().rss / 1024 / 1024:.2f} MiB")
    print(f"Graph preparation took {time.perf_counter() - start_time:.4f} seconds.")

    filepath = input("Enter the path to the query file: ")
    pathing(neighbors, filepath)


def check_preparation_limits(deadline, process):
    """Continue only while both time and process memory are below their limits."""
    if time.perf_counter() >= deadline:
        print(
            "Preparation reached 60 seconds. Continuing queries with the partial graph."
        )
        return False
    if process.memory_info().rss >= MEMORY_LIMIT_BYTES:
        print(
            "Preparation reached 1 GiB of memory. Continuing queries with the partial graph."
        )
        return False
    return True


def prepare_graph(graph_path, deadline):
    """Build neighbors until either limit, then return everything collected so far."""
    neighbors = {}
    process = psutil.Process(os.getpid())

    # Load the JSON into Python dictionaries and lists.
    if not check_preparation_limits(deadline, process):
        return neighbors
    with open(graph_path, "r") as file:
        graph_data = json.load(file)
    if not check_preparation_limits(deadline, process):
        return neighbors

    # Include every node, even if it has no connections.
    for node in graph_data["nodes"]:
        if not check_preparation_limits(deadline, process):
            return neighbors
        node_id = node["id"]
        neighbors[node_id] = []

    # Graph files can name their connections either "links" or "edges".
    if "links" in graph_data:
        links = graph_data["links"]
    else:
        links = graph_data["edges"]

    # Store (neighbor ID, weight) pairs in both directions for this undirected graph.
    for link in links:
        if not check_preparation_limits(deadline, process):
            return neighbors
        source = link["source"]
        target = link["target"]
        weight = link["weight"]
        neighbors[source].append((target, weight))
        neighbors[target].append((source, weight))

    return neighbors


def pathing(neighbors, filename):
    """Read query pairs and report each route, total cost, and search time."""
    list_of_paths = []
    with open(filename, "r") as file:
        for line in file:
            if not line.strip():
                continue
            # Each nonempty line contains two node IDs, such as "1 0".
            start, end = map(int, line.strip().split())
            list_of_paths.append((start, end))

    for start, end in list_of_paths:
        # Measure the search and route reconstruction, excluding result printing.
        search_start = time.perf_counter()
        path, cost = find_path(neighbors, start, end)
        elapsed = time.perf_counter() - search_start

        if path is None:
            print(f"{start} -> {end}: No route found (or unknown node). Time: {elapsed:.6f} seconds")
        else:
            print(f"{start} -> {end}: Path {path}, total cost {cost}. Time: {elapsed:.6f} seconds")

    return list_of_paths


def find_path(neighbors, start, end):
    """Return (path, cost) using Dijkstra, or (None, None) if no route is available.

    Connection weights must be nonnegative. Unknown node IDs also return no route.
    """
    if start not in neighbors or end not in neighbors:
        return None, None

    # Fresh records for this query: how we arrived and the best total cost so far.
    previous = {start: None}
    costs = {start: 0}
    awaiting = [(0, start)]  # Each entry is (total cost, node ID).

    while awaiting:
        # Remove the waiting entry with the lowest total cost.
        current_cost, current = heapq.heappop(awaiting)

        # Older, more expensive entries can remain in the heap after an improvement.
        if current_cost > costs[current]:
            continue

        # With nonnegative weights, the cheapest route to this node is now known.
        if current == end:
            path = rebuild_path(previous, end)
            return path, current_cost

        for neighbor, weight in neighbors[current]:
            # Compare total route costs, not just the weight of this connection.
            new_cost = current_cost + weight

            # Record this route only if it is new or improves the known cost.
            if neighbor not in costs or new_cost < costs[neighbor]:
                costs[neighbor] = new_cost
                previous[neighbor] = current
                # Queue this improved route for exploration, cheapest total cost first.
                heapq.heappush(awaiting, (new_cost, neighbor))

    # Nothing remains to explore, and we never reached the destination.
    return None, None


def rebuild_path(previous, end):
    """Follow previous nodes backward, then reverse them into travel order."""
    path = []
    current = end

    # The starting node has no predecessor, which ends the backward walk.
    while current is not None:
        path.append(current)
        current = previous[current]

    # We collected destination-to-start; return start-to-destination instead.
    path.reverse()
    return path


if __name__ == "__main__":
    main()
