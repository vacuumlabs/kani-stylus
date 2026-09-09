; Pairs with h2_add_guarded, and shows why that harness has *two* properties.
; Run: z3 hand/h2.smt2   ->   unsat, unsat, sat
(set-logic QF_BV)
(declare-const a (_ BitVec 8))
(declare-const b (_ BitVec 8))

; Property 1 of 2 — the panic branch rustc inserts for `a + b` under
; -C overflow-checks=on. Guarded by the assume, it cannot fire.
(push 1)
(assert (bvule a (_ bv100 8)))
(assert (bvule b (_ bv100 8)))
(assert (bvugt (bvadd a b) (_ bv255 8)))  ; unrepresentable in 8 bits
(check-sat)   ; unsat — but see the note below
(pop 1)

; Property 2 of 2 — the assert!(sum >= a), negated.
(push 1)
(assert (bvule a (_ bv100 8)))
(assert (bvule b (_ bv100 8)))
(assert (bvult (bvadd a b) a))
(check-sat)   ; unsat => proved
(pop 1)

; NOTE on property 1: it is unsat for a boring reason. `bvugt x #xff` is
; unsatisfiable for *any* 8-bit x, because bvadd already wrapped. You cannot
; state "overflowed" inside the width you overflowed out of — you have to widen
; first. That is the whole reason CBMC/Kani insert an explicit check instead of
; trusting the arithmetic, and the same trap as the KB's U256 rule.
;
; Done properly, in 16 bits — and now it is reachable, so drop the assume:
(push 1)
(declare-const a16 (_ BitVec 16))
(declare-const b16 (_ BitVec 16))
(assert (bvule a16 (_ bv255 16)))
(assert (bvule b16 (_ bv255 16)))
(assert (bvugt (bvadd a16 b16) (_ bv255 16)))
(check-sat)   ; sat => u8 addition really can overflow, given the chance
(get-model)
(pop 1)
