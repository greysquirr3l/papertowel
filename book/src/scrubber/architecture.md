# Architecture Detection

AI-generated code often lacks coherent architectural patterns. While a human developer would organize code into layers, define abstractions at boundaries, and split responsibilities across modules, AI tends to dump everything into flat files with minimal structure.

The architecture detector analyzes your codebase for these organizational anti-patterns.

## What It Detects

### ARCH001: Flat Module Structure

Projects with all source files at the same directory level, lacking meaningful subdirectories like `domain/`, `infrastructure/`, or `services/`.

**Why it matters:** Human developers naturally organize code as it grows. A project with 20+ files all in `src/` suggests generation rather than evolution.

### ARCH002: Missing Architectural Layers

No recognizable layer directories found in larger projects. The detector looks for patterns from:

- **DDD/Clean Architecture**: `domain/`, `application/`, `infrastructure/`, `presentation/`
- **Hexagonal**: `ports/`, `adapters/`, `core/`
- **CQRS**: `commands/`, `queries/`, `handlers/`
- **Common conventions**: `services/`, `repositories/`, `models/`, `cli/`, `api/`

**Threshold:** Triggered when a project has 16+ source files with no layer directories.

### ARCH003: God Files

Files exceeding 800 lines that likely mix multiple responsibilities. AI tends to generate long, monolithic files rather than splitting concerns.

**Why 800 lines?** Rust files with inline tests commonly reach 400-600 lines legitimately. 800 lines is a reasonable threshold for "this probably does too much."

### ARCH004: Low Trait Ratio

Projects where less than 2% of types are traits. AI-generated code often skips defining abstractions at boundaries, leading to concrete types everywhere with no interfaces.

**Why it matters:** Traits enable dependency inversion, testability, and clear module boundaries. Their absence suggests "just make it work" generation rather than thoughtful design.

**Caveat:** CLI tools and scripts may legitimately have few traits. The detector requires at least 5 structs before flagging.

### ARCH005: Anemic Domain Models

High ratio of structs with no associated `impl` blocks. AI tends to generate data-only structs without domain behavior.

**Threshold:** Flagged when >80% of structs have no methods.

**Why it matters:** Rich domain models encapsulate both data and behavior. Anemic models push logic into external functions, often a sign of procedural thinking.

## Configuration

The architecture detector uses these defaults:

| Setting | Default | Description |
|---------|---------|-------------|
| `min_source_files` | 8 | Skip analysis for smaller projects |
| `god_file_lines` | 800 | Lines threshold for god file detection |
| `min_trait_ratio` | 0.02 | Minimum trait/(trait+struct) ratio |
| `max_anemic_ratio` | 0.80 | Maximum fraction of structs without methods |
| `min_directory_depth` | 2 | Minimum nesting depth for non-flat structure |

## Exclusions

The architecture detector automatically skips:

- `/target/` (build artifacts)
- `/tests/` and `*_test.rs` (test files)
- `/book/` and `/docs/` (documentation)
- `/vendor/` (vendored dependencies)
- `/.git/` and `/.coraline/` (tool data)

Use `.papertowelignore` to exclude additional paths.

## Grade Impact

Architecture findings are weighted at **20%** of the overall grade, equal to lexical findings. This reflects that code organization is a strong signal of generation vs. authorship.

| Finding | Severity | Confidence |
|---------|----------|------------|
| ARCH001 (flat structure) | Medium | 0.75 |
| ARCH002 (missing layers) | Medium | 0.70 |
| ARCH003 (god file) | High | 0.85 |
| ARCH004 (low traits) | Medium | 0.65 |
| ARCH005 (anemic models) | Low | 0.60 |
