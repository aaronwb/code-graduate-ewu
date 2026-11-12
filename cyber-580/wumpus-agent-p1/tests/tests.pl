/*  tests.pl -- unit tests, one small behaviour each.

        swipl -g run_tests -t halt tests/tests.pl      (from the project folder)

    Each test name says which rule it checks, so a failing test points at the
    predicate to read.  Sections follow the layers of the program:
        kb_state   tell/1 bookkeeping        (kb.pl: tell, apply_action, settle)
        kb_pit     pit reasoning             (kb.pl: no_pit, pit)
        kb_wumpus  wumpus reasoning          (kb.pl: no_wumpus, cleared, candidate)
        kb_risk    risk estimates            (kb.pl: risk, pit_risk, wumpus_risk)
        agent      action choice and routes  (agent.pl)
        agent_risk gambling when allowed     (agent.pl: choose, least_risky)
        world      the simulator             (world.pl)
        checker    the soundness checks      (check.pl) -- do they notice lies?
        episodes   whole runs with exact expected scores
*/
:- use_module(library(plunit)).
:- use_module('../src/kb').
:- use_module('../src/agent').
:- use_module('../src/world').
:- use_module('../src/check').
:- use_module('../src/episode').

%% helpers ---------------------------------------------------------------
% percept(+Names): tell a percept naming only what is sensed, e.g. percept([breeze]).
percept(Names) :-
    findall(V, ( member(S, [stench,breeze,glitter,bump,scream]),
                 ( memberchk(S, Names) -> V = S ; V = none ) ), P),
    tell(percept(P)).

fresh :- tell(init), tell(size(4,4)).

% do(+Action, +PerceptNames): tell the action, then the percept that follows.
do(A, Names) :- tell(action(A)), percept(Names).

safe_list(L)   :- findall(X-Y, ask(unexplored(X,Y)), L0), sort(L0, L).
target_list(L) :- findall(X-Y, ask(shoot_target(X,Y)), L0), sort(L0, L).

% run(+File, -Outcome, -Score): play a whole episode.
run(File, Why, Score) :-
    load_world(File), tell(init), world_size(XM,YM), tell(size(XM,YM)),
    play(0), world_over(Why), world_score(Score).
% While playing, check the README's promise on every step: the KB never holds
% more than 3 facts per square (visited + breeze + stench) plus a few flags.
play(N) :- ( world_over(_) -> true
           ; N >= 5000 -> throw(too_many_steps)
           ; world_percept(P), agent_step(P, A), check_kb_size, world_do(A),
             N1 is N+1, play(N1) ).
check_kb_size :-
    world_size(XM,YM), ask(fact_count(C)), Limit is 3*XM*YM + 12,
    ( C =< Limit -> true ; throw(kb_too_big(C, limit(Limit))) ).

%% -----------------------------------------------------------------------
:- begin_tests(kb_state).

test(starts_at_1_1_facing_east) :-
    fresh, ask(at(1,1)), ask(facing(east)).
test(first_percept_marks_start_visited) :-
    fresh, percept([]), ask(visited(1,1)).
test(turnleft_cycles_east_north_west_south) :-
    fresh, percept([]),
    tell(action(turnleft)), ask(facing(north)),
    tell(action(turnleft)), ask(facing(west)),
    tell(action(turnleft)), ask(facing(south)),
    tell(action(turnleft)), ask(facing(east)).
test(turnright_cycles_east_south_west_north) :-
    fresh, percept([]),
    tell(action(turnright)), ask(facing(south)),
    tell(action(turnright)), ask(facing(west)),
    tell(action(turnright)), ask(facing(north)),
    tell(action(turnright)), ask(facing(east)).
test(forward_east_moves_plus_x) :-
    fresh, percept([]), do(forward, []), ask(at(2,1)), ask(visited(2,1)).
test(forward_north_moves_plus_y) :-
    fresh, percept([]), tell(action(turnleft)), percept([]),
    do(forward, []), ask(at(1,2)).
test(bump_means_no_move_and_a_wall) :-
    fresh, percept([]), do(forward, [bump]),
    ask(at(1,1)), ask(wall(2,1)), \+ ask(safe(2,1)).
test(turn_does_not_move) :-
    fresh, percept([]), do(turnleft, []), ask(at(1,1)).
test(visited_square_is_no_longer_on_todo_list) :-
    fresh, percept([]), do(forward, []), safe_list(L), \+ memberchk(2-1, L).
test(glitter_then_grab_sets_has_gold) :-
    fresh, percept([glitter]), ask(glitter), \+ ask(has_gold),
    tell(action(grab)), ask(has_gold), \+ ask(glitter).
test(glitter_flag_clears_when_not_sensed) :-
    fresh, percept([glitter]), do(turnleft, []), \+ ask(glitter).
test(breeze_and_stench_recorded_where_sensed) :-
    fresh, percept([breeze,stench]), ask(breeze(1,1)), ask(stench(1,1)).
test(nothing_recorded_when_nothing_sensed) :-
    fresh, percept([]), \+ ask(breeze(1,1)), \+ ask(stench(1,1)).
test(size_keeps_squares_outside_off_the_list) :-
    tell(init), tell(size(1,1)), percept([]), safe_list([]).
test(without_size_neighbours_are_still_found) :-
    tell(init), percept([]), safe_list([1-2, 2-1]).

test(glitter_ignored_once_gold_is_held) :-
    fresh, percept([glitter]), tell(action(grab)), percept([glitter]),
    \+ ask(glitter).

test(fresh_kb_holds_two_facts) :-
    tell(init), ask(fact_count(2)).           % at(1,1) and facing(east)
test(init_wipes_everything) :-
    fresh, tell(risk_limit(0.5)), percept([stench,breeze,glitter]),
    tell(action(shoot)), tell(init), ask(fact_count(2)).
test(revisiting_a_square_adds_no_facts) :-
    fresh, percept([]), do(turnleft, []), ask(fact_count(N)),
    do(turnleft, []), do(turnleft, []), do(turnleft, []), ask(fact_count(N)).

:- end_tests(kb_state).

%% -----------------------------------------------------------------------
:- begin_tests(kb_pit).

test(quiet_start_makes_both_neighbours_safe) :-
    fresh, percept([]), safe_list([1-2, 2-1]).
test(breeze_at_start_makes_nothing_safe) :-
    fresh, percept([breeze]), safe_list([]).
test(breezy_square_adds_no_safe_squares) :-
    fresh, percept([]), do(forward, [breeze]), safe_list([1-2]).
test(square_between_breezy_and_quiet_is_safe) :-
    % (2,1) breezy, (1,2) quiet: (2,2) touches the quiet one, so no pit.
    fresh, percept([]), do(forward, [breeze]),
    do(turnleft, [breeze]), do(turnleft, [breeze]), do(forward, []),
    do(turnright, []), do(forward, []),
    ask(at(1,2)), safe_list(L), memberchk(2-2, L), \+ memberchk(3-1, L).
test(pit_pinned_when_only_one_neighbour_is_possible) :-
    % as above: (2,1) breezy, (1,1) visited, (2,2) cleared -> pit is (3,1).
    fresh, percept([]), do(forward, [breeze]),
    do(turnleft, [breeze]), do(turnleft, [breeze]), do(forward, []),
    do(turnright, []), do(forward, []),
    ask(pit(3,1)), \+ ask(pit(2,2)).
test(pit_not_pinned_with_two_possibilities) :-
    fresh, percept([]), do(forward, [breeze]), \+ ask(pit(_,_)).

:- end_tests(kb_pit).

%% -----------------------------------------------------------------------
:- begin_tests(kb_wumpus).

test(stench_at_start_makes_nothing_safe) :-
    fresh, percept([stench]), safe_list([]).
test(stench_at_start_gives_two_shoot_targets) :-
    fresh, percept([stench]), target_list([1-2, 2-1]).
test(breeze_and_stench_gives_no_shoot_target) :-
    fresh, percept([stench,breeze]), target_list([]).
test(no_stench_means_no_shoot_target) :-
    fresh, percept([]), target_list([]).
test(shoot_uses_up_the_arrow) :-
    fresh, percept([stench]), do(shoot, [stench]), target_list([]).
test(scream_makes_stench_neighbours_safe) :-
    fresh, percept([stench]), do(shoot, [stench,scream]),
    ask(wumpus_dead), safe_list([1-2, 2-1]).
test(miss_clears_square_ahead_and_pins_wumpus) :-
    fresh, percept([stench]), do(shoot, [stench]),
    safe_list([2-1]), ask(wumpus(1,2)).
test(miss_clears_only_the_firing_direction) :-
    fresh, percept([stench]), tell(action(turnleft)), percept([stench]),
    do(shoot, [stench]), safe_list([1-2]), ask(wumpus(2,1)).
test(wumpus_not_pinned_with_two_candidates) :-
    fresh, percept([stench]), \+ ask(wumpus(_,_)).
test(two_stench_squares_pin_the_wumpus) :-
    % stench at (2,1) and (1,2), start quiet -> wumpus must be (2,2).
    fresh, percept([]), do(forward, [stench]),
    do(turnleft, [stench]), do(turnleft, [stench]), do(forward, []),
    do(turnright, []), do(forward, [stench]),
    ask(wumpus(2,2)), \+ ask(wumpus(3,1)).
test(one_wumpus_rule_clears_squares_far_from_a_stench) :-
    % same walk: (3,1) and (1,3) touch only one stench square each -> safe.
    fresh, percept([]), do(forward, [stench]),
    do(turnleft, [stench]), do(turnleft, [stench]), do(forward, []),
    do(turnright, []), do(forward, [stench]),
    safe_list([1-3, 3-1]).
test(pinned_wumpus_is_the_only_shoot_target) :-
    fresh, percept([]), do(forward, [stench]),
    do(turnleft, [stench]), do(turnleft, [stench]), do(forward, []),
    do(turnright, []), do(forward, [stench]),
    target_list([2-2]).
test(quiet_neighbour_clears_a_square_of_wumpus) :-
    % (2,1) stench, start quiet: (1,2) is safe, (2,2) and (3,1) are not.
    fresh, percept([]), do(forward, [stench]), safe_list([1-2]).
test(dead_wumpus_stench_is_ignored) :-
    fresh, percept([stench]), do(shoot, [stench,scream]),
    do(forward, [stench]), safe_list(L), memberchk(2-2, L), memberchk(3-1, L).

% Walk (1,1) (2,1) (2,2) (3,2)-stench, back to (2,2) (1,2).  Wumpus could be
% (4,2) or (3,3).  Used by the two arrow-path tests below.
walk_to_1_2 :-
    fresh, percept([]), do(forward, []), do(turnleft, []), do(forward, []),
    do(turnright, []), do(forward, [stench]),
    do(turnleft, [stench]), do(turnleft, [stench]),
    do(forward, []), do(forward, []), ask(at(1,2)), ask(facing(west)).
test(arrow_clears_through_visited_squares) :-
    % from (1,2) east the arrow crosses visited (2,2),(3,2) and reaches (4,2).
    walk_to_1_2, do(turnleft, []), do(turnleft, []), ask(facing(east)),
    do(shoot, []), ask(wumpus(3,3)).
test(arrow_does_not_clear_past_an_unvisited_square) :-
    % from (1,3) east, (2,3) is unvisited (might be a wall): (3,3) NOT cleared.
    walk_to_1_2, do(turnright, []), do(forward, []), do(turnright, []),
    ask(at(1,3)), ask(facing(east)),
    do(shoot, []), \+ ask(wumpus(_,_)).

:- end_tests(kb_wumpus).

%% -----------------------------------------------------------------------
:- begin_tests(kb_risk).

risk_of(X,Y,Pct) :- once(ask(risk(X,Y,R))), Pct is round(R*100).
risk_squares(L)  :- findall(X-Y, ask(risk(X,Y,_)), L).

test(no_risk_listed_when_everything_is_proven) :-
    fresh, percept([]), risk_squares([]).
test(breeze_at_start_two_squares_56_percent_each) :-
    % one pit at least among 2 squares, prior 0.2: 0.2 / (1 - 0.8^2) = 0.556
    fresh, percept([breeze]), risk_of(2,1,56), risk_of(1,2,56).
test(breeze_with_three_unknown_neighbours_is_41_percent) :-
    % walk (1,1) (2,1) (2,2), then breeze at (2,3): 0.2 / (1 - 0.8^3) = 0.41
    fresh, percept([]), do(forward, []), do(turnleft, []), do(forward, []),
    do(forward, [breeze]),
    risk_of(1,3,41), risk_of(3,3,41), risk_of(2,4,41).
test(square_explaining_two_breezes_is_riskier) :-
    % breeze at (2,1) and (1,2).  Layouts that explain both: (2,2) is a pit
    % (weight 0.2) or it is not and (3,1),(1,3) both are (0.8*0.2*0.2 = 0.032).
    % (2,2): 0.2/0.232 = 86%.   (3,1): (0.2*0.2 + 0.032)/0.232 = 31%.
    fresh, percept([]), do(forward, [breeze]),
    do(turnleft, [breeze]), do(turnleft, [breeze]), do(forward, []),
    do(turnright, []), do(forward, [breeze]),
    risk_of(2,2,86), risk_of(3,1,31), risk_of(1,3,31).
test(stench_at_start_is_50_50) :-
    fresh, percept([stench]), risk_of(2,1,50), risk_of(1,2,50).
test(pinned_wumpus_is_certain_death) :-
    fresh, percept([stench]), do(shoot, [stench]), risk_of(1,2,100).
test(dead_wumpus_is_no_risk) :-
    fresh, percept([stench]), do(shoot, [stench,scream]), risk_squares([]).
test(pit_and_wumpus_risks_combine) :-
    % 1 - (1-0.556)*(1-0.5) = 0.78
    fresh, percept([stench,breeze]), risk_of(2,1,78).
test(risk_limit_absent_by_default) :-
    fresh, \+ ask(risk_limit(_)).
test(risk_limit_is_stored_and_replaced) :-
    fresh, tell(risk_limit(0.3)), tell(risk_limit(0.5)),
    findall(T, ask(risk_limit(T)), [0.5]).

:- end_tests(kb_risk).

%% -----------------------------------------------------------------------
:- begin_tests(agent).

step(Names, Action) :-
    findall(V, ( member(S, [stench,breeze,glitter,bump,scream]),
                 ( memberchk(S, Names) -> V = S ; V = none ) ), P),
    agent_step(P, Action).

test(glitter_means_grab)             :- fresh, step([glitter], grab).
test(grab_beats_everything_else)     :- fresh, step([glitter,breeze,stench], grab).
test(breeze_at_start_means_climb)    :- fresh, step([breeze], climb).
test(quiet_start_means_forward)      :- fresh, step([], forward).
test(stench_at_start_means_shoot)    :- fresh, step([stench], shoot).
test(stench_and_breeze_means_climb)  :- fresh, step([stench,breeze], climb).
test(gold_at_start_grab_then_climb)  :- fresh, step([glitter], grab), step([], climb).
test(after_scream_walk_on)           :- fresh, step([stench], shoot),
                                        step([stench,scream], forward).
test(after_miss_walk_into_cleared_square) :-
    fresh, step([stench], shoot), step([stench], forward).
test(turns_toward_the_only_safe_square) :-
    % bump east: the only safe square left is north, one left turn away.
    fresh, step([], forward), step([bump], turnleft), step([], forward).
test(turns_right_when_that_is_shorter) :-
    % facing north at (1,1) with only (2,1) to visit -> one right turn.
    fresh, tell(action(turnleft)), tell(size(4,1)), step([], turnright).
test(with_gold_heads_home_not_exploring) :-
    fresh, step([], forward), step([glitter], grab),
    step([], turnleft), step([], turnleft), step([], forward), step([], climb).
test(nothing_left_to_explore_goes_home) :-
    tell(init), tell(size(2,1)),
    step([], forward), step([], turnleft), step([], turnleft),
    step([], forward), step([], climb).
test(agent_file_has_no_state) :-
    \+ ( current_predicate(agent:Name/Arity), functor(H, Name, Arity),
         predicate_property(agent:H, dynamic) ).

:- end_tests(agent).

%% -----------------------------------------------------------------------
:- begin_tests(agent_risk).

rstep(Names, Action) :-
    findall(V, ( member(S, [stench,breeze,glitter,bump,scream]),
                 ( memberchk(S, Names) -> V = S ; V = none ) ), P),
    agent_step(P, Action).

test(no_limit_never_gambles) :-
    fresh, rstep([breeze], climb).
test(gambles_when_risk_is_within_limit) :-
    fresh, tell(risk_limit(0.6)), rstep([breeze], forward).
test(does_not_gamble_when_risk_is_over_limit) :-
    fresh, tell(risk_limit(0.5)), rstep([breeze], climb).
test(safe_square_is_preferred_to_a_gamble) :-
    % (2,1) breezy; (1,2) is proven safe, so go back for it first.
    fresh, tell(risk_limit(1.0)), rstep([], forward), rstep([breeze], turnleft).
test(shooting_is_preferred_to_a_gamble) :-
    fresh, tell(risk_limit(1.0)), rstep([stench], shoot).
test(picks_the_least_risky_square) :-
    % at (1,2): (2,2) is 86%, (3,1) and (1,3) are 31%.  (1,3) is straight ahead.
    fresh, tell(risk_limit(0.35)), rstep([], forward),
    rstep([breeze], turnleft), rstep([breeze], turnleft), rstep([breeze], forward),
    rstep([], turnright), rstep([], forward), rstep([breeze], forward),
    ask(facing(north)).
test(with_gold_goes_home_instead_of_gambling) :-
    fresh, tell(risk_limit(1.0)), rstep([breeze,glitter], grab), rstep([breeze], climb).

:- end_tests(agent_risk).

%% -----------------------------------------------------------------------
:- begin_tests(world).

w(F, P) :- atom_concat('worlds/edge/', F, P).

test(percept_at_quiet_start) :-
    w('2x2_gold_diag.json', F), load_world(F),
    world_percept([none,none,none,none,none]).
test(stench_next_to_wumpus) :-
    w('2x2_stench_at_start.json', F), load_world(F),
    world_percept([stench,none,none,none,none]).
test(breeze_next_to_pit) :-
    w('2x2_breeze_at_start.json', F), load_world(F),
    world_percept([none,breeze,none,none,none]).
test(walking_into_wall_bumps_once) :-
    w('2x2_gold_diag.json', F), load_world(F),
    world_do(turnright), world_do(forward),
    world_percept([_,_,_,bump,_]), world_percept([_,_,_,none,_]),
    world_agent(1,1,south).
test(walking_into_pit_dies) :-
    w('2x2_breeze_at_start.json', F), load_world(F),
    world_do(forward), world_over(died), world_score(-1001).
test(walking_into_wumpus_dies) :-
    w('2x2_stench_at_start.json', F), load_world(F),
    world_do(turnleft), world_do(forward), world_over(died).
test(arrow_kills_and_screams_once) :-
    w('2x2_stench_at_start.json', F), load_world(F),
    world_do(turnleft), world_do(shoot),
    world_percept([_,_,_,_,scream]), world_percept([_,_,_,_,none]),
    world_do(forward), \+ world_over(_).
test(arrow_costs_ten_extra) :-
    w('2x2_gold_diag.json', F), load_world(F), world_do(shoot), world_score(-11).
test(second_shot_does_nothing) :-
    w('2x2_stench_at_start.json', F), load_world(F),
    world_do(shoot), world_do(turnleft), world_do(shoot),
    world_percept([_,_,_,_,none]).
test(climb_without_gold_just_leaves) :-
    w('2x2_gold_diag.json', F), load_world(F),
    world_do(climb), world_over(left), world_score(-1).

:- end_tests(world).

%% -----------------------------------------------------------------------
% The checker must notice when the KB believes something false.  Each test
% loads a world, then plants a false belief directly in the KB.
:- begin_tests(checker).

cw(F) :- atom_concat('worlds/edge/', F, P), load_world(P), fresh.
checks(Action, Names) :-
    check_before_action(Action, Vs),
    findall(C, member(violation(C,_), Vs), Cs), sort(Cs, Names).

test(honest_kb_passes) :-
    cw('2x2_gold_diag.json'), percept([]), checks(turnleft, []).
test(notices_a_pit_called_safe) :-
    % 2x2_breeze_at_start has a pit at (2,1)
    cw('2x2_breeze_at_start.json'), percept([breeze]),
    assertz(kb:safe(2,1)), checks(turnleft, [safe]).
test(notices_a_live_wumpus_called_safe) :-
    % 2x2_stench_at_start has the wumpus at (1,2)
    cw('2x2_stench_at_start.json'), percept([stench]),
    assertz(kb:safe(1,2)), checks(turnleft, [safe]).
test(notices_wrong_position) :-
    cw('2x2_gold_diag.json'), percept([]),
    retract(kb:at(1,1)), assertz(kb:at(2,2)), checks(turnleft, Names),
    memberchk(position, Names).
test(notices_walking_onto_an_unproven_square) :-
    cw('2x2_breeze_at_start.json'), percept([breeze]), checks(forward, [gamble]).
test(gamble_is_allowed_with_a_risk_limit) :-
    cw('2x2_breeze_at_start.json'), tell(risk_limit(0.6)), percept([breeze]),
    checks(forward, []).
test(notices_wrong_heading) :-
    cw('2x2_gold_diag.json'), percept([]),
    retract(kb:facing(east)), assertz(kb:facing(west)),
    check_after_action(Vs), memberchk(violation(facing,_), Vs).
test(episode_reports_no_violations_and_counts_work) :-
    atom_concat('worlds/edge/', 'open_10.json', P), load_world(P),
    episode([], result(won, 958, 42, Inferences, Facts, [])),
    Inferences > 0, Facts > 0.

:- end_tests(checker).

%% -----------------------------------------------------------------------
% Whole episodes.  expected(World, Outcome, Score): if the outcome is right
% but the score is off, look at routing/turn choice; if the outcome changed,
% look at inference or the decision order.
:- begin_tests(episodes).

expected('2x2_breeze_at_start',      left,   -1).
expected('2x2_gold_diag',            won,   991).
expected('2x2_pit_blocks',           left,   -1).
expected('2x2_stench_at_start',      won,   979).
expected('2x2_wumpus_guards_gold',   left,  -20).
expected('corridor_wumpus_at_end',   won,   977).
expected('gold_in_pit',              left,  -29).
expected('gold_under_wumpus',        won,   953).
expected('gold_walled_by_pits',      left,  -44).
expected('interior_walls',           won,   968).
expected('narrow_passage_10',        won,   796).
expected('no_gold',                  left,  -33).
expected('open_10',                  won,   958).
expected('pit_plugs_passage',        left,  -31).
expected('wumpus_blocks_corridor',   left,  -23).
expected('wumpus_in_pit',            won,   982).

test(edge_world, [forall(expected(Name, Why, Score)), true(Got == Why/Score)]) :-
    format(atom(F), 'worlds/edge/~w.json', [Name]),
    run(F, W, S), Got = W/S.

% The same, with gambling allowed: run_risky(World, Limit, Outcome, Score).
risky('2x2_pit_blocks',      0.6, won,   990).
risky('2x2_pit_blocks',      0.5, left,   -1).
risky('2x2_breeze_at_start', 0.6, died, -1001).

test(risky_world, [forall(risky(Name, Limit, Why, Score)), true(Got == Why/Score)]) :-
    format(atom(F), 'worlds/edge/~w.json', [Name]),
    load_world(F), tell(init), world_size(XM,YM), tell(size(XM,YM)),
    tell(risk_limit(Limit)),
    play(0), world_over(W), world_score(S), Got = W/S.

:- end_tests(episodes).
