; SMT-LIB 2, the input language CBMC uses when you point it at Z3.
; Run:  z3 u256.smt2
;
; QF_BV = quantifier-free bitvectors. This is the theory Stylus proofs live in:
; a U256 is literally (_ BitVec 256), and alloy's `+` is bvadd, which wraps.

(set-logic QF_BV)
(declare-const a (_ BitVec 256))
(declare-const b (_ BitVec 256))

; Query 1 — is wrapping overflow reachable? Ask for a witness.
; Expect: sat, plus a model. This is a Kani counterexample in miniature.
(push 1)
(assert (bvult (bvadd a b) a))
(check-sat)
(get-model)
(pop 1)

; Query 2 — prove a claim by asserting its negation.
; Claim: if a + b wraps below a, then b was nonzero.
; Expect: unsat  =>  no counterexample exists  =>  claim holds.
(push 1)
(assert (bvult (bvadd a b) a))
(assert (= b (_ bv0 256)))
(check-sat)
(pop 1)
