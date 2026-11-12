/*  world.pl -- the ENVIRONMENT (simulator).  Not part of the agent.

    Loads a world exported by environments.py (export_json), hands out
    percepts and applies actions.  The agent never sees these facts.

    Scoring (AIMA): -1 per action, -10 extra for the arrow,
                    +1000 for climbing out with the gold, -1000 for dying.
*/
:- module(world, [load_world/1, set_world/3, world_size/2, world_percept/1, world_do/1,
                  world_over/1, world_score/1, world_thing/3, world_agent/3]).
:- if(exists_source(library(json))).      % SWI-Prolog 9.3+ moved the library
:- use_module(library(json)).
:- else.
:- use_module(library(http/json)).
:- endif.

:- dynamic thing/3, dims/2, pos/2, dir/1, holding/0, arrow/0, wumpus_alive/0,
           bump/0, scream/0, over/1, score/1.

%% -------------------------------------------------------------- CONSTANTS
start(1, 1, east).          % start square and heading
points(action,   -1).       % every action
points(arrow,   -10).       % extra, for shooting
points(gold,   1000).       % climbing out with the gold
points(death, -1000).

load_world(File) :-
    setup_call_cleanup(open(File, read, S), json_read_dict(S, Dict), close(S)),
    get_dict(world_dims, Dict, [W,H]),
    get_dict(world, Dict, Columns),
    findall(thing(Type,X,Y),
            ( nth0(X, Columns, Column), nth0(Y, Column, Items),
              member(Item, Items), Item \== null,
              atom_string(Atom, Item), downcase_atom(Atom, Type),
              Type \== explorer ),
            Things),
    set_world(W, H, Things).

% set_world(+W, +H, +Things): W,H include the wall ring; Things is a list of
% thing(Type,X,Y) with Type one of wall, pit, wumpus, gold.
set_world(W, H, Things) :-
    retractall(thing(_,_,_)), retractall(dims(_,_)),
    assertz(dims(W,H)),
    forall(member(thing(T,X,Y), Things), assertz(thing(T,X,Y))),
    check_world,
    retractall(pos(_,_)), retractall(dir(_)), retractall(holding),
    retractall(arrow), retractall(wumpus_alive), retractall(bump),
    retractall(scream), retractall(over(_)), retractall(score(_)),
    start(SX,SY,SD),
    assertz(pos(SX,SY)), assertz(dir(SD)), assertz(arrow),
    assertz(wumpus_alive), assertz(score(0)),
    ( deadly(SX,SY) -> die ; true ).

% Warn about worlds the generator could never produce.
check_world :-
    aggregate_all(count, thing(wumpus,_,_), NW),
    ( NW =< 1 -> true
    ; format(user_error, "warning: world has ~d wumpuses (agent assumes 1)~n", [NW]) ),
    start(SX,SY,_),
    forall(( member(T, [pit,wumpus]), thing(T,SX,SY) ),
           format(user_error, "warning: ~w on the start square~n", [T])),
    (   thing(wall,SX,SY)
    ->  throw(error(invalid_world('start square (1,1) is a wall'), _))
    ;   true
    ).

% Interior size (without the wall ring) -- what the agent may be told.
world_size(XMax, YMax) :- dims(W,H), XMax is W-2, YMax is H-2.

world_thing(T,X,Y) :- thing(T,X,Y).
world_agent(X,Y,D) :- pos(X,Y), dir(D).
world_over(Why)    :- over(Why).
world_score(S)     :- score(S).

world_percept([St,Br,Gl,Bu,Sc]) :-
    pos(X,Y),
    ( near(wumpus,X,Y)           -> St = stench  ; St = none ),
    ( near(pit,X,Y)              -> Br = breeze  ; Br = none ),
    ( thing(gold,X,Y)            -> Gl = glitter ; Gl = none ),
    ( retract(bump)              -> Bu = bump    ; Bu = none ),
    ( retract(scream)            -> Sc = scream  ; Sc = none ).

near(T,X,Y) :- step(_,X,Y,A,B), thing(T,A,B), !.

step(east,X,Y,A,Y)  :- A is X+1.
step(north,X,Y,X,B) :- B is Y+1.
step(west,X,Y,A,Y)  :- A is X-1.
step(south,X,Y,X,B) :- B is Y-1.

lturn(east,north). lturn(north,west). lturn(west,south). lturn(south,east).

blocked(X,Y) :- thing(wall,X,Y), !.
blocked(X,Y) :- dims(W,H), ( X < 1 ; Y < 1 ; X >= W-1 ; Y >= H-1 ), !.   % wall ring

deadly(X,Y) :- thing(pit,X,Y), !.
deadly(X,Y) :- thing(wumpus,X,Y), wumpus_alive, !.

add_score(N) :- retract(score(S)), S1 is S+N, assertz(score(S1)).
add_points(What) :- points(What, N), add_score(N).
die :- add_points(death), assertz(over(died)).

world_do(Action) :- add_points(action), do(Action), !.
world_do(_).

do(turnleft)  :- retract(dir(D)), lturn(D,D2), assertz(dir(D2)).
do(turnright) :- retract(dir(D)), lturn(D2,D), assertz(dir(D2)).
do(forward) :-
    pos(X,Y), dir(D), step(D,X,Y,A,B),
    (   blocked(A,B)
    ->  assertz(bump)
    ;   retract(pos(X,Y)), assertz(pos(A,B)),
        ( deadly(A,B) -> die ; true )
    ).
do(grab) :-
    pos(X,Y), retract(thing(gold,X,Y)), retractall(thing(gold,X,Y)),
    assertz(holding).
do(shoot) :-
    retract(arrow), add_points(arrow),
    pos(X,Y), dir(D), fly(D,X,Y).
do(climb) :-
    start(SX,SY,_), pos(SX,SY),
    (   holding
    ->  add_points(gold), assertz(over(won))
    ;   assertz(over(left))
    ).

% The arrow flies straight until it hits a wall; it kills a wumpus on the way.
fly(D,X,Y) :-
    step(D,X,Y,A,B),
    (   blocked(A,B) -> true
    ;   thing(wumpus,A,B), wumpus_alive
    ->  retract(wumpus_alive), assertz(scream)
    ;   fly(D,A,B)
    ).
