# Configuring Personas

The behavior of the Wringer is governed by **Persona Profiles**. A profile is a TOML configuration that defines the "human" identity of the project's author.

## The Persona Profile

A profile determines not just *when* code is committed, but *how* those commits are presented.

### Schedule Configuration

The `[persona.schedule]` section defines the working rhythm of the author:

- `active_hours`: A list of time windows (e.g., `["09:00-17:00", "21:00-01:00"]`) when the author is active.
- `peak_productivity`: The window where commits are most frequent.
- `avg_commits_per_session`: How many commits typically occur in a single work session.
- `session_variance`: The amount of randomness applied to session lengths and commit intervals.

### Message Style

The `[persona.messages]` section defines the commit message aesthetic:

- `style`:
    `conventional`: Uses the Conventional Commits specification (`feat(scope): description`).
    `lazy`: Uses shorthand, lowercase, and vague messages (`wip`, `fix stuff`).
    `mixed`: A probabilistic blend of both.
- `wip_frequency`: How often a commit is replaced by a "Work In Progress" message.
- `typo_rate`: The probability that a commit message contains a simulated typo.
- `profanity_frequency`: The probability of injecting mild frustration (`ugh`, `damn`).
- `emoji_rate`: The probability of adding an ASCII emoji (`:)`, `:/`).

## Built-in Profiles

`papertowel` ships with two standard profiles:

1. **`night-owl`**: High activity in the late evening and early morning, mixed commit styles, and a higher propensity for "lazy" messages and typos.
2. **`nine-to-five`**: Strict daytime activity, highly conventional commit styles, and very low entropy.

## Creating Custom Profiles

You can create your own persona using the interactive builder:

```bash
papertowel profile create <name>
```

Custom profiles are stored in `~/.config/papertowel/profiles/*.toml`.

## Managing Profiles

- **List all profiles**: `papertowel profile list`
- **Inspect a profile**: `papertowel profile show <name>`
