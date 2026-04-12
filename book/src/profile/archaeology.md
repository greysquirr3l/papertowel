# Archaeology and Entropy

If the Wringer only cherry-picked clean commits, the history would still look "too perfect." Real human development is iterative, messy, and often involves a series of failed attempts before the final solution is reached.

**Archaeology** is the process of injecting synthetic "messy middle" artifacts into the git history to simulate this evolution.

## The Concept: Net-Zero Injections

The core challenge of Archaeology is to add "human" noise without actually changing the final state of the code. If you inject a TODO and never remove it, the code is now different.

The Wringer solves this by using **Injection Pairs**. Every archaeological artifact is added in one commit and removed in another.

### The TODO Cycle

The Wringer randomly selects a `.rs` file and injects a plausible TODO comment:

1. **Commit A**: Adds `// TODO: review error handling here` to `src/lib.rs`.
2. **Commit B**: Removes the comment.

The net effect on the working tree is zero, but the git log now shows a human developer thinking through the problem and then resolving it.

### The Dead-Code Cycle

Similarly, the Wringer can simulate "scratch work":

1. **Commit A**: Adds a commented-out `eprintln!` or a temporary variable used for debugging.
2. **Commit B**: Removes the "dead code" before the final "clean" commit is applied.

## Configuration

Archaeology is controlled via the `[persona.archaeology]` section of the persona profile:

- `todo_inject_rate`: Probability of a TODO pair being injected before a real commit.
- `dead_code_rate`: Probability of a dead-code pair being injected.
- `rename_chains`: When enabled, the Wringer can simulate a series of renames for a single function across several commits, simulating the evolution of an API.

## Why This Matters

To a forensic analyst, the presence of "resolved" TODOs and "cleaned up" debug statements is a powerful signal of human authorship. It demonstrates a process of iteration and refinement that LLMs (which typically output the final version of a function in one go) cannot naturally replicate.
