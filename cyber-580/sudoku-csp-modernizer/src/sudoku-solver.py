import re
from typing import Dict, Optional


def cross(A, B) -> tuple:
    "Cross product of strings in A and strings in B."
    return tuple(a + b for a in A for b in B)

digits    = '123456789'
rows      = 'ABCDEFGHI'
cols      = digits
squares   = cross(rows, cols)

# Structure
Square    = str
DigitSet  = str
Grid      = Dict[Square, DigitSet] # E.g. {'A9': '123', ...}
all_boxes = [cross(rs, cs)  for rs in ('ABC','DEF','GHI') for cs in ('123','456','789')]
all_units = [cross(rows, c) for c in cols] + [cross(r, cols) for r in rows] + all_boxes
units     = {s: tuple(u for u in all_units if s in u) for s in squares}
peers     = {s: set().union(*units[s]) - {s} for s in squares}

# Solution Checker - Simple
def is_solution(solution: Grid, puzzle: Grid) -> bool:
    "Does the solution work/is it valid?"
    return (solution is not None and 
            all(solution[s] in puzzle[s] for s in squares) and 
            all({solution[s] for s in unit} == set(digits) for unit in all_units))


# --- Second Part of the Program --- CSP ---

def constrain(grid: Grid) -> Optional[Grid]:
    "Return a new grid with the puzzle's givens filled in and propagated."
    result = {s: digits for s in squares}
    for s in grid:
        if len(grid[s]) == 1:
            if fill(result, s, grid[s]) is None:
                return None
    return result

def fill(grid: Grid, s: str, d: str) -> Optional[Grid]:
    "Assign d to s by eliminating every other candidate (mutates grid)."
    if grid[s] == d or all(eliminate(grid, s, d2) for d2 in grid[s] if d2 != d):
        return grid
    return None


def eliminate(grid: Grid, s: str, d: str) -> Optional[Grid]:
    "Remove d from s's domain and propagate both rules (mutates grid)."
    if d not in grid[s]:
        return grid                                  # already gone
    grid[s] = grid[s].replace(d, '')
    if not grid[s]:
        return None                                  # domain wipe-out
    if len(grid[s]) == 1:                            # rule 1: singleton => prune peers
        d2 = grid[s]
        if not all(eliminate(grid, p, d2) for p in peers[s]):
            return None
    for u in units[s]:                               # rule 2: hidden single in a unit
        places = [p for p in u if d in grid[p]]
        if not places or (len(places) == 1 and not fill(grid, places[0], d)):
            return None
    return grid

# --- Third Part of the Program --- I/O ---

def parse(picture: str) -> Grid:
    vals = re.findall(r"[.1-9]|[{][1-9]+[}]", picture)
    assert len(vals) == 81, f"expected 81 cells, got {len(vals)}"
    return {s: digits if v == '.' else re.sub(r"[{}]", '', v) for s, v in zip(squares, vals)}
 
def picture(grid: Optional[Grid]) -> str:
    if grid is None:
        return "None"
    val = lambda d: '.' if d == digits else d if len(d) == 1 else '{' + d + '}'
    w = max(len(val(grid[s])) for s in grid)
    dash = '\n' + '+'.join(3 * ['-' * (w * 3 + 2)])
    cell = lambda r, c: val(grid[r + c]).center(w) + ('|' if c in '36' else ' ')
    line = lambda r: ''.join(cell(r, c) for c in cols) + (dash if r in 'CF' else '')
    return '\n'.join(map(line, rows))


# --- Fourth Part of the Program --- Search ---

def search(grid: Optional[Grid]) -> Optional[Grid]:
    "Depth-first search with constraint propagation to find a solution."
    if grid is None: 
        return None
    s = min((s for s in squares if len(grid[s]) > 1), 
            default=None, key=lambda s: len(grid[s]))
    if s is None: # No squares with multiple possibilities; the search has succeeded
        return grid
    for d in grid[s]:
        solution = search(fill(grid.copy(), s, d))
        if solution:
            return solution
    return None

def solve(grid: Grid) -> Optional[Grid]:
    "Helper to constrain the initial puzzle and then search for a solution."
    return search(constrain(grid))

# --- Main ---

if __name__ == '__main__':
    p1 = parse("53..7.... 6..195... .98....6. 8...6...3 4..8.3..1 7...2...6 .6....28. ...419..5 ....8..79")
    p2 = parse("4.....8.5.3..........7......2.....6.....8.4......1.......6.3.7.5..2.....1.4......")
    
    for p in (p1, p2, parse('.' * 81)):
        sol = solve(p)
        assert is_solution(sol, p)
        
    print(picture(constrain(p1)))
    print("constrain(p2) fills", sum(len(v) == 1 for v in constrain(p2).values()), "of 81 squares")
    print(picture(solve(p2)))