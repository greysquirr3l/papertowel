# AI Coding Agent Instructions

These instructions apply to all AI coding assistant interactions in this repository.

## Core Standards

- Preserve current behavior and public APIs unless a change request explicitly allows breakage.
- Make changes in small, testable increments.
- Run the project's test and lint commands before finalizing significant refactors.

## God File Refactors

When asked to split or dismantle a "god file", follow the workflow in
[.papertowel/god-is-dead.md](.papertowel/god-is-dead.md).

Required process:

1. Discovery and architecture read first.
2. Score candidate files with explicit rationale.
3. Present an architecture-aware step plan.
4. Ask for explicit approval before any edits.
5. Execute only approved steps incrementally, with validation after each step.

Do not perform broad one-shot rewrites for god file work.

## Branch Discipline for Large Refactors

- Confirm current branch before edits.
- Prefer a scoped refactor branch name for god-file decomposition work (for example
  `refactor/godfile-<area>-split`).
- If branch scope is unclear, propose a better branch and ask before switching.
