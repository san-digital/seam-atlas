import Mathlib.Data.Set.Basic
import Mathlib.Tactic

/-!
# Checking one point per cell decides a diff

An abstract model of the method behind `src/partition.rs` and `src/diff.rs`. It does not
model the Rust code itself (see PROOF.md).

1. A `Pipeline` tries an ordered list of rules, each a list of predicates
   (`sem : Pred -> Input -> Bool`), and returns the first match or `defaultOutcome`.
2. On any set `C` where every predicate the pipeline uses is constant (`CellConstant`),
   `Pipeline.eval` gives the same result for all `x, y in C`.
3. So for a finite cover of a domain `D` by such cells, each with a chosen point `cr.2`,
   two pipelines agree on all of `D` exactly when they agree on every chosen point, and
   any disagreement shows up at some chosen point.
-/

namespace SeamAtlas

/-- Conjunctive rule over atomic predicate identifiers `π` producing outcome `β`. -/
structure Rule (π β : Type*) where
  whenAll : List π
  outcome : β

/-- Priority-ordered first-match rule pipeline with fallback `defaultOutcome`. -/
structure Pipeline (π β : Type*) where
  rules : List (Rule π β)
  defaultOutcome : β

/-- Evaluates whether all predicates in `r.whenAll` hold at `x`. -/
def Rule.matches {π β α : Type*} (sem : π → α → Bool) (r : Rule π β) (x : α) : Bool :=
  r.whenAll.all (fun p => sem p x)

/-- First-match evaluation of `rules`, falling back to `defaultOutcome`. -/
def evalRules {π β α : Type*} (sem : π → α → Bool) : List (Rule π β) → β → α → β
  | [], defOut, _ => defOut
  | r :: rs, defOut, x =>
      if r.matches sem x then r.outcome else evalRules sem rs defOut x

/-- Evaluates `pl` at input `x`. -/
def Pipeline.eval {π β α : Type*} (sem : π → α → Bool) (pl : Pipeline π β) (x : α) : β :=
  evalRules sem pl.rules pl.defaultOutcome x

/-- Collects all atomic predicates referenced anywhere in a rule list. -/
def rulesPreds {π β : Type*} : List (Rule π β) → List π
  | [] => []
  | r :: rs => r.whenAll ++ rulesPreds rs

/-- Collects all atomic predicates referenced in `pl`. -/
def Pipeline.preds {π β : Type*} (pl : Pipeline π β) : List π :=
  rulesPreds pl.rules

/-- Every predicate in `ps` takes a constant truth value across `C`. -/
def CellConstant {π α : Type*} (sem : π → α → Bool) (ps : List π) (C : Set α) : Prop :=
  ∀ p ∈ ps, ∀ x ∈ C, ∀ y ∈ C, sem p x = sem p y

private theorem list_all_eq_on_cell {π α : Type*}
    (sem : π → α → Bool) (ps : List π) {C : Set α}
    (hconst : CellConstant sem ps C)
    {x y : α} (hx : x ∈ C) (hy : y ∈ C) :
    ps.all (fun p => sem p x) = ps.all (fun p => sem p y) := by
  induction ps with
  | nil => rfl
  | cons p tail ih =>
      simp only [List.all_cons]
      have hp : sem p x = sem p y := hconst p List.mem_cons_self x hx y hy
      have htail : tail.all (fun q => sem q x) = tail.all (fun q => sem q y) :=
        ih (fun q hq u hu v hv => hconst q (List.mem_cons_of_mem p hq) u hu v hv)
      rw [hp, htail]

theorem rule_matches_eq_on_cell {π β α : Type*}
    (sem : π → α → Bool) (r : Rule π β) {C : Set α}
    (hconst : CellConstant sem r.whenAll C)
    {x y : α} (hx : x ∈ C) (hy : y ∈ C) :
    r.matches sem x = r.matches sem y :=
  list_all_eq_on_cell sem r.whenAll hconst hx hy

theorem evalRules_eq_on_cell {π β α : Type*}
    (sem : π → α → Bool) (rs : List (Rule π β)) (defOut : β) {C : Set α}
    (hconst : CellConstant sem (rulesPreds rs) C)
    {x y : α} (hx : x ∈ C) (hy : y ∈ C) :
    evalRules sem rs defOut x = evalRules sem rs defOut y := by
  induction rs with
  | nil => rfl
  | cons r tail ih =>
      simp only [evalRules]
      have hr_const : CellConstant sem r.whenAll C := by
        intro p hp u hu v hv
        exact hconst p (List.mem_append_left _ hp) u hu v hv
      have htail_const : CellConstant sem (rulesPreds tail) C := by
        intro p hp u hu v hv
        exact hconst p (List.mem_append_right _ hp) u hu v hv
      rw [rule_matches_eq_on_cell sem r hr_const hx hy]
      by_cases hm : r.matches sem y
      · simp [hm]
      · simp [hm, ih htail_const]

/-- Any two inputs in a predicate-constant cell `C` get the same pipeline output. -/
theorem pipeline_eval_eq_on_cell {π β α : Type*}
    (sem : π → α → Bool) (pl : Pipeline π β) {C : Set α}
    (hconst : CellConstant sem pl.preds C)
    {x y : α} (hx : x ∈ C) (hy : y ∈ C) :
    pl.eval sem x = pl.eval sem y :=
  evalRules_eq_on_cell sem pl.rules pl.defaultOutcome hconst hx hy

/-- A pointed cover of domain `D` consists of cells `(C, rep)` with `rep in C subseteq D`
whose union contains every `x in D`. -/
structure PointedCover (α : Type*) (D : Set α) where
  cells : List (Set α × α)
  rep_mem : ∀ cr ∈ cells, cr.2 ∈ cr.1
  cell_sub : ∀ cr ∈ cells, cr.1 ⊆ D
  covers : ∀ x ∈ D, ∃ cr ∈ cells, x ∈ cr.1

/-- `old` and `new` agree on every point of `D` if and only if they agree on the chosen
point `cr.2` of every cell in a predicate-constant pointed cover. -/
theorem diff_sound_and_complete {π β α : Type*}
    (sem : π → α → Bool) (old new : Pipeline π β) {D : Set α}
    (cover : PointedCover α D)
    (hconst_old : ∀ cr ∈ cover.cells, CellConstant sem old.preds cr.1)
    (hconst_new : ∀ cr ∈ cover.cells, CellConstant sem new.preds cr.1) :
    (∀ x ∈ D, old.eval sem x = new.eval sem x) ↔
      (∀ cr ∈ cover.cells, old.eval sem cr.2 = new.eval sem cr.2) := by
  constructor
  · intro hall cr hcr
    exact hall cr.2 (cover.cell_sub cr hcr (cover.rep_mem cr hcr))
  · intro hreps x hx
    obtain ⟨cr, hcr_mem, hx_mem⟩ := cover.covers x hx
    have hrep_in : cr.2 ∈ cr.1 := cover.rep_mem cr hcr_mem
    have hold_eq := pipeline_eval_eq_on_cell sem old (hconst_old cr hcr_mem) hx_mem hrep_in
    have hnew_eq := pipeline_eval_eq_on_cell sem new (hconst_new cr hcr_mem) hx_mem hrep_in
    rw [hold_eq, hnew_eq]
    exact hreps cr hcr_mem

/-- If `old` and `new` differ anywhere on `D`, they differ at the chosen point `cr.2` of
some cell. -/
theorem diff_witness_complete {π β α : Type*}
    (sem : π → α → Bool) (old new : Pipeline π β) {D : Set α}
    (cover : PointedCover α D)
    (hconst_old : ∀ cr ∈ cover.cells, CellConstant sem old.preds cr.1)
    (hconst_new : ∀ cr ∈ cover.cells, CellConstant sem new.preds cr.1)
    {x : α} (hx : x ∈ D) (hne : old.eval sem x ≠ new.eval sem x) :
    ∃ cr ∈ cover.cells, cr.2 ∈ D ∧ old.eval sem cr.2 ≠ new.eval sem cr.2 := by
  obtain ⟨cr, hcr_mem, hx_mem⟩ := cover.covers x hx
  have hrep_in : cr.2 ∈ cr.1 := cover.rep_mem cr hcr_mem
  have hrep_D : cr.2 ∈ D := cover.cell_sub cr hcr_mem hrep_in
  refine ⟨cr, hcr_mem, hrep_D, ?_⟩
  intro heq
  have hold_eq := pipeline_eval_eq_on_cell sem old (hconst_old cr hcr_mem) hx_mem hrep_in
  have hnew_eq := pipeline_eval_eq_on_cell sem new (hconst_new cr hcr_mem) hx_mem hrep_in
  apply hne
  rw [hold_eq, hnew_eq, heq]

end SeamAtlas
