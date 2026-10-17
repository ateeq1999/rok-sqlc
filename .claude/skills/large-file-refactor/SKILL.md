---
description: Audit and safely refactor source files exceeding 600 lines
  into cohesive, maintainable modules while preserving behavior. Use
  when asked to organize large files, modularize a codebase, or enforce
  a large-file refactoring policy.
name: large-file-refactor
---

# Large File Refactoring Skill

## Purpose

Audit the repository and reorganize eligible source files exceeding 600
lines into smaller, cohesive modules with clear responsibilities.
Prioritize maintainability, separation of concerns, testability, and
safe refactoring over merely reducing line counts.

Use the existing architecture and coding conventions of the repository.
Prefer clean architecture and feature-oriented modularization where
appropriate.

## When to Use

Use this skill when the user asks to:

-   Organize or split large source files.
-   Refactor files exceeding a line-count threshold.
-   Improve module boundaries or separation of concerns.
-   Audit a repository for oversized files and make a refactoring plan.
-   Apply a consistent large-file organization policy across a codebase.

## 1. Audit Before Editing

1.  Identify the repository root and inspect the workspace structure.
2.  Read relevant project manifests, module declarations, tests, and
    repository instructions.
3.  Recursively find eligible source files exceeding 600 lines.
4.  Include Rust source files (`.rs`) by default. Identify unusually
    large files in other languages, but refactor them only when the
    project architecture supports doing so safely.
5.  Exclude generated files, vendored dependencies, build artifacts, and
    third-party code unless explicitly requested.
6.  For each large file, identify:
    -   Its responsibilities and public API.
    -   Logical groups of types, functions, traits, and implementations.
    -   Dependencies and ownership relationships between those groups.
    -   Existing tests and behavioral guarantees.
    -   Suitable extraction targets and module boundaries.

Provide a concise audit report and proposed plan before editing. Then
proceed with routine, behavior-preserving extractions without waiting
for approval.

## 2. Choose Modules by Responsibility

Split files according to cohesive responsibilities and domain
boundaries, not arbitrary line ranges.

Possible Rust module boundaries include:

-   `pipeline.rs` --- orchestration and ordered processing.
-   `state.rs` --- shared internal state and context.
-   `types.rs` --- type definitions and conversions.
-   `names.rs` --- naming, identifier normalization, and lookup helpers.
-   `constraints.rs` --- constraint-related operations.
-   `relations.rs` --- relationships and cross-entity operations.
-   `table/` --- table construction, columns, and alterations.
-   `enums.rs` --- enum definitions and operations.
-   `diagnostics.rs` --- errors, warnings, and diagnostic reporting.
-   `tests/` --- tests grouped by behavior.

These are examples, not mandatory filenames. Choose names that
accurately represent the code being moved.

### Architecture Rules

-   Follow the repository's existing Rust module conventions, whether
    `mod.rs` or `module.rs` style.
-   Preserve public APIs and function signatures wherever possible.
-   Prefer the narrowest suitable visibility, such as `pub(super)` or
    `pub(crate)`, instead of exposing implementation details.
-   Avoid circular dependencies between modules.
-   Do not create a catch-all `utils.rs` or `helpers.rs` dumping ground.
-   Do not create one file per function or split code only to satisfy
    the line limit.
-   Keep related structs, traits, implementations, and tests together
    when that improves cohesion.
-   Extract shared state only when it simplifies ownership and
    dependencies.
-   Avoid unnecessary abstractions, traits, generics, and design
    patterns.
-   If a file is naturally cohesive and splitting it would make the code
    harder to understand, leave it intact and document the reason.

## 3. Preserve Behavior

Treat this as a structural refactor, not feature development.

-   Do not intentionally change business logic or application behavior.
-   Do not change database schemas, SQL migrations, serialized formats,
    or public interfaces unless a change is essential. Pause and explain
    before making a risky compatibility change.
-   Preserve error behavior, error types, and error messages where
    practical.
-   Preserve execution order wherever order affects results.
-   Preserve transaction boundaries, concurrency behavior, ownership
    semantics, and resource lifetimes.
-   Preserve feature-gated and platform-specific code.
-   Preserve important documentation, comments, attributes, and
    conditional compilation.
-   Do not remove tests or apparently unused code without verifying its
    purpose.
-   Do not add dependencies unless strictly necessary and justified.
-   Do not suppress compiler or Clippy warnings to hide refactoring
    problems.
-   When behavior is ambiguous, inspect callers and tests rather than
    guessing.

## 4. Refactor Incrementally

Work on one logical extraction at a time.

For each large file:

1.  Record its original line count and responsibilities.
2.  Run relevant tests before editing.
3.  Choose a small, logically complete group of items to extract.
4.  Move that group into its new module with minimal changes.
5.  Update imports, visibility, module declarations, and call sites.
6.  Run formatting, compilation, and relevant tests.
7.  Inspect the diff for unintended changes.
8.  Continue only after the current step passes.

Prefer relatively independent components first, such as naming helpers,
type conversions, diagnostics, or enum operations. Extract orchestration
and shared state after their dependencies are understood.

Avoid one enormous change spanning the entire repository when smaller,
verifiable changes are practical. Continue until the audit is complete
or a genuine blocker is reached.

## 5. Rust Verification

Inspect the workspace and toolchain configuration before choosing
commands. Typical checks are:

``` bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
```

Where appropriate, also run:

``` bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Do not assume every project supports every feature combination or
command. Run relevant examples, integration tests, and feature-specific
checks when practical.

-   Report environmental failures, missing services, and pre-existing
    failures accurately.
-   Do not claim a check passed unless it ran successfully.
-   Determine whether failures were introduced by the refactor.
-   Fix regressions caused by the changes before proceeding.
-   Do not weaken tests to make the refactor pass.

## 6. Line-Count Target

Aim for every eligible source file to contain 600 lines or fewer,
including comments and tests where applicable.

The threshold is a signal to review responsibility boundaries, not a
mandate to split cohesive code artificially. If a file should remain
above 600 lines, document why further splitting would reduce clarity or
increase coupling.

After refactoring, rescan the repository and identify any remaining
eligible files over the threshold.

## 7. Tests and Regression Protection

Preserve existing tests and add or reorganize tests only where needed to
protect behavior during extraction.

Organize tests around domain behavior rather than incidental
implementation details. Depending on the repository, relevant groups
might include:

-   Pipeline and parsing behavior.
-   Table and column operations.
-   Type conversions.
-   Enum handling.
-   Constraints and relationships.
-   Error handling and diagnostics.
-   Migration ordering and source locations.

Keep test assertions meaningful. Do not rewrite tests merely to
accommodate an avoidable architectural change.

## 8. Deliverables

At completion, report:

1.  **Audit summary:** every eligible file initially exceeding 600
    lines.
2.  **Before-and-after measurements:** original line count, resulting
    modules, and final line counts.
3.  **Final module trees:** show the resulting structure for each
    substantially reorganized area.
4.  **Changes made:** explain each extraction and its responsibility.
5.  **Verification results:** commands executed and their actual
    outcomes.
6.  **Remaining exceptions:** explain each eligible file still above 600
    lines.
7.  **Outstanding issues:** blocked work, unresolved risks, and
    pre-existing failures.

Use actual repository measurements. Never fabricate results.

A summary table can use this format:

  ----------------------------------------------------------------------------
  Original file            Original lines New structure     Final largest file
  ------------------ -------------------- --------------- --------------------
  `src/example.rs`           Actual count Actual module          Actual result
                                          structure       

  ----------------------------------------------------------------------------

## 9. Execution Policy

-   Begin by auditing the repository and presenting the proposed plan.
-   Then implement the refactoring; do not stop after producing a plan.
-   Continue through all eligible files unless a genuine blocker or
    risky compatibility decision requires a pause.
-   If a proposed change risks altering public APIs, database behavior,
    data compatibility, or another important contract, pause that
    specific change and explain the risk.
-   Review the final diff for accidental behavior changes.
-   Re-scan the repository and run the relevant verification suite
    before reporting completion.

## Definition of Success

The repository has a more cohesive module structure; every eligible
oversized file has been reviewed; files exceeding 600 lines have been
split where architecturally appropriate; existing behavior is preserved;
and verification results are accurately reported.
