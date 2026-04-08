# The Wringer

The Wringer is `papertowel`'s system for git history humanization. While the Scrubber cleans the code, the Wringer cleans the *provenance*.

## Overview

A perfectly clean git history—where every commit is a logical unit of work, devoid of mistakes, and applied at a steady cadence—is a strong signal of AI generation (or an obsessively clean rebase, which is similarly suspicious).

The Wringer replaces "perfect" history with "human" history.

### The Core Mechanism: Worktree Drip-Feeding

Most history-humanizers attempt to rewrite the existing git log using `filter-branch` or `rebase`. This is dangerous and leaves forensic traces in the git object database.

**The Wringer takes a different approach.** It uses git worktrees to maintain a parallel "public" version of your project.

1. **Private Branch**: You continue developing on your private branch, committing at "machine speed" with whatever messages you like.
2. **Public Worktree**: The Wringer creates a separate worktree on a `public` branch.
3. **The Queue**: It analyzes the delta between your private and public branches and builds a **Replay Plan**.
4. **The Drip**: Instead of applying all changes at once, the Wringer "drips" commits into the public worktree over time, based on a **Persona Profile**.

## The Replay Plan

When you run `papertowel wring queue`, the tool doesn't just copy commits. It analyzes them to create a realistic human flow:

### Session Grouping
Humans don't commit every 5 minutes for 24 hours. They work in bursts. The Wringer groups commits into "sessions" based on temporal proximity.

### Intelligent Squashing and Splitting
- **Squashing**: Small, related commits (e.g., fixing a typo in the same file) are squashed into a single "human" commit.
- **Splitting**: Massive commits that touch unrelated parts of the codebase are flagged as candidates for splitting, simulating the process of a human breaking down a large task.

### Target Scheduling
Each commit in the queue is assigned a `target_time`. This time is calculated based on the Persona's active hours and productivity peaks, ensuring that the public history looks like it was written by someone with a life, a timezone, and a sleep schedule.

## Using the Wringer

### Setup
Initialize the public worktree:
```bash
papertowel wring init --branch public
```

### Planning
Analyze your dev branch and build the replay queue:
```bash
papertowel wring queue --from dev
```

### Execution
Start the drip-feed process. You can run it as a one-off or as a daemon that applies commits as their `target_time` arrives:
```bash
papertowel wring drip --daemon --profile night-owl
```

### Status
Check the current progress of the drip-feed:
```bash
papertowel wring status
```
