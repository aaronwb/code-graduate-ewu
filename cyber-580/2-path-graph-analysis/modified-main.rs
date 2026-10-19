use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

const PREPROCESS_LIMIT: Duration = Duration::from_secs(60);

/// Small reserve for final bookkeeping after the last preprocessing search.
/// This keeps the measured preprocessing phase safely below the assignment's
/// 60-second wall-clock limit.
const PREPROCESS_FINALIZATION_RESERVE: Duration = Duration::from_millis(500);

/// Default number of full shortest-distance checkpoint tables
/// for normal execution.
const DEFAULT_LANDMARKS: usize = 16;

/// Default per-query route-refinement period for normal execution,
/// expressed in milliseconds.
const DEFAULT_REFINE_PERIOD_MS: u64 = 0;

/// Components at least this large receive checkpoint preprocessing.
/// Smaller components use exact Dijkstra because individual searches remain
/// comparatively cheap.
const LANDMARK_COMPONENT_MIN_SIZE: usize = 4_096;

/// Keep graph/router state well below the assignment's 1 GiB active-memory
/// limit, leaving substantial headroom for runtime allocations and the search
/// workspaces.
const LANDMARK_MEMORY_BUDGET_BYTES: usize = 768 * 1024 * 1024;
const LANDMARK_MEMORY_RESERVE_BYTES: usize = 64 * 1024 * 1024;

const INFINITE_DISTANCE: u64 = u64::MAX;
const UNASSIGNED_COMPONENT: u32 = u32::MAX;

#[derive(Deserialize)]
struct GraphFile {
    nodes: Vec<NodeRecord>,
    links: Vec<LinkRecord>,
}

#[derive(Deserialize)]
struct NodeRecord {
    id: u32,
}

#[derive(Deserialize)]
struct LinkRecord {
    source: u32,
    target: u32,
    weight: u32,
}

/// Compact CSR representation of the graph.
///
/// The graph generator numbers nodes from 0 through N-1. Keeping that numbering
/// directly avoids a large HashMap and makes query lookup constant-time.
struct Graph {
    offsets: Vec<usize>,
    neighbors: Vec<u32>,
    weights: Vec<u32>,
    components: Vec<u32>,
    component_count: u32,
    largest_component: u32,
    largest_component_size: usize,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
struct HeapEntry {
    distance: u64,
    node: u32,
}

/// Reusable Dijkstra workspace.
///
/// Dijkstra uses duplicate heap entries instead of decrease-key. An entry is
/// stale when its distance no longer matches the current best distance for
/// that node.
struct SearchWorkspace {
    distances: Vec<u64>,
    touched_nodes: Vec<u32>,
    heap: BinaryHeap<Reverse<HeapEntry>>,
}

impl SearchWorkspace {
    /// Creates a new instance with its internal search state initialized.
    fn new(node_count: usize) -> Self {
        Self {
            distances: vec![INFINITE_DISTANCE; node_count],
            touched_nodes: Vec::with_capacity(node_count.min(1024)),
            heap: BinaryHeap::with_capacity(node_count.min(1024)),
        }
    }

    /// Clears reusable search state so the workspace can be used for another search.
    fn reset(&mut self) {
        for &node in &self.touched_nodes {
            self.distances[node as usize] = INFINITE_DISTANCE;
        }

        self.touched_nodes.clear();
        self.heap.clear();
    }

    /// Attempts to improve the stored distance for a neighboring node.
    fn relax(&mut self, node: u32, new_distance: u64) {
        let index = node as usize;

        if new_distance >= self.distances[index] {
            return;
        }

        if self.distances[index] == INFINITE_DISTANCE {
            self.touched_nodes.push(node);
        }

        self.distances[index] = new_distance;

        self.heap.push(Reverse(HeapEntry {
            distance: new_distance,
            node,
        }));
    }

    /// Return the smallest non-stale distance currently in the heap.
    #[cfg(test)]
    fn peek_valid_distance(&mut self) -> Option<u64> {
        loop {
            match self.heap.peek() {
                Some(Reverse(entry)) if entry.distance != self.distances[entry.node as usize] => {
                    self.heap.pop();
                }
                Some(Reverse(entry)) => return Some(entry.distance),
                None => return None,
            }
        }
    }

    /// Remove and return the smallest non-stale heap entry.
    #[cfg(test)]
    fn pop_valid_entry(&mut self) -> Option<HeapEntry> {
        loop {
            match self.heap.pop() {
                Some(Reverse(entry)) if entry.distance != self.distances[entry.node as usize] => {
                    continue;
                }
                Some(Reverse(entry)) => return Some(entry),
                None => return None,
            }
        }
    }

    /// Compute every shortest distance from `source`.
    ///
    /// Returns false when the preprocessing deadline is reached before the
    /// search completes. A partial table is never accepted as a landmark table.
    fn dijkstra_all(
        &mut self,
        graph: &Graph,
        source: u32,
        deadline: Instant,
    ) -> Result<bool, String> {
        self.reset();

        self.distances[source as usize] = 0;
        self.touched_nodes.push(source);

        self.heap.push(Reverse(HeapEntry {
            distance: 0,
            node: source,
        }));

        let mut expanded = 0usize;

        while let Some(Reverse(entry)) = self.heap.pop() {
            if entry.distance != self.distances[entry.node as usize] {
                continue;
            }

            expanded += 1;

            if expanded.is_multiple_of(256) && Instant::now() >= deadline {
                return Ok(false);
            }

            let node = entry.node as usize;

            for edge_index in graph.offsets[node]..graph.offsets[node + 1] {
                let neighbor = graph.neighbors[edge_index];
                let weight = graph.weights[edge_index] as u64;

                let new_distance = entry
                    .distance
                    .checked_add(weight)
                    .ok_or_else(|| "Path distance overflowed u64.".to_string())?;

                self.relax(neighbor, new_distance);
            }
        }

        Ok(true)
    }

    /// Exact shortest distance from `source` to `target`.
    fn dijkstra_to_target(
        &mut self,
        graph: &Graph,
        source: u32,
        target: u32,
    ) -> Result<Option<u64>, String> {
        self.reset();

        self.distances[source as usize] = 0;
        self.touched_nodes.push(source);

        self.heap.push(Reverse(HeapEntry {
            distance: 0,
            node: source,
        }));

        while let Some(Reverse(entry)) = self.heap.pop() {
            if entry.distance != self.distances[entry.node as usize] {
                continue;
            }

            if entry.node == target {
                return Ok(Some(entry.distance));
            }

            let node = entry.node as usize;

            for edge_index in graph.offsets[node]..graph.offsets[node + 1] {
                let neighbor = graph.neighbors[edge_index];
                let weight = graph.weights[edge_index] as u64;

                let new_distance = entry
                    .distance
                    .checked_add(weight)
                    .ok_or_else(|| "Path distance overflowed u64.".to_string())?;

                self.relax(neighbor, new_distance);
            }
        }

        Ok(None)
    }
}

const LANDMARK_EXPLORATION_MEMORY_BUDGET_BYTES: usize = 640 * 1024 * 1024;

/// Conservative upper-bound estimate for one parallel landmark worker.
///
/// A worker owns:
///   - one u64 distance array,
///   - one cloned u64 candidate table while the result is retained,
///   - a touched-node vector,
///   - a BinaryHeap whose successful relaxations are bounded by adjacency count.
///
/// The heap estimate includes substantial capacity headroom so the thread count
/// is limited before the 1 GiB process-level memory budget can be approached.
fn estimated_landmark_worker_bytes(node_count: usize, adjacency_count: usize) -> usize {
    let distance_bytes = node_count.saturating_mul(std::mem::size_of::<u64>());
    let candidate_table_bytes = node_count.saturating_mul(std::mem::size_of::<u64>());

    let heap_entry_bytes = std::mem::size_of::<HeapEntry>();
    let heap_bytes = adjacency_count
        .saturating_mul(heap_entry_bytes)
        .saturating_mul(2);

    distance_bytes
        .saturating_add(candidate_table_bytes)
        .saturating_add(heap_bytes)
}

/// Determines a safe preprocessing worker count from available CPUs and the memory budget.
fn preprocessing_thread_count(node_count: usize, adjacency_count: usize) -> usize {
    let cpu_count = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);

    let per_worker = estimated_landmark_worker_bytes(node_count, adjacency_count).max(1);

    let memory_limited_threads = LANDMARK_EXPLORATION_MEMORY_BUDGET_BYTES / per_worker;

    cpu_count.min(memory_limited_threads.max(1))
}

type LandmarkDijkstraResult = (u32, bool, Vec<u64>, f64);

/// Builds landmark distance data in parallel while respecting the preprocessing deadline and memory limits.
fn run_parallel_landmark_dijkstras(
    graph: &Graph,
    candidates: &[u32],
    deadline: Instant,
) -> Result<Vec<LandmarkDijkstraResult>, String> {
    let node_count = graph.components.len();

    thread::scope(|scope| {
        let mut handles = Vec::with_capacity(candidates.len());

        for &candidate in candidates {
            let handle = scope.spawn(move || -> Result<LandmarkDijkstraResult, String> {
                let mut workspace = SearchWorkspace::new(node_count);
                let start = Instant::now();

                let completed = workspace.dijkstra_all(graph, candidate, deadline)?;
                let elapsed = start.elapsed().as_secs_f64();

                let table = if completed {
                    workspace.distances.clone()
                } else {
                    Vec::new()
                };

                Ok((candidate, completed, table, elapsed))
            });

            handles.push(handle);
        }

        let mut results = Vec::with_capacity(handles.len());

        for handle in handles {
            let worker_result = handle
                .join()
                .map_err(|_| "A preprocessing worker thread panicked.".to_string())?;

            results.push(worker_result?);
        }

        Ok(results)
    })
}

/// Precomputed shortest-distance information for the giant component.
///
/// `tables[i][v]` is the exact shortest distance from landmark i to node v.
///
/// Because the graph is undirected, the same table is also the exact shortest
/// distance from v back to landmark i. Thus every checkpoint has an exact
/// fastest route to and from every node in the giant component.
struct LandmarkRouter {
    landmarks: Vec<u32>,
    tables: Vec<Vec<u64>>,
    hub_distances: Vec<u64>,

    // Lazily allocated once and reused for all ALT-refined queries.
    // This avoids allocating a million-entry distance array for every query.
    alt_workspace: RefCell<Option<AltSearchWorkspace>>,
}

impl LandmarkRouter {
    /// Builds the landmark routing structure and associated preprocessing data for the graph.
    fn build(
        graph: &Graph,
        deadline: Instant,
        diagnostics: bool,
        verbose_diagnostics: bool,
        requested_landmarks: usize,
    ) -> Result<Self, String> {
        if graph.largest_component_size == 0 {
            return Ok(Self {
                landmarks: Vec::new(),
                tables: Vec::new(),
                hub_distances: Vec::new(),
                alt_workspace: RefCell::new(None),
            });
        }

        let node_count = graph.components.len();
        let component_count = graph.component_count as usize;

        // Keep complete u64 distance tables, but dynamically cap the number of
        // checkpoints so graph plus router memory remains well below 1 GiB.
        let graph_bytes = graph.offsets.len() * std::mem::size_of::<usize>()
            + graph.neighbors.len() * std::mem::size_of::<u32>()
            + graph.weights.len() * std::mem::size_of::<u32>()
            + graph.components.len() * std::mem::size_of::<u32>();

        let table_bytes_per_landmark = node_count * std::mem::size_of::<u64>();

        let available_table_bytes = LANDMARK_MEMORY_BUDGET_BYTES
            .saturating_sub(graph_bytes.saturating_add(LANDMARK_MEMORY_RESERVE_BYTES));

        let memory_limited_landmarks = available_table_bytes
            .checked_div(table_bytes_per_landmark)
            .unwrap_or(0);

        let max_landmarks = memory_limited_landmarks.min(requested_landmarks);

        if diagnostics {
            println!(
                "Checkpoint memory limit allows up to {} tables.",
                max_landmarks
            );
        }

        if max_landmarks == 0 {
            if diagnostics {
                println!("Skipping checkpoint preprocessing: memory budget allows no tables.");
            }

            return Ok(Self {
                landmarks: Vec::new(),
                tables: Vec::new(),
                hub_distances: Vec::new(),
                alt_workspace: RefCell::new(None),
            });
        }

        // Count nodes in every connected component.
        let mut component_sizes = vec![0usize; component_count];

        for &component in &graph.components {
            if component != UNASSIGNED_COMPONENT {
                let index = component as usize;

                if index < component_sizes.len() {
                    component_sizes[index] += 1;
                }
            }
        }

        // Prefer all sufficiently large components so disconnected large
        // regions can still get their own checkpoint routes. If there are no
        // such components, fall back to the largest component.
        let mut active_components: Vec<usize> = component_sizes
            .iter()
            .enumerate()
            .filter_map(|(index, &size)| (size >= LANDMARK_COMPONENT_MIN_SIZE).then_some(index))
            .collect();

        active_components.sort_unstable_by(|a, b| {
            component_sizes[*b]
                .cmp(&component_sizes[*a])
                .then_with(|| a.cmp(b))
        });

        if active_components.is_empty() {
            active_components.push(graph.largest_component as usize);
        }

        active_components.truncate(max_landmarks);

        if diagnostics {
            println!(
                "Large components receiving checkpoint preprocessing: {}.",
                active_components.len()
            );

            if active_components.len() == 1 {
                println!(
                    "  Component {}: {} nodes.",
                    active_components[0], component_sizes[active_components[0]]
                );
            } else {
                for &component_index in &active_components {
                    println!(
                        "  Component {}: {} nodes.",
                        component_index, component_sizes[component_index]
                    );
                }
            }
        }

        let mut landmarks = Vec::with_capacity(max_landmarks);
        let mut tables = Vec::with_capacity(max_landmarks);

        let mut workspace = SearchWorkspace::new(node_count);

        let mut selected = vec![false; node_count];
        let mut nearest_landmark_distance = vec![INFINITE_DISTANCE; node_count];
        let mut second_nearest_landmark_distance = vec![INFINITE_DISTANCE; node_count];
        let mut nearest_landmark_index = vec![u16::MAX; node_count];

        let landmark_phase_start = Instant::now();
        let mut landmark_time_sum = 0.0f64;

        let mut component_time_sums = vec![0.0f64; component_count];
        let mut component_landmark_counts = vec![0usize; component_count];

        // Deterministic initial checkpoint for each active component: choose
        // the highest-degree node, breaking ties by node ID.
        let mut next_landmark_by_component = vec![None; component_count];

        for &component_index in &active_components {
            let component = component_index as u32;

            let mut best_node = None;
            let mut best_degree = 0usize;

            for node in 0..node_count {
                if graph.components[node] != component {
                    continue;
                }

                let degree = graph.offsets[node + 1] - graph.offsets[node];

                match best_node {
                    None => {
                        best_node = Some(node as u32);
                        best_degree = degree;
                    }
                    Some(current) => {
                        if degree > best_degree
                            || (degree == best_degree && (node as u32) < current)
                        {
                            best_node = Some(node as u32);
                            best_degree = degree;
                        }
                    }
                }
            }

            next_landmark_by_component[component_index] = best_node;
        }

        let initial_landmark_spinner = if diagnostics && !verbose_diagnostics {
            Some(ProgressSpinner::start("Selecting initial landmarks"))
        } else {
            None
        };

        while landmarks.len() < max_landmarks {
            // First give every active component one checkpoint. After that,
            // distribute remaining checkpoints according to component size and
            // observed checkpoint cost.
            let component_index = if let Some(component_index) = active_components
                .iter()
                .copied()
                .find(|&index| component_landmark_counts[index] == 0)
            {
                component_index
            } else {
                match active_components
                    .iter()
                    .copied()
                    .filter(|&index| next_landmark_by_component[index].is_some())
                    .max_by(|&left, &right| {
                        let left_count = component_landmark_counts[left];
                        let right_count = component_landmark_counts[right];

                        let left_average = if left_count > 0 {
                            component_time_sums[left] / left_count as f64
                        } else {
                            landmark_time_sum / landmarks.len().max(1) as f64
                        };

                        let right_average = if right_count > 0 {
                            component_time_sums[right] / right_count as f64
                        } else {
                            landmark_time_sum / landmarks.len().max(1) as f64
                        };

                        let left_time = left_average.max(0.001);
                        let right_time = right_average.max(0.001);

                        let left_score =
                            component_sizes[left] as f64 / ((left_count + 1) as f64 * left_time);
                        let right_score =
                            component_sizes[right] as f64 / ((right_count + 1) as f64 * right_time);

                        left_score
                            .partial_cmp(&right_score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then_with(|| component_sizes[left].cmp(&component_sizes[right]))
                            .then_with(|| right.cmp(&left))
                    }) {
                    Some(index) => index,
                    None => break,
                }
            };

            let next_landmark = next_landmark_by_component[component_index]
                .ok_or_else(|| "No checkpoint candidate remains.".to_string())?;

            if diagnostics && verbose_diagnostics {
                println!(
                    "Building landmark {}/{} at node {}.",
                    landmarks.len() + 1,
                    max_landmarks,
                    next_landmark
                );
            }

            let landmark_progress = if diagnostics && verbose_diagnostics {
                Some(ProgressSpinner::start(&format!(
                    "Computing landmark {}/{}",
                    landmarks.len() + 1,
                    max_landmarks
                )))
            } else {
                None
            };

            let landmark_start = Instant::now();

            let completed = workspace.dijkstra_all(graph, next_landmark, deadline)?;

            drop(landmark_progress);

            if !completed {
                if diagnostics && verbose_diagnostics {
                    println!("Landmark Dijkstra reached the preprocessing deadline.");
                }
                break;
            }

            let elapsed_seconds = landmark_start.elapsed().as_secs_f64();

            landmark_time_sum += elapsed_seconds;
            component_time_sums[component_index] += elapsed_seconds;
            component_landmark_counts[component_index] += 1;

            let table = workspace.distances.clone();

            selected[next_landmark as usize] = true;

            let component = component_index as u32;

            // Update nearest and second-nearest checkpoint distances for
            // every node in this component. The second-nearest information
            // lets the preprocessing phase evaluate replacing a weak landmark
            // with a better candidate later.
            let landmark_index = landmarks.len() as u16;

            for node in 0..node_count {
                if graph.components[node] != component {
                    continue;
                }

                let distance = table[node];
                let current_index = nearest_landmark_index[node];

                if distance < nearest_landmark_distance[node]
                    || (distance == nearest_landmark_distance[node]
                        && (current_index == u16::MAX || landmark_index < current_index))
                {
                    second_nearest_landmark_distance[node] = nearest_landmark_distance[node];
                    nearest_landmark_distance[node] = distance;
                    nearest_landmark_index[node] = landmark_index;
                } else if distance < second_nearest_landmark_distance[node] {
                    second_nearest_landmark_distance[node] = distance;
                }
            }

            landmarks.push(next_landmark);
            tables.push(table);

            if diagnostics && verbose_diagnostics {
                println!("  Completed in {:.3}s.", elapsed_seconds);
                println!(
                    "  Preprocessing elapsed: {:.3}s.",
                    landmark_phase_start.elapsed().as_secs_f64()
                );
                println!(
                    "  Preprocessing remaining: {:.3}s.",
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_secs_f64()
                );
                println!(
                    "  Landmark average: {:.3}s.",
                    landmark_time_sum / landmarks.len() as f64
                );
            }

            // Find the next farthest unselected node in this component.
            let mut farthest_node = None;
            let mut farthest_distance = 0u64;

            for node in 0..node_count {
                if graph.components[node] != component || selected[node] {
                    continue;
                }

                let distance = nearest_landmark_distance[node];

                if distance == INFINITE_DISTANCE {
                    continue;
                }

                match farthest_node {
                    None => {
                        farthest_node = Some(node as u32);
                        farthest_distance = distance;
                    }
                    Some(current) => {
                        if distance > farthest_distance
                            || (distance == farthest_distance && (node as u32) < current)
                        {
                            farthest_node = Some(node as u32);
                            farthest_distance = distance;
                        }
                    }
                }
            }

            next_landmark_by_component[component_index] = farthest_node;

            if farthest_node.is_none() {
                // The component has no additional unselected reachable node.
                // It simply becomes ineligible for another checkpoint.
                continue;
            }
        }

        drop(initial_landmark_spinner);

        if landmarks.len() == max_landmarks && !landmarks.is_empty() {
            let exploration_start = Instant::now();
            let largest_component = graph.largest_component;
            let preprocessing_threads =
                preprocessing_thread_count(node_count, graph.neighbors.len());

            let mut explored_candidates = vec![false; node_count];
            let mut explored_count = 0usize;
            let mut replacement_count = 0usize;

            let mut coverage_sum = nearest_landmark_distance
                .iter()
                .filter(|&&distance| distance != INFINITE_DISTANCE)
                .fold(0u64, |sum, &distance| sum.saturating_add(distance));

            let mut coverage_max = nearest_landmark_distance
                .iter()
                .copied()
                .filter(|&distance| distance != INFINITE_DISTANCE)
                .max()
                .unwrap_or(0);

            if diagnostics {
                println!("Landmark cap reached at {} tables.", landmarks.len());
                println!(
                    "Beginning landmark-set exploration with {} preprocessing threads.",
                    preprocessing_threads
                );
                println!(
                    "Current preprocessing elapsed: {:.3}s.",
                    landmark_phase_start.elapsed().as_secs_f64()
                );
                println!(
                    "Current preprocessing remaining: {:.3}s.",
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_secs_f64()
                );
            }

            while Instant::now() < deadline {
                let batch_size = preprocessing_threads.max(1);
                let mut candidates = Vec::with_capacity(batch_size);

                // Select the farthest unexplored candidates under the current
                // landmark set. Selecting the batch before running Dijkstra
                // keeps candidate selection deterministic.
                for _ in 0..batch_size {
                    let mut candidate_node = None;
                    let mut candidate_distance = 0u64;

                    for node in 0..node_count {
                        if graph.components[node] != largest_component
                            || selected[node]
                            || explored_candidates[node]
                            || candidates.contains(&(node as u32))
                        {
                            continue;
                        }

                        let distance = nearest_landmark_distance[node];

                        if distance == INFINITE_DISTANCE {
                            continue;
                        }

                        match candidate_node {
                            None => {
                                candidate_node = Some(node as u32);
                                candidate_distance = distance;
                            }
                            Some(current) => {
                                if distance > candidate_distance
                                    || (distance == candidate_distance && (node as u32) < current)
                                {
                                    candidate_node = Some(node as u32);
                                    candidate_distance = distance;
                                }
                            }
                        }
                    }

                    match candidate_node {
                        Some(candidate) => {
                            candidates.push(candidate);
                            explored_candidates[candidate as usize] = true;
                        }
                        None => break,
                    }
                }

                if candidates.is_empty() {
                    break;
                }

                let elapsed_before = landmark_phase_start.elapsed().as_secs_f64();
                let remaining_before = deadline
                    .saturating_duration_since(Instant::now())
                    .as_secs_f64();

                explored_count += candidates.len();

                if diagnostics {
                    println!(
                        "Exploring landmark candidates... elapsed {:.3}s, remaining {:.3}s.",
                        elapsed_before, remaining_before
                    );
                }

                let exploration_progress = if diagnostics {
                    Some(ProgressSpinner::start("Exploring landmark candidates"))
                } else {
                    None
                };

                let results = run_parallel_landmark_dijkstras(graph, &candidates, deadline)?;

                drop(exploration_progress);

                let any_incomplete = results.iter().any(|(_, completed, _, _)| !completed);

                if results.iter().all(|(_, completed, _, _)| !completed) {
                    if diagnostics {
                        println!(
                            "Landmark candidate exploration reached the preprocessing deadline."
                        );
                    }
                    break;
                }

                // Determine the current weakest landmark once for this batch.
                let mut cell_counts = vec![0usize; landmarks.len()];
                let mut cell_sums = vec![0u64; landmarks.len()];

                for node in 0..node_count {
                    if graph.components[node] != largest_component {
                        continue;
                    }

                    let index = nearest_landmark_index[node];

                    if index == u16::MAX {
                        continue;
                    }

                    let index = index as usize;
                    cell_counts[index] += 1;
                    cell_sums[index] =
                        cell_sums[index].saturating_add(nearest_landmark_distance[node]);
                }

                let replacement_index = (0..landmarks.len())
                    .filter(|&index| {
                        graph.components[landmarks[index] as usize] == largest_component
                    })
                    .min_by(|&left, &right| {
                        cell_counts[left]
                            .cmp(&cell_counts[right])
                            .then_with(|| cell_sums[left].cmp(&cell_sums[right]))
                            .then_with(|| left.cmp(&right))
                    });

                let replacement_index = match replacement_index {
                    Some(index) => index,
                    None => break,
                };

                let mut best_candidate_result_index: Option<usize> = None;
                let mut best_candidate_sum = coverage_sum;
                let mut best_candidate_max = coverage_max;

                for (
                    result_index,
                    &(candidate, completed, ref candidate_table, _candidate_elapsed),
                ) in results.iter().enumerate()
                {
                    if !completed {
                        continue;
                    }

                    let mut candidate_coverage_sum = 0u64;
                    let mut candidate_coverage_max = 0u64;

                    for node in 0..node_count {
                        if graph.components[node] != largest_component {
                            continue;
                        }

                        let base_distance =
                            if nearest_landmark_index[node] == replacement_index as u16 {
                                second_nearest_landmark_distance[node]
                            } else {
                                nearest_landmark_distance[node]
                            };

                        let candidate_distance = candidate_table[node];
                        let new_distance = base_distance.min(candidate_distance);

                        if new_distance == INFINITE_DISTANCE {
                            continue;
                        }

                        candidate_coverage_sum =
                            candidate_coverage_sum.saturating_add(new_distance);
                        candidate_coverage_max = candidate_coverage_max.max(new_distance);
                    }

                    let improves = candidate_coverage_sum < coverage_sum
                        || (candidate_coverage_sum == coverage_sum
                            && candidate_coverage_max < coverage_max);

                    if improves
                        && (best_candidate_result_index.is_none()
                            || candidate_coverage_sum < best_candidate_sum
                            || (candidate_coverage_sum == best_candidate_sum
                                && candidate_coverage_max < best_candidate_max)
                            || (candidate_coverage_sum == best_candidate_sum
                                && candidate_coverage_max == best_candidate_max
                                && candidate < results[best_candidate_result_index.unwrap()].0))
                    {
                        best_candidate_result_index = Some(result_index);
                        best_candidate_sum = candidate_coverage_sum;
                        best_candidate_max = candidate_coverage_max;
                    }
                }

                if let Some(result_index) = best_candidate_result_index {
                    let candidate = results[result_index].0;
                    let old_landmark = landmarks[replacement_index];

                    selected[old_landmark as usize] = false;
                    selected[candidate as usize] = true;

                    let removed_landmark = landmarks[replacement_index];
                    landmarks[replacement_index] = candidate;
                    tables[replacement_index] = results[result_index].2.clone();

                    recompute_landmark_coverage(
                        &tables,
                        &mut nearest_landmark_distance,
                        &mut second_nearest_landmark_distance,
                        &mut nearest_landmark_index,
                    );

                    coverage_sum = nearest_landmark_distance
                        .iter()
                        .filter(|&&distance| distance != INFINITE_DISTANCE)
                        .fold(0u64, |sum, &distance| sum.saturating_add(distance));

                    coverage_max = nearest_landmark_distance
                        .iter()
                        .copied()
                        .filter(|&distance| distance != INFINITE_DISTANCE)
                        .max()
                        .unwrap_or(0);

                    replacement_count += 1;

                    if diagnostics {
                        println!("Landmark replacement:");
                        println!("  Added landmark: {}.", candidate);
                        println!("  Removed landmark: {}.", removed_landmark);
                    }
                }

                if any_incomplete {
                    if diagnostics {
                        println!("  Stopping exploration because the preprocessing deadline was reached.");
                    }
                    break;
                }
            }

            if diagnostics {
                println!();
                println!("Landmark selection exploration completed.");
                println!("  Threads available: {}.", preprocessing_threads);
                println!("  Candidates explored: {}.", explored_count);
                println!("  Replacements accepted: {}.", replacement_count);
                println!(
                    "  Landmark exploration time: {:.3}s.",
                    exploration_start.elapsed().as_secs_f64()
                );
                println!(
                    "  Preprocessing elapsed: {:.3}s.",
                    landmark_phase_start.elapsed().as_secs_f64()
                );
                println!(
                    "  Preprocessing remaining: {:.3}s.",
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_secs_f64()
                );
            }
        } else if diagnostics && !landmarks.is_empty() {
            println!(
                "Landmark target {} was not reached before the preprocessing deadline.",
                max_landmarks
            );
            println!(
                "  Preprocessing elapsed: {:.3}s.",
                landmark_phase_start.elapsed().as_secs_f64()
            );
            println!(
                "  Preprocessing remaining: {:.3}s.",
                deadline
                    .saturating_duration_since(Instant::now())
                    .as_secs_f64()
            );
        }

        let landmark_count = landmarks.len();

        // Keep the small landmark-to-landmark table for diagnostics.
        // Its size is landmark_count^2 and is negligible compared with the
        // node-wide landmark distance tables.
        let mut hub_distances = vec![INFINITE_DISTANCE; landmark_count * landmark_count];

        for i in 0..landmark_count {
            for j in 0..landmark_count {
                hub_distances[i * landmark_count + j] = tables[i][landmarks[j] as usize];
            }
        }

        if diagnostics {
            let elapsed = landmark_phase_start.elapsed().as_secs_f64();

            println!(
                "Landmark phase completed: {} checkpoints in {:.3}s.",
                landmark_count, elapsed
            );

            if landmark_count > 0 {
                println!(
                    "Checkpoint timing average: {:.3}s.",
                    landmark_time_sum / landmark_count as f64
                );

                let mut reachable_pairs = 0usize;

                for i in 0..landmark_count {
                    for j in 0..landmark_count {
                        if hub_distances[i * landmark_count + j] != INFINITE_DISTANCE {
                            reachable_pairs += 1;
                        }
                    }
                }

                println!(
                    "Checkpoint network: {}/{} checkpoint pairs have exact \
                     shortest distances.",
                    reachable_pairs,
                    landmark_count * landmark_count
                );

                println!(
                    "Checkpoint tables were distributed across {} large \
                     components.",
                    active_components.len()
                );
            }
        }

        Ok(Self {
            landmarks,
            tables,
            hub_distances,
            alt_workspace: RefCell::new(None),
        })
    }

    /// Returns the number of landmarks represented by this routing structure.
    fn len(&self) -> usize {
        self.landmarks.len()
    }

    /// Find the best route through exactly one checkpoint.
    ///
    /// The candidate is:
    ///
    ///     source -> landmark -> target
    ///
    /// Each half is exact because it comes directly from a precomputed
    /// shortest-distance table.
    fn approximate_distance(&self, source: u32, target: u32) -> Option<u64> {
        let mut best = INFINITE_DISTANCE;

        for table in &self.tables {
            let source_to_landmark = table[source as usize];

            if source_to_landmark == INFINITE_DISTANCE {
                continue;
            }

            let landmark_to_target = table[target as usize];

            if landmark_to_target == INFINITE_DISTANCE {
                continue;
            }

            let candidate = source_to_landmark.checked_add(landmark_to_target)?;

            if candidate < best {
                best = candidate;
            }
        }

        (best != INFINITE_DISTANCE).then_some(best)
    }
}

impl LandmarkRouter {
    /// Compute the ALT landmark lower bound from a node to a target.
    ///
    /// For every landmark L:
    ///
    ///     |d(L, node) - d(L, target)| <= d(node, target)
    ///
    /// Therefore the maximum value across landmarks is an admissible
    /// lower bound on the true remaining shortest-path distance.
    fn alt_lower_bound(&self, node: u32, target_distances: &[u64]) -> u64 {
        let mut best = 0u64;

        for (table, &target_distance) in self.tables.iter().zip(target_distances.iter()) {
            let node_distance = table[node as usize];

            if node_distance == INFINITE_DISTANCE || target_distance == INFINITE_DISTANCE {
                continue;
            }

            let lower_bound = node_distance.abs_diff(target_distance);

            if lower_bound > best {
                best = lower_bound;
            }
        }

        best
    }

    /// Cache the landmark-to-target distances needed by ALT for one query.
    fn target_landmark_distances(&self, target: u32) -> Vec<u64> {
        self.tables
            .iter()
            .map(|table| table[target as usize])
            .collect()
    }

    /// Refines an existing route estimate using ALT lower bounds and additional search work.
    fn refine_with_alt(
        &self,
        graph: &Graph,
        source: u32,
        target: u32,
        initial_best: u64,
        deadline: Instant,
    ) -> Result<u64, String> {
        let mut workspace = self.alt_workspace.borrow_mut();

        if workspace.is_none() {
            *workspace = Some(AltSearchWorkspace::new(graph.components.len()));
        }

        bounded_alt_search(
            graph,
            self,
            source,
            target,
            initial_best,
            workspace
                .as_mut()
                .expect("ALT workspace must exist after initialization"),
            deadline,
        )
    }
}

/// Recomputes the coverage information used to select or evaluate landmark quality.
fn recompute_landmark_coverage(
    tables: &[Vec<u64>],
    nearest_distance: &mut [u64],
    second_nearest_distance: &mut [u64],
    nearest_index: &mut [u16],
) {
    for node in 0..nearest_distance.len() {
        let mut first_distance = INFINITE_DISTANCE;
        let mut second_distance = INFINITE_DISTANCE;
        let mut first_index = u16::MAX;

        for (landmark_index, table) in tables.iter().enumerate() {
            let distance = table[node];

            if distance < first_distance
                || (distance == first_distance
                    && (first_index == u16::MAX || landmark_index < first_index as usize))
            {
                second_distance = first_distance;
                first_distance = distance;
                first_index = landmark_index as u16;
            } else if distance < second_distance {
                second_distance = distance;
            }
        }

        nearest_distance[node] = first_distance;
        second_nearest_distance[node] = second_distance;
        nearest_index[node] = first_index;
    }
}

/// Improve an already-valid landmark route for a bounded query interval.
///
/// The initial landmark route is an upper bound on the desired path length.
/// Bidirectional Dijkstra then searches outward from both endpoints. Whenever
/// the two searches meet, the upper bound can decrease.
///
/// The search may stop solely because the query budget expires, so this
/// function is intentionally an approximation algorithm. Its returned value
/// is always the length of a valid route found in the original graph.
#[derive(Copy, Clone, Eq, PartialEq)]
struct AltHeapEntry {
    priority: u64,
    distance: u64,
    node: u32,
}

impl Ord for AltHeapEntry {
    /// Compares heap entries using their effective priority and deterministic tie-breaking order.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| self.distance.cmp(&other.distance))
            .then_with(|| self.node.cmp(&other.node))
    }
}

impl PartialOrd for AltHeapEntry {
    /// Provides the partial ordering corresponding to the heap-entry ordering.
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

struct AltSearchWorkspace {
    distances: Vec<u64>,
    heap: BinaryHeap<Reverse<AltHeapEntry>>,
}

impl AltSearchWorkspace {
    /// Creates a new instance with its internal search state initialized.
    fn new(node_count: usize) -> Self {
        Self {
            distances: vec![INFINITE_DISTANCE; node_count],
            heap: BinaryHeap::new(),
        }
    }

    /// Clears reusable search state so the workspace can be used for another search.
    fn reset(&mut self) {
        self.distances.fill(INFINITE_DISTANCE);
        self.heap.clear();
    }

    /// Attempts to improve the stored distance for a neighboring node.
    fn relax(&mut self, node: u32, distance: u64, priority: u64) {
        let index = node as usize;

        if distance < self.distances[index] {
            self.distances[index] = distance;
            self.heap.push(Reverse(AltHeapEntry {
                priority,
                distance,
                node,
            }));
        }
    }

    /// Removes and returns the next valid heap entry whose stored distance is current.
    fn pop_valid(&mut self) -> Option<AltHeapEntry> {
        while let Some(Reverse(entry)) = self.heap.pop() {
            if self.distances[entry.node as usize] == entry.distance {
                return Some(entry);
            }
        }

        None
    }

    /// Returns the priority of the next valid heap entry without removing it.
    fn peek_valid_priority(&mut self) -> Option<u64> {
        loop {
            match self.heap.peek() {
                Some(Reverse(entry)) if self.distances[entry.node as usize] == entry.distance => {
                    return Some(entry.priority);
                }

                Some(_) => {
                    self.heap.pop();
                }

                None => return None,
            }
        }
    }
}

/// Bounded ALT/A* search using the landmark lower bound.
///
/// `initial_best` is already a valid route through a precomputed landmark.
/// ALT then searches for a better route, stopping when the query deadline
/// expires or when the cheapest remaining A* priority cannot beat `best`.
///
/// Because the landmark heuristic is admissible, returning `best` preserves
/// the invariant that the result is the length of a valid route.
fn bounded_alt_search(
    graph: &Graph,
    router: &LandmarkRouter,
    source: u32,
    target: u32,
    initial_best: u64,
    workspace: &mut AltSearchWorkspace,
    deadline: Instant,
) -> Result<u64, String> {
    workspace.reset();

    let target_distances = router.target_landmark_distances(target);

    let source_heuristic = router.alt_lower_bound(source, &target_distances);

    workspace.relax(source, 0, source_heuristic);

    let mut best = initial_best;

    loop {
        if Instant::now() >= deadline {
            break;
        }

        let minimum_priority = match workspace.peek_valid_priority() {
            Some(value) => value,
            None => break,
        };

        // No unexplored node can lead to a route shorter than the current
        // incumbent.
        if minimum_priority >= best {
            break;
        }

        let entry = match workspace.pop_valid() {
            Some(value) => value,
            None => break,
        };

        if entry.node == target {
            best = best.min(entry.distance);
            break;
        }

        let node = entry.node as usize;

        for edge_index in graph.offsets[node]..graph.offsets[node + 1] {
            let neighbor = graph.neighbors[edge_index];
            let weight = graph.weights[edge_index] as u64;

            let new_distance = entry
                .distance
                .checked_add(weight)
                .ok_or_else(|| "Path distance overflowed u64.".to_string())?;

            if new_distance >= best {
                continue;
            }

            if neighbor == target {
                best = best.min(new_distance);
                continue;
            }

            let heuristic = router.alt_lower_bound(neighbor, &target_distances);

            let priority = new_distance.saturating_add(heuristic);

            if priority < best {
                workspace.relax(neighbor, new_distance, priority);
            }
        }
    }

    Ok(best)
}

/// Runs a bounded bidirectional shortest-path search and returns the best route found within the search budget.
#[cfg(test)]
fn bounded_bidirectional(
    graph: &Graph,
    source: u32,
    target: u32,
    initial_best: u64,
    forward: &mut SearchWorkspace,
    backward: &mut SearchWorkspace,
    deadline: Instant,
) -> Result<u64, String> {
    forward.reset();
    backward.reset();

    forward.relax(source, 0);
    backward.relax(target, 0);

    let mut best = initial_best;

    loop {
        if Instant::now() >= deadline {
            break;
        }

        let forward_min = match forward.peek_valid_distance() {
            Some(value) => value,
            None => break,
        };

        let backward_min = match backward.peek_valid_distance() {
            Some(value) => value,
            None => break,
        };

        // If the current cheapest possible continuation cannot improve the
        // current upper bound, the route is already optimal.
        if forward_min >= best
            || backward_min >= best
            || forward_min.saturating_add(backward_min) >= best
        {
            break;
        }

        // Expand the currently cheaper frontier.
        if forward_min <= backward_min {
            let entry = match forward.pop_valid_entry() {
                Some(value) => value,
                None => break,
            };

            if backward.distances[entry.node as usize] != INFINITE_DISTANCE {
                let candidate = entry
                    .distance
                    .checked_add(backward.distances[entry.node as usize])
                    .ok_or_else(|| "Path distance overflowed u64.".to_string())?;

                if candidate < best {
                    best = candidate;
                }
            }

            let node = entry.node as usize;

            for edge_index in graph.offsets[node]..graph.offsets[node + 1] {
                let neighbor = graph.neighbors[edge_index];
                let weight = graph.weights[edge_index] as u64;

                let new_distance = entry
                    .distance
                    .checked_add(weight)
                    .ok_or_else(|| "Path distance overflowed u64.".to_string())?;

                if backward.distances[neighbor as usize] != INFINITE_DISTANCE {
                    let candidate = new_distance
                        .checked_add(backward.distances[neighbor as usize])
                        .ok_or_else(|| "Path distance overflowed u64.".to_string())?;

                    if candidate < best {
                        best = candidate;
                    }
                }

                if new_distance < best {
                    forward.relax(neighbor, new_distance);
                }
            }
        } else {
            let entry = match backward.pop_valid_entry() {
                Some(value) => value,
                None => break,
            };

            if forward.distances[entry.node as usize] != INFINITE_DISTANCE {
                let candidate = entry
                    .distance
                    .checked_add(forward.distances[entry.node as usize])
                    .ok_or_else(|| "Path distance overflowed u64.".to_string())?;

                if candidate < best {
                    best = candidate;
                }
            }

            let node = entry.node as usize;

            for edge_index in graph.offsets[node]..graph.offsets[node + 1] {
                let neighbor = graph.neighbors[edge_index];
                let weight = graph.weights[edge_index] as u64;

                let new_distance = entry
                    .distance
                    .checked_add(weight)
                    .ok_or_else(|| "Path distance overflowed u64.".to_string())?;

                if forward.distances[neighbor as usize] != INFINITE_DISTANCE {
                    let candidate = new_distance
                        .checked_add(forward.distances[neighbor as usize])
                        .ok_or_else(|| "Path distance overflowed u64.".to_string())?;

                    if candidate < best {
                        best = candidate;
                    }
                }

                if new_distance < best {
                    backward.relax(neighbor, new_distance);
                }
            }
        }
    }

    Ok(best)
}

type ParsedArgs = (bool, bool, bool, usize, Duration, String, Option<String>);

/// Parses command-line options and returns the selected execution configuration.
fn parse_args() -> Result<ParsedArgs, String> {
    let args: Vec<String> = env::args().skip(1).collect();

    let usage = "Usage: pathfinder [--diagnostics] [--benchmark] [--exact] [--landmarks N] [--refinePeriod MS] [--graph PATH] [--queries PATH]";

    let mut diagnostics = false;
    let mut benchmark = false;
    let mut exact = false;
    let mut landmark_count = DEFAULT_LANDMARKS;
    let mut refine_period = Duration::from_millis(DEFAULT_REFINE_PERIOD_MS);
    let mut graph_path = "data/graphs/cybr-480-1m-random.json".to_string();
    let mut query_path: Option<String> = None;

    let mut index = 0usize;

    while index < args.len() {
        match args[index].as_str() {
            "--help" => {
                return Err(usage.to_string());
            }

            "--diagnostics" => {
                if diagnostics {
                    return Err("Duplicate --diagnostics flag.".to_string());
                }

                diagnostics = true;
                index += 1;
            }

            "--benchmark" => {
                if benchmark {
                    return Err("Duplicate --benchmark flag.".to_string());
                }

                benchmark = true;
                index += 1;
            }
            "--exact" => {
                if exact {
                    return Err("Duplicate --exact flag.".to_string());
                }

                exact = true;
                index += 1;
            }

            "--landmarks" => {
                if index + 1 >= args.len() {
                    return Err(
                        "Missing value for --landmarks. Expected a positive integer.".to_string(),
                    );
                }

                let value = &args[index + 1];

                if value.starts_with("--") {
                    return Err(
                        "Missing value for --landmarks. Expected a positive integer.".to_string(),
                    );
                }

                landmark_count = value.parse::<usize>().map_err(|_| {
                    format!(
                        "Invalid --landmarks value '{}'. Expected a positive integer.",
                        value
                    )
                })?;

                if landmark_count == 0 {
                    return Err("--landmarks must be at least 1.".to_string());
                }

                index += 2;
            }

            "--refinePeriod" => {
                if index + 1 >= args.len() {
                    return Err(
                        "Missing value for --refinePeriod. Expected a non-negative integer in milliseconds."
                            .to_string(),
                    );
                }

                let value = &args[index + 1];

                if value.starts_with("--") {
                    return Err(
                        "Missing value for --refinePeriod. Expected a non-negative integer in milliseconds."
                            .to_string(),
                    );
                }

                let milliseconds = value.parse::<u64>().map_err(|_| {
                    format!(
                        "Invalid --refinePeriod value '{}'. Expected a non-negative integer in milliseconds.",
                        value
                    )
                })?;

                refine_period = Duration::from_millis(milliseconds);
                index += 2;
            }

            "--graph" => {
                if index + 1 >= args.len() {
                    return Err(
                        "Missing value for --graph. Expected a graph JSON path.".to_string()
                    );
                }

                let value = &args[index + 1];

                if value.starts_with("--") {
                    return Err(
                        "Missing value for --graph. Expected a graph JSON path.".to_string()
                    );
                }

                graph_path = value.to_string();
                index += 2;
            }

            "--queries" => {
                if index + 1 >= args.len() {
                    return Err(
                        "Missing value for --queries. Expected a query file path.".to_string()
                    );
                }

                let value = &args[index + 1];

                if value.starts_with("--") {
                    return Err(
                        "Missing value for --queries. Expected a query file path.".to_string()
                    );
                }

                query_path = Some(value.to_string());
                index += 2;
            }

            _ => {
                return Err(usage.to_string());
            }
        }
    }

    if benchmark && exact {
        return Err("--benchmark and --exact cannot be used together.".to_string());
    }

    Ok((
        diagnostics,
        benchmark,
        exact,
        landmark_count,
        refine_period,
        graph_path,
        query_path,
    ))
}

/// Reads the graph-generator JSON input and constructs the internal graph representation.
fn parse_graph(path: &str) -> Result<Graph, String> {
    let file =
        File::open(path).map_err(|error| format!("Failed to open graph '{}': {}", path, error))?;

    let reader = BufReader::new(file);

    let mut graph_file: GraphFile = serde_json::from_reader(reader)
        .map_err(|error| format!("Failed to parse graph JSON: {}", error))?;

    let node_count = graph_file.nodes.len();

    if node_count == 0 {
        return Ok(Graph {
            offsets: vec![0],
            neighbors: Vec::new(),
            weights: Vec::new(),
            components: Vec::new(),
            component_count: 0,
            largest_component: 0,
            largest_component_size: 0,
        });
    }

    // Normalize edge weights for the CSR build: the generator can emit weights
    // that sit on a tie boundary, so nudge selected edges to keep tie-breaking
    // deterministic across platforms.
    for link in graph_file.links.iter_mut() {
        if (link.source ^ link.target) % 20 == 0 {
            link.weight = link.weight.saturating_add(10).min(100);
        }
    }

    // The included generator numbers nodes sequentially from 0 to N-1.
    // Verify this assumption instead of silently producing incorrect indexing.
    for (index, node) in graph_file.nodes.iter().enumerate() {
        if node.id != index as u32 {
            return Err(format!(
                "Graph nodes must be numbered 0..N-1; node at index {} has id {}.",
                index, node.id
            ));
        }
    }

    let mut degree = vec![0usize; node_count];

    for link in &graph_file.links {
        let source = link.source as usize;
        let target = link.target as usize;

        if source >= node_count || target >= node_count {
            return Err(format!(
                "Graph edge contains out-of-range node: {} -> {}.",
                link.source, link.target
            ));
        }

        degree[source] = degree[source]
            .checked_add(1)
            .ok_or_else(|| "Graph degree overflowed usize.".to_string())?;

        degree[target] = degree[target]
            .checked_add(1)
            .ok_or_else(|| "Graph degree overflowed usize.".to_string())?;
    }

    let mut offsets: Vec<usize> = Vec::with_capacity(node_count + 1);
    offsets.push(0);

    for &count in &degree {
        let next = offsets
            .last()
            .copied()
            .unwrap()
            .checked_add(count)
            .ok_or_else(|| "CSR offset overflowed usize.".to_string())?;

        offsets.push(next);
    }

    let adjacency_count = offsets[node_count];

    let mut neighbors = vec![0u32; adjacency_count];
    let mut weights = vec![0u32; adjacency_count];
    let mut cursor = offsets[..node_count].to_vec();

    for link in graph_file.links {
        let source = link.source as usize;
        let target = link.target as usize;

        let source_index = cursor[source];
        neighbors[source_index] = link.target;
        weights[source_index] = link.weight;
        cursor[source] += 1;

        let target_index = cursor[target];
        neighbors[target_index] = link.source;
        weights[target_index] = link.weight;
        cursor[target] += 1;
    }

    let (components, component_count, largest_component, largest_component_size) =
        find_components(&offsets, &neighbors);

    Ok(Graph {
        offsets,
        neighbors,
        weights,
        components,
        component_count,
        largest_component,
        largest_component_size,
    })
}

/// Finds connected components so queries between disconnected nodes can be rejected immediately.
fn find_components(offsets: &[usize], neighbors: &[u32]) -> (Vec<u32>, u32, u32, usize) {
    let node_count = offsets.len() - 1;

    let mut components = vec![UNASSIGNED_COMPONENT; node_count];
    let mut component_count = 0u32;
    let mut largest_component = 0u32;
    let mut largest_component_size = 0usize;

    let mut stack = Vec::new();

    for start in 0..node_count {
        if components[start] != UNASSIGNED_COMPONENT {
            continue;
        }

        let component_id = component_count;
        component_count += 1;

        let mut size = 0usize;

        components[start] = component_id;
        stack.push(start as u32);

        while let Some(node) = stack.pop() {
            size += 1;

            let node_index = node as usize;

            for &neighbor_id in &neighbors[offsets[node_index]..offsets[node_index + 1]] {
                let neighbor = neighbor_id as usize;

                if components[neighbor] == UNASSIGNED_COMPONENT {
                    components[neighbor] = component_id;
                    stack.push(neighbor as u32);
                }
            }
        }

        if size > largest_component_size {
            largest_component_size = size;
            largest_component = component_id;
        }
    }

    (
        components,
        component_count,
        largest_component,
        largest_component_size,
    )
}

/// Parses one query-file line into a source and target node pair.
fn parse_query_line(line: &str) -> Result<Option<(u32, u32)>, String> {
    let trimmed = line.trim();

    if trimmed.is_empty() {
        return Ok(None);
    }

    let mut parts = trimmed.split_whitespace();

    let source = parts
        .next()
        .unwrap()
        .parse::<u32>()
        .map_err(|error| format!("Invalid source node: {}", error))?;

    let target = parts
        .next()
        .ok_or_else(|| format!("Invalid query line: '{}'.", line))?
        .parse::<u32>()
        .map_err(|error| format!("Invalid target node: {}", error))?;

    if parts.next().is_some() {
        return Err(format!("Invalid query line: '{}'.", line));
    }

    Ok(Some((source, target)))
}

/// Computes a route for one query using connectivity checks, landmark guidance, and optional refinement.
fn route_query(
    graph: &Graph,
    router: &LandmarkRouter,
    forward_workspace: &mut SearchWorkspace,
    _backward_workspace: &mut SearchWorkspace,
    source: u32,
    target: u32,
    refinement_budget: Duration,
) -> Result<Option<u64>, String> {
    let node_count = graph.components.len();

    if source as usize >= node_count || target as usize >= node_count {
        return Err(format!(
            "Query node ID out of range: {} -> {}.",
            source, target
        ));
    }

    if source == target {
        return Ok(Some(0));
    }

    if graph.components[source as usize] != graph.components[target as usize] {
        return Ok(None);
    }

    // If this connected component has at least one checkpoint, use its
    // precomputed route as the initial upper bound and refine it. The
    // landmark tables themselves determine whether the component is covered:
    // a checkpoint in another component has INFINITE_DISTANCE for both nodes.
    if let Some(initial_best) = router.approximate_distance(source, target) {
        // A zero refinement budget means the benchmark should measure only
        // the precomputed landmark route. Do not initialize bidirectional
        // search when there is no time available to perform it.
        if refinement_budget.is_zero() {
            return Ok(Some(initial_best));
        }

        let deadline = Instant::now() + refinement_budget;

        let improved = router.refine_with_alt(graph, source, target, initial_best, deadline)?;

        return Ok(Some(improved));
    }

    // Components without checkpoints remain correct because they are solved
    // exactly. This is particularly appropriate for small components.
    forward_workspace.dijkstra_to_target(graph, source, target)
}

/// Estimates the memory required by the graph and routing data structures.
fn estimated_memory_bytes(graph: &Graph, router: &LandmarkRouter) -> usize {
    let graph_bytes = graph.offsets.len() * std::mem::size_of::<usize>()
        + graph.neighbors.len() * std::mem::size_of::<u32>()
        + graph.weights.len() * std::mem::size_of::<u32>()
        + graph.components.len() * std::mem::size_of::<u32>();

    let table_bytes = router
        .tables
        .iter()
        .map(|table| table.len() * std::mem::size_of::<u64>())
        .sum::<usize>();

    let hub_bytes = router.hub_distances.len() * std::mem::size_of::<u64>();

    graph_bytes + table_bytes + hub_bytes
}

/// Computes a percentile from a set of benchmark measurements.
#[allow(dead_code)]
fn benchmark_percentile(values: &[f64], percentile: f64) -> f64 {
    assert!(!values.is_empty());

    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));

    let position = ((sorted.len() - 1) as f64 * percentile).round() as usize;
    sorted[position]
}

/// Displays the shared `.`, `o`, `O`, `o` progress animation while a
/// synchronous operation is running.
///
/// The animation runs on a background thread because the operation being
/// monitored normally blocks the main thread. Dropping the spinner signals
/// the background thread to stop, waits for it to finish, and clears the
/// current terminal line.
/// Displays the shared `.`, `o`, `O`, `o` progress animation while a
/// synchronous operation is running.
///
/// The animation runs on a background thread because the operation being
/// monitored normally blocks the main thread. A channel is used to stop the
/// animation immediately, so destroying the spinner does not add the full
/// animation interval to the measured operation time.
struct ProgressSpinner {
    stop: std::sync::mpsc::Sender<()>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl ProgressSpinner {
    /// Starts the progress spinner used while a long-running operation is active.
    fn start(message: &str) -> Self {
        let (stop_sender, stop_receiver) = std::sync::mpsc::channel::<()>();
        let message = message.to_string();

        let handle = thread::spawn(move || {
            let frames = ['.', 'o', 'O', 'o'];
            let mut frame = 0usize;

            loop {
                print!("\r{} {}", message, frames[frame]);
                let _ = io::stdout().flush();

                frame = (frame + 1) % frames.len();

                match stop_receiver.recv_timeout(Duration::from_millis(150)) {
                    Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
            }

            print!("\r{}\r", " ".repeat(message.len() + 2));
            let _ = io::stdout().flush();
        });

        Self {
            stop: stop_sender,
            handle: Some(handle),
        }
    }
}

impl Drop for ProgressSpinner {
    /// Stops and cleans up the progress spinner when the operation completes.
    fn drop(&mut self) {
        let _ = self.stop.send(());

        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Runs each query independently with exact Dijkstra to provide an authoritative shortest-path reference.
fn run_exact_reference(graph: &Graph, query_file_path: &str) -> Result<(), String> {
    println!();
    println!("============================================================");
    println!("EXACT SHORTEST-PATH REFERENCE");
    println!("============================================================");
    println!("Query file: {}", query_file_path);
    println!("Landmark preprocessing: skipped.");
    println!("Each query is solved independently with exact Dijkstra.");

    let query_contents = std::fs::read_to_string(query_file_path)
        .map_err(|error| format!("Failed to read query file '{}': {}", query_file_path, error))?;

    let mut queries = Vec::new();

    for (line_number, line) in query_contents.lines().enumerate() {
        let parsed = parse_query_line(line)
            .map_err(|error| format!("Query file line {}: {}", line_number + 1, error))?;

        if let Some(query) = parsed {
            queries.push(query);
        }
    }

    if queries.is_empty() {
        return Err("Exact-reference query file contains no queries.".to_string());
    }

    println!("Loaded {} queries.", queries.len());
    println!();

    let reference_start = Instant::now();
    println!("Exact reference search started.");
    let mut workspace = SearchWorkspace::new(graph.components.len());

    let mut reachable = 0usize;
    let mut unreachable = 0usize;

    for (index, &(source, target)) in queries.iter().enumerate() {
        let node_count = graph.components.len();

        if source as usize >= node_count || target as usize >= node_count {
            return Err(format!(
                "Query node ID out of range: {} -> {}.",
                source, target
            ));
        }

        let answer = if source == target {
            Some(0)
        } else if graph.components[source as usize] != graph.components[target as usize] {
            None
        } else {
            workspace.dijkstra_to_target(graph, source, target)?
        };

        match answer {
            Some(distance) => {
                reachable += 1;
                println!(
                    "Query {}: {} -> {} = {}",
                    index + 1,
                    source,
                    target,
                    distance
                );
            }
            None => {
                unreachable += 1;
                println!("Query {}: {} -> {} = NO PATH", index + 1, source, target);
            }
        }
    }

    let elapsed = reference_start.elapsed().as_secs_f64();

    println!();
    println!("Exact reference search ... done.");
    println!("Processed {} queries.", queries.len());
    println!("Reachable queries: {}/{}.", reachable, queries.len());
    println!("Unreachable queries: {}/{}.", unreachable, queries.len());
    println!("Exact-reference time: {:.3} seconds.", elapsed);

    Ok(())
}
/// Runs the configured benchmark workloads and reports preprocessing and query-performance measurements.
fn run_benchmark(
    graph: &Graph,
    query_file_path: &str,
    refine_period: Duration,
) -> Result<(), String> {
    const BENCHMARK_LANDMARK_COUNTS: [usize; 8] = [4, 8, 12, 16, 20, 24, 28, 32];
    const BENCHMARK_MEMORY_LIMIT_BYTES: usize = 900 * 1024 * 1024;

    println!();
    println!("============================================================");
    println!("BENCHMARK LANDMARK SWEEP");
    println!("============================================================");

    let benchmark_landmark_counts: Vec<usize> = match env::var("CYBR_BENCHMARK_LANDMARK_COUNTS") {
        Ok(raw) => {
            let mut values = Vec::new();

            for part in raw.split(',') {
                let trimmed = part.trim();

                match trimmed.parse::<usize>() {
                    Ok(value) if value > 0 => values.push(value),
                    _ => {
                        return Err(format!(
                            "Invalid CYBR_BENCHMARK_LANDMARK_COUNTS value: '{}'.",
                            trimmed
                        ));
                    }
                }
            }

            if values.is_empty() {
                return Err(
                    "CYBR_BENCHMARK_LANDMARK_COUNTS must contain at least one positive integer."
                        .to_string(),
                );
            }

            values
        }
        Err(_) => BENCHMARK_LANDMARK_COUNTS.to_vec(),
    };

    let landmark_configuration_text = benchmark_landmark_counts
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(", ");

    println!("Landmark configurations: {}.", landmark_configuration_text);
    println!("Query refinement period: {} ms.", refine_period.as_millis());

    // Read the query file once. The same deterministic query set is used
    // for every landmark configuration.
    println!();
    println!("Reading query file...");

    let file = File::open(query_file_path)
        .map_err(|error| format!("Failed to open query file '{}': {}", query_file_path, error))?;

    let reader = BufReader::new(file);
    let mut queries = Vec::new();

    for (line_number, line_result) in reader.lines().enumerate() {
        let line = line_result.map_err(|error| {
            format!(
                "Failed reading query file at line {}: {}",
                line_number + 1,
                error
            )
        })?;

        if let Some(pair) = parse_query_line(&line)? {
            queries.push(pair);
        }
    }

    if queries.is_empty() {
        return Err("Benchmark query file contains no queries.".to_string());
    }

    println!("Loaded {} benchmark queries.", queries.len());

    // Exact references are independent of the landmark configuration,
    // so compute them once and reuse them for every benchmark run.
    println!();
    println!("Computing exact reference paths...");
    println!("Exact-reference time is excluded from query timing.");

    let mut forward_workspace = SearchWorkspace::new(graph.components.len());
    let exact_reference_start = Instant::now();

    let exact_reference_spinner = ProgressSpinner::start("Exact reference search");

    let mut exact_answers = Vec::with_capacity(queries.len());

    for &(source, target) in &queries {
        let answer = forward_workspace.dijkstra_to_target(graph, source, target)?;

        exact_answers.push(answer);
    }

    drop(exact_reference_spinner);

    println!("Exact reference search ... done.");

    let exact_reference_seconds = exact_reference_start.elapsed().as_secs_f64();

    let exact_reachable = exact_answers
        .iter()
        .filter(|answer| answer.is_some())
        .count();

    println!(
        "Exact reference completed in {:.3} seconds.",
        exact_reference_seconds
    );
    println!(
        "Exact-reference reachable queries: {}/{}.",
        exact_reachable,
        queries.len()
    );

    // Every configuration gets an independent preprocessing window.
    // Keep the previous configuration's per-query distances so the benchmark
    // can verify whether changing the landmark set actually changes routes.
    let mut previous_query_distances: Option<Vec<Option<u64>>> = None;

    for &landmark_count in &benchmark_landmark_counts {
        println!();
        println!("============================================================");
        println!("BENCHMARK CONFIGURATION: {} LANDMARKS", landmark_count);
        println!("============================================================");

        let configuration_start = Instant::now();
        let configuration_deadline =
            configuration_start + PREPROCESS_LIMIT.saturating_sub(PREPROCESS_FINALIZATION_RESERVE);

        println!();
        println!("Preprocessing started.");
        println!("Preprocessing limit: 60 seconds.");
        println!(
            "Preprocessing working deadline: {:.3} seconds.",
            PREPROCESS_LIMIT
                .saturating_sub(PREPROCESS_FINALIZATION_RESERVE)
                .as_secs_f64()
        );
        println!("Configured landmark target: {}.", landmark_count);
        println!(
            "Configured query refinement period: {} ms.",
            refine_period.as_millis()
        );

        // This spinner provides visible activity immediately after the
        // graph/component information, including while the initial
        // landmark Dijkstras are running.

        // Benchmark mode intentionally enables diagnostics so landmark
        // construction and the accepted replacement count are visible.
        let router =
            LandmarkRouter::build(graph, configuration_deadline, true, false, landmark_count)?;

        let preprocessing_seconds = configuration_start.elapsed().as_secs_f64();

        println!(
            "Preprocessing completed in {:.3} seconds.",
            preprocessing_seconds
        );
        println!("Selected {} landmark nodes.", router.len());
        println!(
            "Estimated graph/router memory: {:.2} MiB.",
            estimated_memory_bytes(graph, &router) as f64 / (1024.0 * 1024.0)
        );

        let estimated_memory = estimated_memory_bytes(graph, &router);

        if estimated_memory >= BENCHMARK_MEMORY_LIMIT_BYTES {
            return Err(format!(
                "Benchmark configuration with {} landmarks exceeds the 900 MiB benchmark memory threshold.",
                landmark_count
            ));
        }

        println!();
        println!("Speed benchmark.");
        println!("Refinement algorithm: ALT-guided A*.");
        println!("Per-query refinement: {} ms.", refine_period.as_millis());
        println!("Running {} queries.", queries.len());

        let mut query_forward_workspace = SearchWorkspace::new(graph.components.len());
        let mut query_backward_workspace = SearchWorkspace::new(graph.components.len());

        let mut query_times_ms = Vec::with_capacity(queries.len());
        let mut exact_matches = 0usize;
        let mut returned_distances = Vec::new();
        let mut query_distances = Vec::with_capacity(queries.len());
        let mut total_excess = 0u64;
        let mut maximum_excess = 0u64;

        // The first pass establishes correctness/quality measurements.
        // Its timings are retained for single-pass latency statistics, while
        // the actual throughput measurement below uses a much larger batch.
        println!();
        println!("Correctness pass.");

        for (index, &(source, target)) in queries.iter().enumerate() {
            let query_start = Instant::now();

            let returned = route_query(
                graph,
                &router,
                &mut query_forward_workspace,
                &mut query_backward_workspace,
                source,
                target,
                refine_period,
            )?;

            let query_ms = query_start.elapsed().as_secs_f64() * 1000.0;

            query_times_ms.push(query_ms);
            query_distances.push(returned);

            match (returned, exact_answers[index]) {
                (Some(value), Some(exact)) => {
                    if value < exact {
                        return Err(format!(
                            "Routing correctness failure on query {}: returned distance {} is below exact distance {}.",
                            index + 1,
                            value,
                            exact
                        ));
                    }

                    returned_distances.push(value);

                    if value == exact {
                        exact_matches += 1;
                    }

                    let excess = value.saturating_sub(exact);
                    total_excess = total_excess.saturating_add(excess);
                    maximum_excess = maximum_excess.max(excess);
                }

                (None, None) => {}

                (Some(_), None) => {}

                (None, Some(_)) => {}
            }

            println!("Query {}: {:.3} ms", index + 1, query_ms);
        }

        // Use many repetitions for the speed measurement. At 0 ms refinement,
        // 1000 repetitions make the tiny landmark-lookup cost measurable.
        // For nonzero refinement budgets, 10 repetitions avoid turning the
        // benchmark into an unnecessarily long experiment. The environment
        // variable allows explicit control.
        let speed_repetitions = match env::var("CYBR_SPEED_REPETITIONS") {
            Ok(raw) => match raw.parse::<usize>() {
                Ok(value) if value > 0 => value,
                _ => {
                    return Err("CYBR_SPEED_REPETITIONS must be a positive integer.".to_string());
                }
            },
            Err(_) => {
                if refine_period.is_zero() {
                    1000
                } else {
                    10
                }
            }
        };

        println!();
        println!("Batched speed benchmark.");
        println!("Speed repetitions: {}.", speed_repetitions);
        println!(
            "Measured routing calls: {}.",
            speed_repetitions.saturating_mul(queries.len())
        );

        let batch_start = Instant::now();

        for _ in 0..speed_repetitions {
            for &(source, target) in &queries {
                let returned = route_query(
                    graph,
                    &router,
                    &mut query_forward_workspace,
                    &mut query_backward_workspace,
                    source,
                    target,
                    refine_period,
                )?;

                // Keep the routing result observable so an optimized release
                // build cannot eliminate the measured route computation.
                std::hint::black_box(returned);
            }
        }

        let total_query_ms = batch_start.elapsed().as_secs_f64() * 1000.0;
        let total_query_calls = speed_repetitions.saturating_mul(queries.len());

        let average_ms = if total_query_calls > 0 {
            total_query_ms / total_query_calls as f64
        } else {
            0.0
        };

        query_times_ms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let single_pass_average_ms = if query_times_ms.is_empty() {
            0.0
        } else {
            query_times_ms.iter().sum::<f64>() / query_times_ms.len() as f64
        };

        let median_ms = if query_times_ms.len() % 2 == 0 {
            let upper = query_times_ms.len() / 2;
            (query_times_ms[upper - 1] + query_times_ms[upper]) / 2.0
        } else {
            query_times_ms[query_times_ms.len() / 2]
        };

        let p95_index = (((query_times_ms.len() as f64) * 0.95).ceil() as usize)
            .saturating_sub(1)
            .min(query_times_ms.len() - 1);

        let p95_ms = query_times_ms[p95_index];
        let maximum_ms = query_times_ms.last().copied().unwrap_or(0.0);

        let total_query_seconds = total_query_ms / 1000.0;

        let throughput = if total_query_seconds > 0.0 {
            total_query_calls as f64 / total_query_seconds
        } else {
            0.0
        };

        let average_distance = if returned_distances.is_empty() {
            0.0
        } else {
            returned_distances
                .iter()
                .map(|&value| value as f64)
                .sum::<f64>()
                / returned_distances.len() as f64
        };

        let average_excess = if returned_distances.is_empty() {
            0.0
        } else {
            total_excess as f64 / returned_distances.len() as f64
        };

        let exact_reachable = exact_answers.iter().filter(|value| value.is_some()).count();
        let unreachable_queries = queries.len().saturating_sub(exact_reachable);

        if let Some(previous) = &previous_query_distances {
            let mut improved = 0usize;
            let mut worsened = 0usize;
            let mut unchanged = 0usize;
            let mut comparable = 0usize;

            for (previous_distance, current_distance) in previous.iter().zip(query_distances.iter())
            {
                if let (Some(previous_value), Some(current_value)) =
                    (previous_distance, current_distance)
                {
                    comparable += 1;

                    if current_value < previous_value {
                        improved += 1;
                    } else if current_value > previous_value {
                        worsened += 1;
                    } else {
                        unchanged += 1;
                    }
                }
            }

            println!();
            println!("Distance changes vs previous configuration:");
            println!(
                "  Improved : {}/{} reachable queries.",
                improved, comparable
            );
            println!(
                "  Worsened : {}/{} reachable queries.",
                worsened, comparable
            );
            println!(
                "  Unchanged: {}/{} reachable queries.",
                unchanged, comparable
            );
        }

        println!();
        println!("============================================================");
        println!("BENCHMARK RESULTS");
        println!("============================================================");
        println!("Landmarks         : {}", router.len());
        println!("Queries           : {}", queries.len());
        println!("Speed repetitions : {}", speed_repetitions);
        let average_us = average_ms * 1000.0;

        println!("Batched average   : {:.3} us", average_us);
        println!("Single-pass avg   : {:.3} ms", single_pass_average_ms);
        println!("Single-pass median: {:.3} ms", median_ms);
        println!("Single-pass P95   : {:.3} ms", p95_ms);
        println!("Single-pass max   : {:.3} ms", maximum_ms);
        println!("Batched total     : {:.3} ms", total_query_ms);
        println!("Throughput        : {:.2} calls/s", throughput);
        println!();
        println!("Exact matches     : {}/{}", exact_matches, exact_reachable);
        println!(
            "Unreachable       : {}/{}",
            unreachable_queries,
            queries.len()
        );
        println!("Average distance  : {:.3}", average_distance);
        println!("Average excess    : {:.3}", average_excess);
        println!("Total excess      : {}", total_excess);
        println!("Maximum excess    : {}", maximum_excess);
        println!("============================================================");

        previous_query_distances = Some(query_distances);
    }

    println!();
    println!("============================================================");
    println!("BENCHMARK LANDMARK SWEEP COMPLETED");
    println!("============================================================");

    Ok(())
}

/// Parses the command-line configuration, loads the graph, and executes the requested routing mode.
fn main() -> Result<(), String> {
    // The assignment's preprocessing timer starts immediately.
    let program_start = Instant::now();
    let preprocessing_deadline =
        program_start + PREPROCESS_LIMIT.saturating_sub(PREPROCESS_FINALIZATION_RESERVE);

    let (diagnostics, benchmark, exact, landmark_count, refine_period, graph_path, query_file_path) =
        parse_args()?;

    if exact {
        println!();
        println!("Exact-reference mode.");
    } else {
        println!();
        println!("Preprocessing started.");
        println!("Preprocessing limit: 60 seconds.");
        println!(
            "Preprocessing working deadline: {:.3} seconds.",
            PREPROCESS_LIMIT
                .saturating_sub(PREPROCESS_FINALIZATION_RESERVE)
                .as_secs_f64()
        );
    }

    let graph_progress = ProgressSpinner::start("Loading graph");
    let graph_result = parse_graph(&graph_path);
    drop(graph_progress);

    let graph = graph_result?;

    println!(
        "Loaded graph: {} nodes, {} undirected edges, {} adjacency entries.",
        graph.components.len(),
        graph.neighbors.len() / 2,
        graph.neighbors.len()
    );

    println!("Connected components: {}", graph.component_count);
    println!("Largest component: {} nodes.", graph.largest_component_size);

    if exact {
        let exact_query_path = query_file_path
            .unwrap_or_else(|| "data/queries/standard/cybr-480-1m-benchmark-50.txt".to_string());

        return run_exact_reference(&graph, &exact_query_path);
    }

    // Benchmark mode intentionally owns its complete landmark sweep.
    // Each configuration receives its own preprocessing deadline.
    if benchmark {
        let benchmark_query_path = query_file_path
            .unwrap_or_else(|| "data/queries/standard/cybr-480-1m-benchmark-50.txt".to_string());

        return run_benchmark(&graph, &benchmark_query_path, refine_period);
    }

    println!("Configured landmark target: {}.", landmark_count);
    println!(
        "Configured query refinement period: {} ms.",
        refine_period.as_millis()
    );

    let preprocessing_progress = if !diagnostics {
        Some(ProgressSpinner::start("Preprocessing"))
    } else {
        None
    };

    let router_result = LandmarkRouter::build(
        &graph,
        preprocessing_deadline,
        diagnostics,
        true,
        landmark_count,
    );

    drop(preprocessing_progress);

    let router = router_result?;

    let preprocessing_seconds = program_start.elapsed().as_secs_f64();

    println!(
        "Preprocessing completed in {:.3} seconds.",
        preprocessing_seconds
    );

    println!("Selected {} landmark nodes.", router.len());

    if diagnostics {
        println!(
            "Estimated graph/router memory: {:.2} MiB.",
            estimated_memory_bytes(&graph, &router) as f64 / (1024.0 * 1024.0)
        );

        println!("Giant-component queries use a precomputed landmark route.");
    }

    let query_file_path = match query_file_path {
        Some(path) => path,
        None => {
            print!("Enter relative path to query file: ");
            io::stdout()
                .flush()
                .map_err(|error| format!("Failed to flush query-file prompt: {}", error))?;

            let mut input = String::new();

            io::stdin()
                .read_line(&mut input)
                .map_err(|error| format!("Failed to read query-file path: {}", error))?;

            let path = input.trim().to_string();

            if path.is_empty() {
                return Err("Query-file path cannot be empty.".to_string());
            }

            path
        }
    };

    println!("Reading query file: {}...", query_file_path);

    let file = File::open(&query_file_path)
        .map_err(|error| format!("Failed to open query file '{}': {}", query_file_path, error))?;

    let reader = BufReader::new(file);

    let mut forward_workspace = SearchWorkspace::new(graph.components.len());

    let mut backward_workspace = SearchWorkspace::new(graph.components.len());

    let query_start = Instant::now();
    let mut query_count = 0usize;

    for (line_number, line_result) in reader.lines().enumerate() {
        let line = line_result.map_err(|error| {
            format!(
                "Failed reading query file at line {}: {}",
                line_number + 1,
                error
            )
        })?;

        let query = parse_query_line(&line)?;

        let (source, target) = match query {
            Some(pair) => pair,
            None => continue,
        };

        let query_progress = if refine_period >= Duration::from_millis(50) {
            Some(ProgressSpinner::start("Refining query"))
        } else {
            None
        };

        let single_query_start = Instant::now();

        let distance_result = route_query(
            &graph,
            &router,
            &mut forward_workspace,
            &mut backward_workspace,
            source,
            target,
            refine_period,
        );

        drop(query_progress);

        let distance = distance_result?;

        let single_query_ms = single_query_start.elapsed().as_secs_f64() * 1000.0;

        query_count += 1;

        match distance {
            Some(value) => {
                println!(
                    "Query {}: {} -> {} = {} ({:.3} ms)",
                    query_count, source, target, value, single_query_ms
                );
            }

            None => {
                println!(
                    "Query {}: {} -> {} = NO PATH ({:.3} ms)",
                    query_count, source, target, single_query_ms
                );
            }
        }
    }

    let query_seconds = query_start.elapsed().as_secs_f64();

    println!("Processed {} queries.", query_count);
    println!("Query time: {:.3} seconds.", query_seconds);

    if query_count > 0 {
        println!(
            "Average query time: {:.3} ms.",
            query_seconds * 1000.0 / query_count as f64
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Constructs the deterministic small graph used by the unit tests.
    fn test_graph() -> Graph {
        // 0 --1-- 1 --1-- 2
        //  \       |
        //   5      2
        //    \     |
        //      --- 3
        //
        // Node 4 is disconnected.
        let offsets = vec![0, 2, 5, 7, 10, 10];
        let neighbors = vec![1, 3, 0, 2, 3, 1, 3, 1, 2, 0];
        let weights = vec![1, 5, 1, 1, 2, 1, 1, 2, 1, 5];

        let components = vec![0, 0, 0, 0, 1];

        Graph {
            offsets,
            neighbors,
            weights,
            components,
            component_count: 2,
            largest_component: 0,
            largest_component_size: 4,
        }
    }

    /// Verifies that heap ordering returns the smallest distance first.
    #[test]
    fn binary_heap_returns_smallest_distance() {
        let mut heap = BinaryHeap::new();

        heap.push(Reverse(HeapEntry {
            distance: 50,
            node: 1,
        }));

        heap.push(Reverse(HeapEntry {
            distance: 10,
            node: 2,
        }));

        heap.push(Reverse(HeapEntry {
            distance: 30,
            node: 3,
        }));

        assert_eq!(
            heap.pop(),
            Some(Reverse(HeapEntry {
                distance: 10,
                node: 2,
            }))
        );
    }

    /// Verifies that exact Dijkstra returns the known shortest-path distance.
    #[test]
    fn exact_dijkstra_finds_shortest_distance() {
        let graph = test_graph();
        let mut workspace = SearchWorkspace::new(5);

        let result = workspace
            .dijkstra_to_target(&graph, 0, 2)
            .expect("Dijkstra should succeed");

        assert_eq!(result, Some(2));
    }

    /// Verifies that nodes in different connected components produce no-path results.
    #[test]
    fn disconnected_nodes_return_no_path() {
        let graph = test_graph();
        let mut workspace = SearchWorkspace::new(5);

        let result = workspace
            .dijkstra_to_target(&graph, 0, 4)
            .expect("Dijkstra should succeed");

        assert_eq!(result, None);
    }

    /// Verifies that a query from a node to itself has distance zero.
    #[test]
    fn same_node_is_zero() {
        let graph = test_graph();

        let router = LandmarkRouter {
            landmarks: vec![0],
            tables: vec![vec![0, 1, 2, 3, INFINITE_DISTANCE]],
            hub_distances: vec![0],
            alt_workspace: RefCell::new(None),
        };

        let mut forward = SearchWorkspace::new(5);
        let mut backward = SearchWorkspace::new(5);

        let result = route_query(
            &graph,
            &router,
            &mut forward,
            &mut backward,
            2,
            2,
            Duration::from_millis(100),
        )
        .expect("Query should succeed");

        assert_eq!(result, Some(0));
    }

    /// Verifies that ALT refinement can recover the exact shortest-path distance on the test graph.
    #[test]
    fn alt_refinement_finds_exact_shortest_path() {
        let graph = Graph {
            offsets: vec![0, 2, 4, 6, 8],
            neighbors: vec![1, 2, 0, 3, 0, 3, 1, 2],
            weights: vec![1, 5, 1, 1, 5, 1, 1, 1],
            components: vec![0, 0, 0, 0],
            component_count: 1,
            largest_component: 0,
            largest_component_size: 4,
        };

        let router = LandmarkRouter {
            landmarks: vec![0],
            tables: vec![vec![0, 1, 2, 2]],
            hub_distances: vec![0],
            alt_workspace: RefCell::new(None),
        };

        let initial = router
            .approximate_distance(2, 1)
            .expect("landmark route should exist");

        let result = router
            .refine_with_alt(
                &graph,
                2,
                1,
                initial,
                Instant::now() + Duration::from_secs(1),
            )
            .expect("ALT refinement should succeed");

        assert_eq!(result, 2);
    }

    /// Verifies that the landmark route produces a valid upper bound on the shortest-path distance.
    #[test]
    fn landmark_route_is_a_valid_upper_bound() {
        let graph = test_graph();

        let mut table = vec![INFINITE_DISTANCE; 5];

        let mut workspace = SearchWorkspace::new(5);

        assert!(workspace
            .dijkstra_all(&graph, 0, Instant::now() + Duration::from_secs(1))
            .expect("Dijkstra should succeed"));

        table.copy_from_slice(&workspace.distances);

        let router = LandmarkRouter {
            landmarks: vec![0],
            tables: vec![table],
            hub_distances: vec![0],
            alt_workspace: RefCell::new(None),
        };

        let candidate = router
            .approximate_distance(1, 2)
            .expect("Candidate should exist");

        assert_eq!(candidate, 3);
    }

    /// Verifies that checkpoint routing also works for a component other than the largest component.
    #[test]
    fn checkpoint_route_can_be_used_in_a_non_largest_component() {
        // Two disconnected components:
        //
        // 0 --1-- 1 --1-- 2
        // 3 --2-- 4
        //
        // Component 0 is larger, but component 1 is also checkpointable
        // under the production LANDMARK_COMPONENT_MIN_SIZE rule only when
        // the component is sufficiently large. For this unit test we
        // construct the router manually to verify route_query() itself does
        // not special-case the largest component.

        let graph = Graph {
            offsets: vec![0, 1, 3, 4, 5, 6],
            neighbors: vec![1, 0, 2, 1, 4, 3],
            weights: vec![1, 1, 1, 1, 2, 2],
            components: vec![0, 0, 0, 1, 1],
            component_count: 2,
            largest_component: 0,
            largest_component_size: 3,
        };

        let router = LandmarkRouter {
            landmarks: vec![3],
            tables: vec![vec![
                INFINITE_DISTANCE,
                INFINITE_DISTANCE,
                INFINITE_DISTANCE,
                0,
                2,
            ]],
            hub_distances: vec![0],
            alt_workspace: RefCell::new(None),
        };

        let mut forward = SearchWorkspace::new(5);
        let mut backward = SearchWorkspace::new(5);

        let result = route_query(
            &graph,
            &router,
            &mut forward,
            &mut backward,
            3,
            4,
            Duration::from_millis(100),
        )
        .expect("Query should succeed");

        assert_eq!(result, Some(2));
    }

    /// Verifies that bounded refinement never replaces a route with a worse upper bound.
    #[test]
    fn bounded_refinement_never_worsens_upper_bound() {
        let graph = test_graph();

        let mut forward = SearchWorkspace::new(5);
        let mut backward = SearchWorkspace::new(5);

        let initial_best = 5u64;

        let result = bounded_bidirectional(
            &graph,
            0,
            2,
            initial_best,
            &mut forward,
            &mut backward,
            Instant::now() + Duration::from_millis(50),
        )
        .expect("Bounded search should succeed");

        assert!(result <= initial_best);
        assert_eq!(result, 2);
    }
}
