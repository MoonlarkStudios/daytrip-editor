# Yrs pending-update repair

Reviewed on 2026-10-25 with Yrs 0.28.0, Rust 1.95.0 and macOS/aarch64.
The publication audit exposed an intermittent pending-update assertion in
`types::array::test::fuzzy_test_300`. The same failure occurred in two of five
pristine upstream runs; [original diagnostics](validation/repository/yrs-array.txt)
remain as historical evidence. A seed alone did not reproduce it because the
test harness also uses randomly ordered hash maps.

## Cause and repair

Yrs can integrate independent operations past a missing clock range. If a later
operation depends on that gap, the original code recorded the client's highest
clock as its retry threshold. Receiving the missing earlier operation fills the
gap without advancing that high clock. The dependent operation then remains
pending even though its dependency is present.

The [vendor patch](../vendor/yrs.patch) records the exact missing operation ID
and tests its presence when deciding whether to retry. This covers origins,
parent items and optional weak references. Further unresolved dependencies are
checked again during integration. Public method signatures, update V1/V2 encoding,
text indexing and garbage collection policy stay compatible. The cached UTF-16
length optimization remains separate in the same upstream patch.

## Regression and validation

The deterministic regression creates three transactions: `a😀` in one shared
text, `z` in a second independent text, then `b` appended to the first. Delivering
the independent transaction and append before the first transaction reproduces
the stranded pending update. The test checks both update formats, all six delivery
orders, duplicate delivery while pending, final text and the converged state vector.

Run it with:

```sh
mise exec -- cargo test --manifest-path vendor/yrs/Cargo.toml --locked --features sync --lib pending_dependency_retries_after_filling_an_earlier_clock_hole
mise run check-yrs
```

The same final regression [fails without the retry repair](validation/dependencies/yrs-regression-before.txt)
and passes with it. [Ten consecutive suite runs](validation/dependencies/yrs-fixed-runs.txt)
each passed **379 tests**, including `fuzzy_test_300`, with one upstream ignored
test and the same seven excluded tests whose external assets are absent from the
crate. The regression also passed with `sync,weak`. No array test or assertion
was disabled. This combines a deterministic causal reproduction with repeated
randomized coverage; it is not proof of every possible CRDT interleaving.

The application restricts imported updates to bounded plain-text state, so this
generic upstream reproduction does not establish a user-visible data-loss case
in Dosty. Keep the regression when rebasing Yrs and remove the local retry patch
when a compatible upstream release includes the repair.

A final clean-build verification repeated all ten full-suite runs successfully
following the identity cleanup. The intentionally unfixed reproduction must use
its own Cargo target directory: sharing the dependency target directory between
the two same-version source trees reused the reproduction executable in one
intermediate check. Its stack trace identified the temporary unpatched source.
Cleaning the vendor artifacts and rebuilding from the patched source removed
that validation-cache collision; the clean-run results are retained above.
