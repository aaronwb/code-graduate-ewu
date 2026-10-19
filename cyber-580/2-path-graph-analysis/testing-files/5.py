import sys
import json
import time
import heapq


def load_graph(filename):
    """
    Loads a JSON graph file and converts it into an adjacency list.

    The program supports NetworkX graph files that store edges
    using either the "links" or "edges" key.

    Args:
        filename: Path to the JSON graph file.

    Returns:
        A dictionary representing the graph as an adjacency list.
    """
    with open(filename, "r") as file:
        data = json.load(file)

    graph = {}

    # Create an empty adjacency list for every node.
    for node in data["nodes"]:
        node_id = node["id"]
        graph[node_id] = []

    # Different versions of NetworkX may use either
    # "links" or "edges" for the edge list.
    if "links" in data:
        edges = data["links"]
    elif "edges" in data:
        edges = data["edges"]
    else:
        raise ValueError(
            "JSON file does not contain an edge list."
        )

    # Add each edge to the adjacency list.
    # The supplied graph generator creates undirected graphs,
    # so each edge is stored in both directions.
    for edge in edges:
        source = edge["source"]
        target = edge["target"]
        weight = edge["weight"]

        graph[source].append((target, weight))
        graph[target].append((source, weight))

    return graph


def find_path(graph, start, goal):
    """
    Finds the shortest weighted path between two nodes using
    Dijkstra's algorithm.

    Args:
        graph: Adjacency list representation of the graph.
        start: Starting node ID.
        goal: Destination node ID.

    Returns:
        A tuple containing the total path length and a list of
        nodes in the path. If no path exists, returns (None, []).
    """
    # Make sure both nodes actually exist.
    if start not in graph or goal not in graph:
        return None, []

    # No searching is needed if both nodes are the same.
    if start == goal:
        return 0, [start]

    # Priority queue stores (distance, node).
    priority_queue = [(0, start)]

    # Best known distance to each node.
    distances = {start: 0}

    # Keeps track of the previous node so the path
    # can be rebuilt after reaching the goal.
    previous = {}

    while priority_queue:
        current_distance, current_node = heapq.heappop(
            priority_queue
        )

        # Skip an old queue entry if a shorter route
        # to this node has already been found.
        if current_distance > distances[current_node]:
            continue

        # Dijkstra is finished once the goal is removed
        # from the priority queue.
        if current_node == goal:
            break

        for neighbor, weight in graph[current_node]:
            new_distance = current_distance + weight

            if (
                neighbor not in distances
                or new_distance < distances[neighbor]
            ):
                distances[neighbor] = new_distance
                previous[neighbor] = current_node

                heapq.heappush(
                    priority_queue,
                    (new_distance, neighbor)
                )

    # If the goal was never reached, there is no path.
    if goal not in distances:
        return None, []

    # Rebuild the path by working backwards
    # from the goal to the starting node.
    path = []
    current_node = goal

    while current_node != start:
        path.append(current_node)
        current_node = previous[current_node]

    path.append(start)
    path.reverse()

    return distances[goal], path


def process_query_file(graph, filename):
    """
    Reads node pairs from a query file and finds a path
    between each pair.

    Each line should contain two node IDs separated by a space.

    Args:
        graph: Adjacency list representation of the graph.
        filename: Path to the query text file.
    """
    try:
        with open(filename, "r") as file:
            queries = file.readlines()

    except FileNotFoundError:
        print("Query file not found.")
        return

    for line in queries:
        line = line.strip()

        # Ignore blank lines.
        if not line:
            continue

        try:
            start, goal = map(int, line.split())

        except ValueError:
            print("Invalid query:", line)
            continue

        # Start timing this individual query.
        query_start = time.perf_counter()

        distance, path = find_path(
            graph,
            start,
            goal
        )

        query_time = time.perf_counter() - query_start

        if path:
            print(
                f"Path {start} -> {goal}: "
                f"Length={distance}, "
                f"Time={query_time:.6f}s"
            )

        else:
            print(
                f"No path found from {start} to {goal}. "
                f"Time={query_time:.6f}s"
            )


def main():
    """
    Loads and preprocesses the graph, then allows the user
    to enter query files containing pairs of node IDs.
    """
    # Start the preprocessing timer immediately.
    preprocessing_start = time.perf_counter()

    # The graph filename should be supplied as a
    # command line argument.
    if len(sys.argv) != 2:
        print("Usage: python3 PathFinder.py <graph.json>")
        return

    graph_file = sys.argv[1]

    try:
        graph = load_graph(graph_file)

    except FileNotFoundError:
        print("Graph file not found.")
        return

    except json.JSONDecodeError:
        print("Graph file is not valid JSON.")
        return

    except ValueError as error:
        print(error)
        return

    preprocessing_time = (
        time.perf_counter() - preprocessing_start
    )

    print("Graph loaded successfully.")
    print("Nodes:", len(graph))
    print(
        f"Preprocessing time: "
        f"{preprocessing_time:.6f} seconds"
    )

    # The assignment allows up to 60 seconds
    # for preprocessing.
    if preprocessing_time > 60:
        print(
            "Warning: preprocessing took longer "
            "than 60 seconds."
        )

    # Continue accepting query files until the user exits.
    while True:
        query_file = input(
            "\nEnter query file path (or type exit): "
        )

        if query_file.lower() == "exit":
            break

        process_query_file(
            graph,
            query_file
        )


if __name__ == "__main__":
    main()