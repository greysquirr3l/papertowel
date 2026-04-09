# Git Hooks

papertowel can install a pre-commit hook that scans staged files before each commit. If AI fingerprints at medium severity or above are detected, the commit is blocked.

## Installing the hook

```bash
papertowel hook install
```

This writes a shell script to `.git/hooks/pre-commit`. If a hook already exists (e.g. from another tool), papertowel will refuse to overwrite it unless you pass `--force`.

## How it works

When you run `git commit`, the hook:

1. Collects the list of staged files (excluding deletions).
2. Extracts the staged version of each file into a temporary directory — this ensures the scan sees exactly what's being committed, not your working-tree edits.
3. Runs `papertowel scan <tmpdir> --fail-on medium`.
4. If any findings at medium severity or above are found, the commit is aborted with the scan output.
5. The temporary directory is cleaned up on exit regardless of outcome.

## Checking hook status

```bash
papertowel hook status
```

Reports whether the hook is installed and whether it was created by papertowel.

## Removing the hook

```bash
papertowel hook uninstall
```

This only removes hooks that papertowel installed. If the hook was placed by another tool, papertowel will refuse to touch it.

## Working with other hooks

If you use a hook manager like [lefthook](https://github.com/evilmartians/lefthook) or [husky](https://typicode.github.io/husky/), you can call papertowel directly from your hook config instead of using `hook install`. For example, in a lefthook config:

```yaml
pre-commit:
  commands:
    papertowel:
      run: papertowel scan {staged_files} --fail-on medium
```

## Bypassing the hook

If you need to commit despite findings (e.g. you're committing test fixtures that intentionally contain slop vocabulary), use git's built-in bypass:

```bash
git commit --no-verify
```
