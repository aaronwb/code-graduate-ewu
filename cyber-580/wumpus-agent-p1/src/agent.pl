/*  agent.pl -- the AGENT.

    The agent is an object with one public method, agent_step/2, and no
    state of its own: this file declares no dynamic predicates and uses no
    global variables.  Percepts go into the knowledge base with tell/1,
    decisions come out of it with ask/1, and everything else (the route
    search) lives in local variables that vanish when agent_step returns.

    Decision order:
      1. glitter here                      -> grab
      2. holding the gold                  -> walk home through safe squares, climb
      3. a proven-safe unvisited square    -> walk to the nearest one
      4. a pit-free square that only the
         wumpus might be in, and an arrow  -> walk next to it, face it, shoot
      5. nothing safe is left, and the KB
         was told a risk limit             -> step onto the least risky unknown
                                              square, if its risk is within the limit
      6. otherwise                         -> walk home, climb out empty-handed
*/
:- module(agent, [agent_step/2]).
:- use_module(kb).

% agent_step(+Percept, -Action)
agent_step(Percept, Action) :-
    tell(percept(Percept)),
    choose(Action),
    tell(action(Action)).

choose(grab)   :- ask(glitter), !.
choose(Action) :- ask(has_gold), !, go_home(Action).
choose(Action) :- findall(X-Y, ask(unexplored(X,Y)), [G|Gs]),
                  route(squares([G|Gs]), Action), !.
choose(Action) :- findall(X-Y, ask(shoot_target(X,Y)), [T|Ts]),
                  route(aim([T|Ts]), Action), !.
choose(Action) :- ask(risk_limit(Limit)),
                  findall(R-(X-Y), ask(risk(X,Y,R)), Risks),
                  least_risky(Risks, Limit, Squares),
                  route(squares(Squares), Action), !.
choose(Action) :- go_home(Action).

% least_risky(+Risk-Square pairs, +Limit, -Squares): the squares that share
% the lowest risk, provided that risk is within the limit.
least_risky(Risks, Limit, Squares) :-
    findall(R, member(R-_, Risks), [R0|Rs]),
    min_list([R0|Rs], Min),
    Min =< Limit,
    risk_tie(Tie),
    findall(S, ( member(R-S, Risks), R =< Min + Tie ), Squares).

risk_tie(1.0e-9).   % risks closer than this count as equal (rounding noise)

go_home(climb)  :- ask(home(X,Y)), ask(at(X,Y)), !.
go_home(Action) :- ask(home(X,Y)), route(squares([X-Y]), Action), !.
go_home(climb).     % unreachable in a sane world; never get stuck

%% --------------------------------------------------------------- ROUTING
% Breadth-first search over (X,Y,Facing) states, moving only through squares
% the KB says are safe.  Every action costs 1, so the first goal state found
% is the cheapest in actions (turns included).  Only the FIRST action of the
% route is returned; the agent re-plans from scratch every step, so no plan
% is ever stored.

route(Goal, Action) :-
    ask(at(X,Y)), ask(facing(D)),
    Start = s(X,Y,D),
    bfs([Start-none], [Start], Goal, Action).

% bfs(+Queue, +Seen, +Goal, -Action); queue items are State-FirstActionTaken.
bfs([State-First|Queue], Seen, Goal, Action) :-
    (   at_goal(Goal, State)
    ->  ( First == none -> goal_action(Goal, Action) ; Action = First )
    ;   findall(Next-F,
                ( move(Goal, State, A, Next),
                  \+ memberchk(Next, Seen),
                  ( First == none -> F = A ; F = First ) ),
                Children),
        findall(S, member(S-_, Children), NewStates),
        append(NewStates, Seen, Seen1),
        append(Queue, Children, Queue1),
        bfs(Queue1, Seen1, Goal, Action)
    ).

at_goal(squares(List), s(X,Y,_)) :- memberchk(X-Y, List).
at_goal(aim(List),     s(X,Y,D)) :- ask(ahead(X,Y,D,A,B)), memberchk(A-B, List).

% What to do when already standing in a goal state.
goal_action(aim(_), shoot).

% Forward is allowed onto safe squares, and onto a destination square (which
% is only ever unsafe when the agent has deliberately chosen to gamble on it).
move(_, s(X,Y,D), turnleft,  s(X,Y,D2)) :- ask(left(D,D2)).
move(_, s(X,Y,D), turnright, s(X,Y,D2)) :- ask(left(D2,D)).
move(Goal, s(X,Y,D), forward, s(A,B,D)) :-
    ask(ahead(X,Y,D,A,B)),
    ( ask(safe(A,B)) -> true ; Goal = squares(List), memberchk(A-B, List) ).
