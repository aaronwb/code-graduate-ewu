/*  sweep.pl -- plays EVERY possible small world, so nothing can hide.

    swipl src/sweep.pl -- [--blind] [--risk=T] SIZE [SIZE ...]      e.g. 2 3

    For an NxN cave: every subset of the non-start squares as pits, times
    every wumpus square, times every gold square.
        2x2:     72 worlds        3x3: 16,384 worlds

    One line per world, same fields as main.pl.  A world is named
        NxN/p<pits>/w<wumpus>/g<gold>
    where <pits> is one digit (0/1) per non-start square and <wumpus>,
    <gold> are positions in that same list.  Squares are listed column by
    column: (1,2) (1,3) ... (2,1) (2,2) ...
*/
:- use_module(world).
:- use_module(episode).

:- initialization(main, main).

main(Argv) :-
    partition([A]>>sub_atom(A,0,2,_,'--'), Argv, Flags, Sizes),
    findall(O, ( member(F, Flags), option(F, O) ), Opts),
    forall(( member(SA, Sizes), atom_number(SA, N) ), sweep(N, Opts)).

option('--blind', blind).
option(Flag, risk(T)) :- atom_concat('--risk=', A, Flag), atom_number(A, T).

sweep(N, Opts) :-
    findall(X-Y, ( between(1,N,X), between(1,N,Y), \+ (X == 1, Y == 1) ), Squares),
    W is N+2,
    forall(( pits(Squares, Bits, Pits),
             nth1(WI, Squares, WX-WY),
             nth1(GI, Squares, GX-GY) ),
           ( findall(thing(pit,PX,PY), member(PX-PY, Pits), PitThings),
             set_world(W, W, [thing(wumpus,WX,WY), thing(gold,GX,GY) | PitThings]),
             episode(Opts, result(Why, Score, Steps, Inf, Facts, Vs)),
             length(Vs, NV), ( NV =:= 0 -> Check = ok ; Check = NV ),
             atomic_list_concat(Bits, BitAtom),
             format("~dx~d/p~w/w~d/g~d: ~w  score=~d  actions=~d  inferences=~d  facts=~d  check=~w~n",
                    [N, N, BitAtom, WI, GI, Why, Score, Steps, Inf, Facts, Check]) )).

% pits(+Squares, -Bits, -Pits): every subset of Squares.
pits([], [], []).
pits([S|Ss], [B|Bs], Pits) :-
    pits(Ss, Bs, Pits0),
    ( B = 0, Pits = Pits0 ; B = 1, Pits = [S|Pits0] ).
