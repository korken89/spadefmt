# Known-bad corpus

Every pair here pins the CURRENT, WRONG output of spadefmt: `X.spade` is an
input whose formatting is a known defect and `X-formatted.spade` is what
spadefmt prints for it today. `tests/golden.rs` checks these pairs (golden and
idempotency) exactly like the top-level ones, so a defect cannot drift
unnoticed and a fix has to consciously promote the pair.

Bless rule: `SPADEFMT_BLESS=1` never rewrites a golden in this directory. A
changed output fails the golden test until either

- the output is now correct: promote the pair (move both files up to `asts/`,
  delete its row below, and if the cause is `ast`, tick the matching
  AST-fidelity item in PLAN.md's upstream tracker), or
- the output is still wrong: re-pin it with
  `SPADEFMT_BLESS_KNOWN_BAD=1 cargo test`.

Cause classes: `spadefmt` is fixable in this repo; `ast` means the spade AST
does not record the source distinction, so a lossless fix needs an upstream
AST change (tracked as AST-fidelity candidates in PLAN.md).

| Fixture | What is wrong | Cause | Correct output |
|---------|---------------|-------|----------------|
| `reorder_blank` | The assoc-types-first reordering of trait/impl bodies runs the blank-line gap check on reordered neighbors: `type A; fn m(..); type B;` with no source blanks prints a blank between `type A;` and `type B;`. | `spadefmt` | No blank is invented: `type A;`, `type B;`, `fn m(self) -> bool;` on consecutive lines. |
| `arm_width` | Lines exceed `max_width` (80) in two ways: once a match arm's call body breaks, the flat `pattern => callee(` head line is never judged against the width (81 columns here; it grows with the pattern instead of breaking it), and a `match` that is a statement's value is judged two columns short (a flat 82-column statement line stays flat; 83 breaks). | `spadefmt` | Nothing past 80 columns: the arm's pattern tuple breaks one element per line, and the second `match` stays broken one arm per line. |
| `multiline_string_macro` | A string literal holding a raw newline inside a macro call is re-indented like code: the continuation line `line two"` gains eight leading spaces, changing the string's bytes. Macro slices print as dedented `Raw` (a verbatim slice would freeze its interior lines at source columns while the head moves with the layout), so a string inside one does not get the verbatim path plain string literals have. | `spadefmt` | The string's lines are byte-identical to the source (`line two"` stays at column 0); only the code around the macro call moves. |
| `trait_member_order` | Trait bodies print associated types before methods regardless of source order: `fn m(..); type A;` becomes `type A; fn m(..);`. | `ast` | Members keep their source order. |
| `use_nesting` | Nested `use` brace trees flatten to one brace level over the common prefix: `use a::{b::{c, d}, e};` becomes `use a::{b::c, b::d, e};`. | `ast` | The source nesting is kept. |
| `backtick_infix` | The backtick infix-call spelling desugars to a plain call: `` x `g` y `` becomes `g(x, y)`. | `ast` | The infix spelling is kept. |
| `lambda_braces` | A statement-less lambda body loses its braces: `fn \|x\| { x }` becomes `fn \|x\| x`. | `ast` | The braces are kept: `let l = fn \|x\| { x };`. |
| `gen_if_empty_else` | An explicit empty `else {}` on a `gen if` is dropped; the AST stores a missing else as the same empty block. | `ast` | `else {}` is kept. |
