; Pairs with the h3_stub_* trio. This is the stubbing lesson in 12 lines.
; Run: z3 hand/h3.smt2   ->   unsat, unsat, sat
(set-logic QF_BV)
(declare-const x (_ BitVec 8))

; h3_stub_none — the real body is in the formula. Masking twice is identity.
(push 1)
(assert (not (= (bvxor (bvxor x #x5a) #x5a) x)))
(check-sat)   ; unsat => proved
(pop 1)

; h3_stub_equivalent — `mask` replaced by the identity. Different formula,
; same verdict. This is a *sound* stub: it happens to preserve the property.
(push 1)
(assert (not (= x x)))
(check-sat)   ; unsat => proved
(pop 1)

; h3_stub_any_overapproximates — `mask` replaced by kani::any(). Each call
; became an unconstrained variable, so the two results are unrelated.
(push 1)
(declare-const m1 (_ BitVec 8))   ; result of the inner  mask(x)
(declare-const m2 (_ BitVec 8))   ; result of the outer  mask(m1)
(assert (not (= m2 x)))
(check-sat)   ; sat, trivially — which is exactly the complaint
(get-model)
(pop 1)

; The moral: a stub deletes a subformula. What replaces it is either equally
; strong (sound), or weaker (over-approximation: false positives, as here), or
; *stronger* — which is the dangerous one, because it makes proofs succeed for
; reasons the real code does not support. Kani will not check this for you.
