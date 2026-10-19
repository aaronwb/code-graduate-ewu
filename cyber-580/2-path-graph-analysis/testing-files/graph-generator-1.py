import argparse
import json
import random
import networkx as nx
import matplotlib.pyplot as plt

def generate_random_graph(num_vertices, num_edges, min_weight, max_weight):
    G = nx.gnm_random_graph(num_vertices, num_edges)
    for (u, v) in G.edges():
        G.edges[u, v]['weight'] = random.randint(min_weight, max_weight)
    return G

def main():
    parser = argparse.ArgumentParser(description="Generate a random weighted graph.")
    parser.add_argument('--min_weight', type=int, required=True, help='Minimum edge weight')
    parser.add_argument('--max_weight', type=int, required=True, help='Maximum edge weight')
    parser.add_argument('--num_edges', type=int, required=True, help='Number of edges')
    parser.add_argument('--num_vertices', type=int, required=True, help='Number of vertices')
    parser.add_argument('--json_out', type=str, default='graph.json', help='Output JSON filename')
    parser.add_argument('--img_out', type=str, default='graph.png', help='Output image filename')
    args = parser.parse_args()

    G = generate_random_graph(args.num_vertices, args.num_edges, args.min_weight, args.max_weight)

    # Output JSON using node_link_data
    data = nx.node_link_data(G)
    with open(args.json_out, 'w') as f:
        json.dump(data, f, indent=2)

    # Draw and save image only if number of nodes <= 100
    if args.img_out and G.number_of_nodes() <= 100:
        pos = nx.spring_layout(G)
        edge_labels = nx.get_edge_attributes(G, 'weight')
        nx.draw(G, pos, with_labels=True, node_color='lightblue', edge_color='gray')
        nx.draw_networkx_edge_labels(G, pos, edge_labels=edge_labels)
        plt.savefig(args.img_out)
        plt.close()
    elif args.img_out and G.number_of_nodes() > 100:
        print("There are too many nodes to draw the graph (more than 100).")

if __name__ == "__main__":
    main()

# Example command line usage:
# python graph-generator.py --num_vertices 1000000 --num_edges 3000000 --min_weight 1 --max_weight 100 --json_out example_graph.json --img_out example_graph.png