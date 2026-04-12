# Style Baselines

One of the most challenging aspects of humanizing code is that "human style" varies wildly between individuals and teams. A "night-owl" profile might be perfect for one developer, but look suspicious for another.

## The Learning Mode

To solve this, `papertowel` includes a **Learn Mode**. Instead of relying on a pre-defined persona, Learn Mode allows the tool to analyze *your* actual existing git history to create a custom style baseline.

### How it Works

When you run `papertowel learn repo <path>`, the tool performs a deep analysis of your recent commits:

1. **Temporal Analysis**: It maps out your actual active hours, productivity peaks, and session gaps.
2. **Lexical Analysis**: It identifies the words and phrases you actually use in your commit messages.
3. **Entropy Analysis**: It calculates your natural typo rate and your frequency of "wip" or "fix" commits.

### Creating a Baseline

The resulting analysis is stored as a **Style Baseline**. This baseline acts as a highly accurate Persona Profile that mirrors your own coding habits.

When you then run `wring drip`, the Wringer uses this baseline instead of a generic profile, ensuring that the "humanized" history is a perfect stylistic match for your actual development pattern.

## Using the Baseline

Once a baseline is generated, apply it when dripping commits:

```bash
papertowel wring drip --profile <your-baseline-name>
```

To inspect the stored baseline without re-running the analysis:

```bash
papertowel learn show .
```

By learning from your own history, `papertowel` moves from "simulating a human" to "simulating *you*."
