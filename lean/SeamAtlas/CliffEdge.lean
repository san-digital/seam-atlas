import SeamAtlas.PartitionSoundness

/-!
# Facts about chains of one-step changes

General lemmas behind the idea of a cliff edge (`src/cliffs.rs`). They hold for any step
relation `R` and say nothing about whether `find_cliffs` reports every edge.

1. Any walk `x_0, x_1, ..., x_k` between two inputs with different outcomes contains a
   step `(x_i, x_{i+1})` where the outcome changes.
2. Along a walk of `k >= 1` steps with `sev(end) - sev(start) >= k * delta`, some single
   step rises by at least `delta`.
3. If no input has a severity strictly between `low` and `high` (`high - low >= 2`), every
   walk from severity at most `low` to severity at least `high` has a step that rises by
   at least 2.
-/

namespace SeamAtlas

/-- Predicate asserting that consecutive elements of `x :: xs` satisfy binary relation `R`. -/
def IsChainStep {α : Type*} (R : α → α → Prop) : α → List α → Prop
  | _, [] => True
  | x, y :: ys => R x y ∧ IsChainStep R y ys

/-- Last element of a non-empty walk starting at `x` with tail `xs`. -/
def walkEnd {α : Type*} : α → List α → α
  | x, [] => x
  | _, y :: ys => walkEnd y ys

/-- Every walk between two inputs with distinct outcomes crosses an adjacent step `(u, v)`
where `eval u != eval v`. -/
theorem exists_outcome_flip_on_walk {α β : Type*} (R : α → α → Prop) (f : α → β) :
    ∀ (x : α) (xs : List α),
      IsChainStep R x xs →
      f x ≠ f (walkEnd x xs) →
      ∃ u v : α, R u v ∧ f u ≠ f v
  | x, [], _, hne => absurd rfl hne
  | x, y :: ys, ⟨hr, hchain⟩, hne => by
      by_cases hxy : f x = f y
      · rw [walkEnd, hxy] at hne
        exact exists_outcome_flip_on_walk R f y ys hchain hne
      · exact ⟨x, y, hr, hxy⟩

/-- If `sev(end) - sev(start) >= xs.length * delta` across a non-empty walk, some single
step rises in severity by at least `delta`. -/
theorem exists_large_severity_step {α : Type*} (R : α → α → Prop) (sev : α → ℤ) (delta : ℤ) :
    ∀ (x : α) (xs : List α),
      xs ≠ [] →
      IsChainStep R x xs →
      (xs.length : ℤ) * delta ≤ sev (walkEnd x xs) - sev x →
      ∃ u v : α, R u v ∧ delta ≤ sev v - sev u
  | _, [], hne, _, _ => absurd rfl hne
  | x, [y], _, ⟨hr, _⟩, hbound => by
      simp only [List.length_singleton, Nat.cast_one, one_mul, walkEnd] at hbound
      exact ⟨x, y, hr, hbound⟩
  | x, y :: z :: zs, _, ⟨hr, hchain⟩, hbound => by
      by_cases hstep : delta ≤ sev y - sev x
      · exact ⟨x, y, hr, hstep⟩
      · push Not at hstep
        have htail_bound : ((z :: zs).length : ℤ) * delta ≤ sev (walkEnd y (z :: zs)) - sev y := by
          simp only [List.length_cons, walkEnd] at hbound ⊢
          push_cast at hbound ⊢
          linarith
        exact exists_large_severity_step R sev delta y (z :: zs) (List.cons_ne_nil z zs) hchain htail_bound

/-- If no input has a severity strictly between `low` and `high` (`low + 2 <= high`), then
every walk from severity `<= low` to severity `>= high` has a single step that rises by
at least 2. -/
theorem unbuffered_cliff_of_missing_intermediate {α : Type*}
    (R : α → α → Prop) (sev : α → ℤ) {low high : ℤ}
    (hgap : low + 2 ≤ high)
    (hno_mid : ∀ z : α, ¬ (low < sev z ∧ sev z < high)) :
    ∀ (x : α) (xs : List α),
      IsChainStep R x xs →
      sev x ≤ low →
      high ≤ sev (walkEnd x xs) →
      ∃ u v : α, R u v ∧ 2 ≤ sev v - sev u
  | x, [], _, hlow, hhigh => by
      simp only [walkEnd] at hhigh
      omega
  | x, y :: ys, ⟨hr, hchain⟩, hlow, hhigh => by
      by_cases hy_low : sev y ≤ low
      · exact unbuffered_cliff_of_missing_intermediate R sev hgap hno_mid y ys hchain hy_low hhigh
      · have hy_high : high ≤ sev y := by
          by_contra hy_lt
          exact hno_mid y ⟨by omega, by omega⟩
        exact ⟨x, y, hr, by omega⟩

end SeamAtlas
