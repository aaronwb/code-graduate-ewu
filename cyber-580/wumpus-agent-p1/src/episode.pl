/*  episode.pl -- plays ONE episode in the world that is currently loaded.

    episode(+Options, -Result)

    Options:  blind      do not tell the agent the cave size
              risk(T)    tell the agent a risk limit
              trace      print every percept and action
              map        also draw the agent's map every step

    Result:   result(Outcome, Score, Actions, Inferences, Facts, Violations)
      Outcome     won / left / died / timeout
      Actions     number of actions taken
      Inferences  Prolog inference steps spent inside the agent and KB: a
                  measure of WORK that does not depend on how fast the
                  machine is
      Facts       facts stored in the KB when the episode ended
      Violations  failed soundness checks (see check.pl); [] when all is well
*/
:- module(episode, [episode/2]).
:- use_module(kb).
:- use_module(agent).
:- use_module(world).
:- use_module(check).

max_steps(20000).       % safety stop; a healthy episode is far shorter

episode(Opts, result(Why, Score, Steps, Inferences, Facts, Violations)) :-
    tell(init),
    ( memberchk(blind, Opts) -> true ; world_size(XM,YM), tell(size(XM,YM)) ),
    ( memberchk(risk(T), Opts) -> tell(risk_limit(T)) ; true ),
    loop(0, Opts, 0, [], Steps, Inferences, Violations),
    world_over(Why), world_score(Score),
    ask(fact_count(Facts)).

loop(N, Opts, I0, V0, Steps, Inferences, Violations) :-
    (   world_over(_)
    ->  Steps = N, Inferences = I0, Violations = V0
    ;   max_steps(N)
    ->  Steps = N, Inferences = I0, Violations = V0,
        assertz(world:over(timeout))
    ;   world_percept(P),
        world_agent(X, Y, D),
        statistics(inferences, Before),
        agent_step(P, Action),
        statistics(inferences, After),
        I1 is I0 + After - Before,
        check_before_action(Action, Va),
        ( wants_trace(Opts) -> show_step(N, X, Y, D, P, Action, Opts) ; true ),
        world_do(Action),
        check_after_action(Vb),
        append(Va, Vb, Vstep),
        report(N, Vstep),
        tag(N, Vstep, Tagged),
        append(V0, Tagged, V1),
        N1 is N+1,
        loop(N1, Opts, I1, V1, Steps, Inferences, Violations)
    ).

tag(N, Vs, Tagged) :- findall(step(N)-V, member(V, Vs), Tagged).

report(N, Vs) :-
    forall(member(violation(Check, Detail), Vs),
           format(user_error, "CHECK FAILED at step ~d: ~w ~w~n", [N, Check, Detail])).

wants_trace(Opts) :- ( memberchk(trace, Opts) ; memberchk(map, Opts) ), !.

show_step(N, X, Y, D, P, Action, Opts) :-
    exclude(==(none), P, Sensed),
    format("~t~d~4| at (~d,~d) facing ~w~t~30| percept ~w~t~62| -> ~w~n",
           [N, X, Y, D, Sensed, Action]),
    ( memberchk(map, Opts) -> draw_map ; true ).

% Agent's-eye map, drawn after the action was told to the KB.
%   @ agent   . visited   o proven safe   P pit   W wumpus   # wall   ? unknown
%   b / s after a visited square = breeze / stench sensed there
%   a number = estimated death risk (%) of an unproven square next to
%   explored ground, e.g. 56 = 56% (99 = certain or nearly)
draw_map :-
    world_size(XM,YM), XE is XM+1, YE is YM+1,
    forall(between(0, YE, I),
           ( Y is YE-I,
             format("      "),
             forall(between(0, XE, X), ( cell(X,Y,C), format("~w", [C]) )),
             nl )),
    nl.

cell(X,Y,'@  ') :- ask(at(X,Y)), !.
cell(X,Y,'#  ') :- ( ask(wall(X,Y)) ; X < 1 ; Y < 1
                   ; ask(size(XM,YM)), ( X > XM ; Y > YM ) ), !.
cell(X,Y,C)     :- ask(visited(X,Y)), !,
                   ( ask(breeze(X,Y)) -> B = b ; B = ' ' ),
                   ( ask(stench(X,Y)) -> S = s ; S = ' ' ),
                   atomic_list_concat(['.',B,S], C).
cell(X,Y,'o  ') :- ask(unexplored(X,Y)), !.
cell(X,Y,'W  ') :- ask(wumpus(X,Y)), !.
cell(X,Y,'P  ') :- ask(pit(X,Y)), !.
cell(X,Y,C)     :- ask(risk(X,Y,R)), !,
                   Pct is min(99, round(R*100)),
                   format(atom(C), "~|~`0t~d~2+ ", [Pct]).
cell(_,_,'?  ').
