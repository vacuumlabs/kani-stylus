; Pairs with h1_nondet. The clean "unsat = proved" form of the same property.
; CBMC's dump is 88 lines; this is the 4 lines that carry the meaning.
;
; Run: z3 hand/h1.smt2   ->   unsat
(set-logic QF_BV)
(declare-const a (_ BitVec 8))   ; kani::any::<u8>()
(assert (bvult a (_ bv10 8)))    ; kani::assume(a < 10)
(assert (not (bvult a (_ bv10 8))))  ; the NEGATED assert!(a < 10)
(check-sat)                      ; unsat => no input violates it => proved
