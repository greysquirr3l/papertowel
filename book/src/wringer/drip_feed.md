# The Drip-Feed Mechanism

The "Drip" is the execution engine of the Wringer. It transforms a static Replay Plan into a living git history.

## How the Drip Works

The `wring drip` command is essentially a scheduler. It monitors the `QueuePlan` (stored in `.papertowel/queue.json`) and compares the `target_time` of pending entries against the current wall-clock time.

### The Tick Cycle

When the Wringer "ticks" (either once or as a daemon):

1. It loads the current queue.
2. It identifies all entries where `target_time <= now`.
3. For each eligible entry:
    - It optionally injects **Archaeological artifacts** (see [Archaeology](#)).
    - It cherry-picks the source commits from the private branch.
    - It creates a new commit on the public branch with the persona-driven message and the exact `target_time` as the timestamp.
    - It marks the entry as `completed`.
4. It persists the updated queue.

## Humanizing the Commits

The Drip doesn't just move code; it translates the *intent* of the commits into a human style.

### Message Humanization

Using the `messages` subsystem, the Wringer transforms the original commit messages. Depending on the persona, it might:

- Convert a "feat: add login" into a "Conventional" commit: `feat(auth): implement login logic`.
- Convert it into a "Lazy" commit: `fix stuff`.
- Inject "Entropy": Add typos, mild profanity, or ASCII emojis (`:)`) based on the persona's specific rates.

### Temporal Jitter

To avoid the "robotic" feel of commits appearing at exactly 10:00, 10:15, and 10:30, the Wringer applies **Jitter**. It varies the intervals between commits within a session, simulating the natural ebb and flow of human productivity.

## Operational Modes

### Single-Shot

Running `papertowel wring drip` without the `--daemon` flag applies all currently due commits and then exits. This is useful for manual synchronization.

### Daemon Mode

Running with `--daemon` puts the Wringer into the background. It will wake up periodically, check the schedule, and "drip" commits exactly when they are supposed to appear, providing a real-time humanization of your project's growth.
