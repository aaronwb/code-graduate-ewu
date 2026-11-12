/*  kb.pl -- the logical KNOWLEDGE BASE for the Wumpus world.

    Everything the agent knows lives here and nowhere else.  The only way in
    is tell/1 and the only way out is ask/1.

    STORED FACTS (all bounded by the number of squares, never by time):
      at(X,Y) facing(D)        where the agent is now            (1 fact each)
      visited(X,Y)             squares the agent has stood on    (<= squares)
      breeze(X,Y) stench(X,Y)  positive percepts only; "no breeze" is
                               visited(X,Y), \+ breeze(X,Y)      (<= squares)
      wall(X,Y)                squares that produced a bump
      size(XMax,YMax)          cave size, if the agent was told
      safe(X,Y)                DERIVED: unvisited square proven safe.  This is
                               the agent's to-do list; the fact is removed
                               again when the square is visited. (<= squares)
      missed(X,Y,D)            arrow fired from X,Y facing D, no scream (<= 1)
      risk_limit(T)            highest death risk the agent may accept (0..1);
                               absent = never gamble                (<= 1)
      glitter, has_gold, arrow_used, wumpus_dead, last_action(A)   flags

    Coordinates are the generator's: start (1,1), east = +X, north = +Y.
*/
:- module(kb, [tell/1, ask/1]).

:- dynamic at/2, facing/1, visited/2, breeze/2, stench/2, wall/2, size/2,
           safe/2, missed/3, glitter/0, has_gold/0, arrow_used/0,
           wumpus_dead/0, last_action/1, risk_limit/1.

%% -------------------------------------------------------------- CONSTANTS
% Every number the KB relies on is named here.  The rules below contain no
% other numbers except 0 and 1 used in plain arithmetic, so a stray number
% or a "mod" anywhere else in this file does not belong.

start_square(1, 1).     % where the agent starts, and the only exit
start_facing(east).
first_coordinate(1).    % smallest X and smallest Y inside the cave
pit_prior(0.2).         % chance that any one square is a pit (generator default)
max_cluster(10).        % pit risk weighs at most 2^10 layouts per square

% stored(?Fact): every kind of fact the KB can hold.  Used to wipe the KB and
% to count what is in it.
stored(at(_,_)).        stored(facing(_)).      stored(visited(_,_)).
stored(breeze(_,_)).    stored(stench(_,_)).    stored(wall(_,_)).
stored(size(_,_)).      stored(safe(_,_)).      stored(missed(_,_,_)).
stored(glitter).        stored(has_gold).       stored(arrow_used).
stored(wumpus_dead).    stored(last_action(_)). stored(risk_limit(_)).

%% ------------------------------------------------------------------ TELL

% tell(init): forget everything, start on the start square.
tell(init) :-
    forall(stored(F), retractall(F)),
    start_square(X,Y), start_facing(D),
    assertz(at(X,Y)),
    assertz(facing(D)).

% tell(size(XMax,YMax)): optional. Without it the agent finds walls by bumping.
tell(size(XMax,YMax)) :-
    retractall(size(_,_)),
    assertz(size(XMax,YMax)).

% tell(risk_limit(T)): optional. The agent may step onto an unproven square
% when its estimated death risk is at most T.  Never told = never gamble.
tell(risk_limit(T)) :-
    retractall(risk_limit(_)),
    assertz(risk_limit(T)).

% tell(percept([Stench,Breeze,Glitter,Bump,Scream])): each slot is its own
% name when sensed, or none.  e.g. [none,breeze,none,none,none]
tell(percept([Stench,Breeze,Glitter,Bump,Scream])) :-
    settle_last_action(Bump, Scream),
    at(X,Y),
    (   visited(X,Y)
    ->  true
    ;   assertz(visited(X,Y)),
        retractall(safe(X,Y)),
        ( Breeze == breeze -> assertz(breeze(X,Y)) ; true ),
        ( Stench == stench -> assertz(stench(X,Y)) ; true )
    ),
    retractall(glitter),
    ( Glitter == glitter, \+ has_gold -> assertz(glitter) ; true ),
    infer.

% tell(action(A)): the agent reports what it is about to do.
tell(action(A)) :-
    retractall(last_action(_)),
    assertz(last_action(A)),
    apply_action(A).

apply_action(turnleft)  :- !, retract(facing(D)), left(D, D2),  assertz(facing(D2)).
apply_action(turnright) :- !, retract(facing(D)), left(D2, D),  assertz(facing(D2)).
apply_action(grab)      :- !, retractall(glitter), assertz(has_gold).
apply_action(shoot)     :- !, assertz(arrow_used).
apply_action(_).        % forward and shoot outcomes are settled by the next percept

% The result of forward / shoot is only known once the next percept arrives.
settle_last_action(Bump, Scream) :-
    ( Scream == scream, \+ wumpus_dead -> assertz(wumpus_dead) ; true ),
    (   retract(last_action(A))
    ->  settle(A, Bump, Scream)
    ;   true
    ).

settle(forward, Bump, _) :- !,
    at(X,Y), facing(D), ahead(X,Y,D,A,B),
    (   Bump == bump
    ->  assertz(wall(A,B)), retractall(safe(A,B))
    ;   retract(at(X,Y)), assertz(at(A,B))
    ).
settle(shoot, _, Scream) :- Scream \== scream, !,
    at(X,Y), facing(D), assertz(missed(X,Y,D)).
settle(_, _, _).

%% ------------------------------------------------------------- INFERENCE

% Forward chaining, run once per percept: every frontier square that can now
% be proven safe is stored as safe/2.  Cost is O(visited squares) per step.
infer :-
    forall(( frontier(X,Y), \+ safe(X,Y), no_pit(X,Y), no_wumpus(X,Y) ),
           assertz(safe(X,Y))).

% frontier: unvisited square next to a visited one.
frontier(X,Y) :- visited(A,B), adjacent(A,B,X,Y), \+ visited(X,Y).

% A square has no pit if some visited neighbour had no breeze.
no_pit(X,Y) :- adjacent(X,Y,A,B), visited(A,B), \+ breeze(A,B), !.

% A square has no (live) wumpus if ...
no_wumpus(_,_) :- wumpus_dead, !.                     % ... it is dead
no_wumpus(X,Y) :- cleared(X,Y), !.                    % ... local evidence
no_wumpus(X,Y) :- once(candidate(_,_)),               % ... there is ONE wumpus
                  \+ candidate(X,Y).                  %     and it must be elsewhere

% Local evidence: a visited neighbour with no stench, or an arrow flew through.
cleared(X,Y) :- adjacent(X,Y,A,B), visited(A,B), \+ stench(A,B), !.
cleared(X,Y) :- missed(SX,SY,D), arrow_path(SX,SY,D,X,Y), !.

% candidate: a square that could still hold the single wumpus -- it touches
% EVERY stench square and has not been cleared.  Fails if no stench is known.
% (If stench exists but no candidate does, the one-wumpus assumption is
% broken and no_wumpus/2 falls back to local evidence only.)
candidate(X,Y) :-
    once(stench(A,B)),
    adjacent(A,B,X,Y),
    \+ visited(X,Y),
    \+ cleared(X,Y),
    forall(stench(C,D), next_to(X,Y,C,D)).

% Squares an arrow certainly crossed: the first square ahead, and beyond it
% only while the squares are visited (so known not to be walls).
arrow_path(SX,SY,D,X,Y) :-
    ahead(SX,SY,D,A,B), in_world(A,B),
    (   A == X, B == Y
    ->  true
    ;   visited(A,B), arrow_path(A,B,D,X,Y)
    ).

% Backward-chained on demand only (map display / debugging); never stored.
pit(X,Y) :-
    breeze(A,B),
    findall(NX-NY, ( adjacent(A,B,NX,NY), \+ visited(NX,NY), \+ no_pit(NX,NY) ),
            [X-Y]).
wumpus(X,Y) :-
    \+ wumpus_dead,
    findall(CX-CY, candidate(CX,CY), [X-Y]).

%% ------------------------------------------------------------------ RISK
% Only asked when nothing is provably safe.  Never stored.

% risk(X,Y,R): R is the estimated chance that stepping onto frontier square
% X,Y kills the agent.  Pit and wumpus are treated as independent.
risk(X,Y,R) :-
    setof(FX-FY, ( frontier(FX,FY), \+ safe(FX,FY) ), Squares),
    member(X-Y, Squares),
    pit_risk(X,Y,RP),
    wumpus_risk(X,Y,RW),
    R is 1 - (1-RP)*(1-RW).

% Pit risk: gather the breezy squares that bear on X,Y and the unknown
% squares that could explain them (see evidence/4).  Weigh every pit layout
% of those unknown squares that explains all those breezes by its prior
% probability; the risk is the share of that weight where X,Y is a pit.
pit_risk(X,Y,0.0) :- no_pit(X,Y), !.
pit_risk(X,Y,R) :-
    evidence([X-Y], [], Breezy, Unknown),
    findall(W-Pits, ( layout(Unknown, Pits, W), explains(Breezy, Pits) ), Models),
    findall(W, member(W-_, Models), All),
    findall(W, ( member(W-Pits, Models), memberchk(X-Y, Pits) ), With),
    sum_list(All, Total), sum_list(With, Here),
    ( Total > 0 -> R is Here/Total ; R = 1.0 ).

% evidence(+Unknown0, +Breezy0, -Breezy, -Unknown): grow outward from the
% square in rings -- breezy squares touching the unknown squares so far, then
% the unknown squares touching those -- until nothing new turns up or the
% next ring would pass max_cluster.  Every breezy square returned has ALL of
% its possible pit squares in Unknown, so the layouts can be checked exactly.
evidence(Us0, Bs0, Bs, Us) :-
    findall(A-B, ( member(X-Y, Us0), adjacent(X,Y,A,B), breeze(A,B) ), L1),
    sort(L1, Bs1),
    findall(UX-UY, ( member(A-B, Bs1), adjacent(A,B,UX,UY),
                     \+ visited(UX,UY), \+ no_pit(UX,UY) ), L2),
    sort(L2, Us1),
    max_cluster(Max), length(Us1, N),
    (   Bs0 \== [], N > Max  ->  Bs = Bs0, Us = Us0      % too big: keep last ring
    ;   Us1 == Us0            ->  Bs = Bs1, Us = Us1      % nothing new: done
    ;   evidence(Us1, Bs1, Bs, Us)
    ).

% layout(+Squares, -Pits, -Weight): every way of putting pits on Squares.
layout([], [], 1.0).
layout([S|Ss], Pits, W) :-
    layout(Ss, Pits0, W0),
    pit_prior(P),
    (   Pits = [S|Pits0], W is W0*P
    ;   Pits = Pits0,     W is W0*(1-P)
    ).

% Every breezy square has at least one of the pits next to it.
explains(Breezy, Pits) :-
    forall(member(A-B, Breezy), ( member(PX-PY, Pits), next_to(A,B,PX,PY) )).

% Wumpus risk: the wumpus is equally likely to be on any remaining candidate.
wumpus_risk(X,Y,0.0) :- no_wumpus(X,Y), !.
wumpus_risk(_,_,R) :-
    findall(c, candidate(_,_), Cs), length(Cs, N),
    ( N > 0 -> R is 1/N ; R = 1.0 ).

%% --------------------------------------------------------------- GEOMETRY

delta(east,  1, 0).
delta(north, 0, 1).
delta(west, -1, 0).
delta(south, 0,-1).

left(east, north).  left(north, west).  left(west, south).  left(south, east).

ahead(X,Y,D,A,B) :- delta(D,DX,DY), A is X+DX, B is Y+DY.

adjacent(X,Y,A,B) :- ahead(X,Y,_,A,B), in_world(A,B).

next_to(X,Y,A,B) :- 1 =:= abs(X-A) + abs(Y-B).

in_world(X,Y) :-
    first_coordinate(Min), X >= Min, Y >= Min,
    \+ wall(X,Y),
    ( size(XMax,YMax) -> X =< XMax, Y =< YMax ; true ).

%% ------------------------------------------------------------------- ASK

% Questions the agent uses to choose an action.
ask(at(X,Y))            :- at(X,Y).
ask(home(X,Y))          :- start_square(X,Y).
ask(facing(D))          :- facing(D).
ask(glitter)            :- glitter.
ask(has_gold)           :- has_gold.
ask(safe(X,Y))          :- ( visited(X,Y) ; safe(X,Y) ), !.
ask(unexplored(X,Y))    :- safe(X,Y).
ask(shoot_target(X,Y))  :- \+ arrow_used, \+ wumpus_dead,
                           frontier(X,Y), \+ safe(X,Y), no_pit(X,Y).
ask(risk_limit(T))      :- risk_limit(T).
ask(risk(X,Y,R))        :- risk(X,Y,R).
ask(ahead(X,Y,D,A,B))   :- ahead(X,Y,D,A,B).
ask(left(D,D2))         :- left(D,D2).
% Questions used only for display / debugging.
ask(visited(X,Y))       :- visited(X,Y).
ask(breeze(X,Y))        :- breeze(X,Y).
ask(stench(X,Y))        :- stench(X,Y).
ask(wall(X,Y))          :- wall(X,Y).
ask(pit(X,Y))           :- pit(X,Y).
ask(wumpus(X,Y))        :- wumpus(X,Y).
ask(wumpus_dead)        :- wumpus_dead.
ask(size(XMax,YMax))    :- size(XMax,YMax).
ask(fact_count(N))      :- aggregate_all(count, ( stored(F), call(F) ), N).
