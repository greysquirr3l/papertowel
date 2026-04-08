# Lexical Analysis

The Lexical detector is the frontline of `papertowel`. It targets the specific vocabulary that LLMs are statistically predisposed to use.

## The Slop Vocabulary

LLMs are trained on vast amounts of documentation and web content, which often contains a specific "corporate-technical" dialect. When this dialect appears in source code or internal comments, it's a strong signal of AI involvement.

### High-Signal Keywords
We maintain a list of keywords that are frequently flagged. When these appear in clusters, the "AI score" for a file increases significantly.

| Word | Why it's flagged |
| :--- | :--- |
| **Robust** | Rarely used by humans to describe their own code unless they're selling it. |
| **Comprehensive** | A classic LLM adjective for summaries or utility functions. |
| **Leverage** | The quintessential "corporate-speak" replacement for "use." |
| **Utilize** | Almost always an unnecessary replacement for "use." |
| **Seamless** | More common in marketing than in actual implementation notes. |

### Common Phrases
Phrases are even stronger indicators than single words.
- *"It's worth noting that..."*
- *"In order to achieve X, we can..."*
- *"This ensures that the system remains..."*

## How Transformation Works

When the Scrubber is in `scrub` mode, it doesn't just delete these words. It attempts to replace them with more "human" alternatives or rephrase the sentence entirely to break the predictable pattern.

For example:
- **AI**: `// This function provides a robust way to utilize the cache.`
- **Humanized**: `// This handles caching.`

By breaking the rhythmic, overly-formal patterns of the LLM, the code becomes indistinguishable from a human's shorthand.
