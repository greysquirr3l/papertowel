# The Public Branch Strategy

The fundamental design goal of the Wringer is to avoid the "forensic footprint" of git history rewriting.

## Why not just rebase?

When you rewrite history using `git rebase` or `git filter-branch`, you are creating new commit objects. While this looks clean on the surface, it creates several problems:

1. **Reflog Traces**: Local reflogs still contain the original commits.
2. **Object Database**: In some environments, "orphaned" commits can be recovered from the git object database for a period of time.
3. **Push Friction**: You have to force-push the entire history, which is a red flag in any shared repository.

## The Worktree Solution

The Wringer avoids all of this by treating the `public` branch as a separate entity.

### The Workflow

1. **Development**: You work on `main` or `dev`. You commit often, you make mistakes, you use `wip` messages. This is your "True History."
2. **The Mirror**: The Wringer manages a second branch (e.g., `public`). This branch is purely for consumption by others.
3. **The Transfer**: The Wringer cherry-picks logic from the private branch to the public branch.

Because the public branch is built from the ground up, its history is "native." There are no rewritten commits, only real commits applied at specific times.

## Forensic Indistinguishability

By combining this strategy with **Persona Profiles** and **Archaeology**, the Wringer ensures that the public branch is not just a cleaned-up version of the private one, but a plausible alternative history.

To an outside observer (or a forensic tool), the public branch looks like the only history that ever existed: a series of organic, human-paced updates.
