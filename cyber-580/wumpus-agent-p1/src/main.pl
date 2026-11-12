/*  main.pl -- runs the agent in one or more world files.

    swipl src/main.pl -- [options] world.json [more.json ...]

      --trace   print every percept and action
      --map     also draw what the agent knows after every step
      --blind   do not tell the agent the cave size (it finds walls by bumping)
      --risk=T  let the agent gamble when nothing is provably safe, on squares
                whose estimated death risk is at most T (0..1).  Default: never.

    One result line per world:
      file: outcome  score=  actions=  inferences=  facts=  check=
    inferences = work done by the agent and KB (same on every machine),
    facts = size of the KB at the end, check = ok or the number of failed
    soundness checks (details are printed as they happen).
*/
:- use_module(world).
:- use_module(episode).

:- initialization(main, main).

main(Argv) :-
    partition([A]>>sub_atom(A,0,2,_,'--'), Argv, Flags, Files),
    (   Files == []
    ->  format("usage: swipl src/main.pl -- [--trace] [--map] [--blind] [--risk=T] world.json ...~n")
    ;   findall(O, ( member(F, Flags), option(F, O) ), Opts),
        forall(member(File, Files), run_world(File, Opts))
    ).

option('--trace', trace).
option('--map',   map).
option('--blind', blind).
option(Flag, risk(T)) :- atom_concat('--risk=', A, Flag), atom_number(A, T).

run_world(File, Opts) :-
    load_world(File),
    episode(Opts, result(Why, Score, Steps, Inferences, Facts, Violations)),
    length(Violations, NV),
    ( NV =:= 0 -> Check = ok ; Check = NV ),
    format("~w: ~w  score=~d  actions=~d  inferences=~d  facts=~d  check=~w~n",
           [File, Why, Score, Steps, Inferences, Facts, Check]).
