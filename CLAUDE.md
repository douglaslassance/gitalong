# Gitalong

## Conventions

### Working with the repository

- Make changes in the work tree and stop there. Do not `git add`, `git commit`, `git push`, or open a pull request unless asked to.
- When asked to commit, split the work tree into logical commits rather than one lump. Each commit should stand on its own.
- Work on the current branch (usually `main`). Do not create branches or open pull requests unless asked to.
- Never rewrite history (rebase, amend, squash) or force push a branch that has already been pushed without asking first.

### Commit messages

- A single line. No body, no bullet points, no trailers.
- Sentence case, imperative mood, no trailing period: `Add LoRA support`, not `Added LoRA support`, `add lora support`, or `Add LoRA support.`
- 72 characters maximum. Drop detail rather than go over.
- No `Co-Authored-By`, no "Generated with" footer, no emoji, no AI attribution of any kind.
- If a change seems to need a body, split it into several focused commits instead.

### Versioning

- Git tags and version strings are bare, no `v` prefix. `1.2.3`, not `v1.2.3`.

### Pull requests

- Keep the description short and objective. State what the change does, not the story of how it got there, unless a reviewer genuinely needs it.
- No narration of rejected approaches, no open questions, no pre-emptive self review. If a decision needs input, ask it as one plain line.
- No wall of generated text, no AI attribution.

### Writing

- No em dashes (—), en dashes (–), or any other non-hyphen dash character anywhere: code, comments, UI copy, commit messages, pull request descriptions, chat. Use a comma, a colon, parentheses, or a second sentence. A spaced hyphen (" - ") standing in for a dash counts as a dash. Plain hyphens are fine in compound words and as the ASCII minus in code.
- Sentence case for user facing strings: labels, section headers, notices, command names, settings. Acronyms and proper nouns keep their capitalization.
- Keep prose short and human. No generated wall of text, no filler, no comments restating the obvious.

### Comments and docstrings

One line. If it needs a second, it does not need a comment: it needs a better
name, a type, encapsulation, or a test. Wanting to write a paragraph is the
signal that two fields are encoding one decision, that a magic number wants an
enum, or that an invariant wants a test which fails loudly when someone breaks
it, where prose saying the same thing quietly becomes a lie.

A comment may state a fact the signature cannot carry: units, an origin, a
sentinel, a licence, a quirk in a format or protocol, why this shape rather than
the obvious one. It may also label a block, though a block that needs a label
usually wants to be a function, so write the function where you can.

TODO, FIXME and HACK are exempt. They are not explaining the code, they are
admitting something about it, and that is worth keeping however it reads.

No comment that restates the signature. No banner blocks, no divider lines, no
commented out code.

The exception is the public API of the `gitalong` library crate. Those `///`
comments compile to docs.rs and are read by people who never open the source,
so there they are the product rather than a note about it.
