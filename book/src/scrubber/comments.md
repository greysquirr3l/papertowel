# Comment Thinning

One of the most obvious tells of AI-generated code is the presence of comments that describe the *what* instead of the *why*.

## The "Obvious" Comment

Humans generally write comments for two reasons:

1. To explain a non-obvious decision.
2. To warn future maintainers about a "gotcha."

AI, however, often generates comments as a way to demonstrate that it understands the task. This results in comments like:

```rust
fn add(a: i32, b: i32) -> i32 {
    // This function adds two integers together and returns the result
    a + b
}
```

## Scoring Redundancy

The `comments` detector analyzes the relationship between the comment text and the code it precedes. If the comment is a near-perfect natural language translation of the code's logic, it is flagged as "High Severity Slop."

## The Thinning Process

When you run `papertowel scrub`, the comment detector performs "thinning":

1. **Deletion**: Truly redundant comments (like the `add` example above) are removed entirely.
2. **Simplification**: Overly formal descriptions are shortened into human-like shorthand.
3. **Preservation**: Comments that contain high-entropy information (like a link to a bug report or a complex mathematical explanation) are preserved.

The result is a codebase that looks like it was written by someone who knows the language well enough that they don't need to explain every line.
