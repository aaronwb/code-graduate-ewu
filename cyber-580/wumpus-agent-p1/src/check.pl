/*  check.pl -- SOUNDNESS CHECKS.  Not part of the agent.

    After every step the runner compares what the KB believes with the truth
    held by the simulator.  A wrong belief is reported at the step it first
    appears, instead of several moves later when the agent dies.

      position   the KB's position is the world's position
      facing     the KB's heading is the world's heading
      safe       every square the KB calls safe has no pit and no live wumpus
      pit        every pit the KB is certain of is a real pit
      wumpus     a wumpus the KB is certain of is the real wumpus
      gamble     without a risk limit the agent only walks onto safe squares
      death      without a risk limit the agent never dies

    Each failure is a term  violation(Check, Detail).
*/
:- module(check, [check_before_action/2, check_after_action/1]).
:- use_module(kb).
:- use_module(world).

% check_before_action(+Action, -Violations): call after agent_step chose
% Action and before the world applies it.
check_before_action(Action, Vs) :-
    findall(V, before(Action, V), Vs).

% check_after_action(-Violations): call after the world applied the action.
check_after_action(Vs) :-
    findall(V, after(V), Vs).

before(_, violation(position, kb(KX,KY)-world(WX,WY))) :-
    ask(at(KX,KY)), world_agent(WX,WY,_),
    ( KX \== WX ; KY \== WY ).
before(_, violation(safe, square(X,Y)-has(T))) :-
    ask(unexplored(X,Y)), deadly_thing(T,X,Y).
before(_, violation(pit, square(X,Y)-is_not_a_pit)) :-
    ask(pit(X,Y)), \+ world_thing(pit,X,Y).
before(_, violation(wumpus, square(X,Y)-is_not_the_wumpus)) :-
    ask(wumpus(X,Y)), \+ world_thing(wumpus,X,Y).
before(forward, violation(gamble, walked_onto(A,B))) :-
    \+ ask(risk_limit(_)),
    ask(at(X,Y)), world_agent(_,_,D), ask(ahead(X,Y,D,A,B)),
    \+ ask(safe(A,B)).

after(violation(facing, kb(KD)-world(WD))) :-
    ask(facing(KD)), world_agent(_,_,WD), KD \== WD.
after(violation(death, died_without_risk_limit)) :-
    world_over(died), \+ ask(risk_limit(_)).

deadly_thing(pit,X,Y)    :- world_thing(pit,X,Y).
deadly_thing(wumpus,X,Y) :- world_thing(wumpus,X,Y), \+ ask(wumpus_dead).
