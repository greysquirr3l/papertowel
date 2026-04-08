# Introduction

Welcome to **papertowel**, the specialized toolkit for cleaning up the "slop" of AI-generated code.

## The Problem: The AI Fingerprint

In many software communities, the origin of code has become a proxy for its quality. Despite the fact that LLMs are increasingly integrated into the development workflow, there is a lingering stigma—and even a forensic obsession—with identifying AI-generated code.

The "AI fingerprint" isn't usually found in the logic, but in the style:
- **Slop Vocabulary**: A predictable set of adjectives and verbs ("robust," "comprehensive," "leverage," "streamlined").
- **Over-Documentation**: Comments that describe *what* the code is doing (which is obvious) rather than *why* it's doing it.
- **Structural Uniformity**: A level of organizational perfection and boilerplate consistency that rarely occurs in human-written code.
- **The "Perfect" History**: A git history where every commit is a perfectly scoped unit of work, devoid of the entropy, distractions, and "fix the fix" cycles that characterize real human development.

## Our Philosophy

**papertowel** exists to decouple code quality from code provenance.

We believe that if the code is correct, tested, and solves the problem, its origin should be incidental. However, we recognize that the "purity police" exist. Instead of fighting a philosophical battle, **papertowel** provides the technical means to sidestep the critique.

Our goal is to make AI-assisted code stylistically indistinguishable from human-written code. We don't just change words; we humanize the entire lifecycle of the code, from the source files to the commit history.

## How it Works

The project is split into two primary subsystems:

### 1. The Scrubber
The Scrubber is a static analysis engine. It scans your source code for stylistic fingerprints and transforms them. It's designed to be pluggable, allowing for new detectors to be added as the "slop vocabulary" evolves.

### 2. The Wringer
The Wringer is a git history humanizer. Rather than rewriting history (which leaves forensic traces in the git object database), it uses git worktrees to "drip-feed" commits from a private development branch to a public branch. This process is driven by **Persona Profiles**, which simulate human behavior—including working hours, productivity peaks, and the natural chaos of human commit messages.

---

*Built with the assistance of machines. Obviously.*
