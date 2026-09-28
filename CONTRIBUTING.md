# Contributing to Proteus

This guide covers how code comments, documentation and commit messages are written in this
repository.

## Writing comments and docs

### Who you're writing for

- **Doc comments** (`///`, `//!`, TSDoc) are for someone using the API who has never seen this
  codebase.
- **Plain comments** (`//`) are for the next person changing the code.

### Doc comments

1. **The first line is one sentence saying what the item does.** It is shown on its own in
   lists and search results. Functions start with a verb ("Hides…", "Returns…"); types start
   with a noun phrase.
2. **Add more only when a caller needs it:** what the item guarantees, its defaults, and the
   edge cases someone would otherwise get wrong.
3. **Use the standard sections.** `# Errors` on every function that returns `Result`,
   `# Panics` on anything that can panic, and `# Examples` on the main entry points (crate
   roots and the key types). Examples must compile; they run as doctests.
4. **Link the items you mention**, as in ``[`Handle::set_visible`]``. A rename then breaks the
   docs build instead of leaving the text quietly wrong.
5. **Say why only when the code can't show it:** a non-obvious constraint, a surprising
   default, or a trade-off the caller must know about. One or two sentences.
6. **Keep it short.** A doc that needs more than about ten lines probably belongs in a guide in
   `docs/`.
7. **Write plain, natural sentences**, the way you would explain the item to a colleague. Use
   the API's own terms, and don't introduce things it doesn't have: the pointer is "pressed",
   not "a button went down".

### Plain comments

- Explain **why**, never what. Don't narrate code that already reads clearly.
- Good uses: ordering constraints ("must run after the visibility pass because…"),
  invariants, and workarounds for a specific external bug, with a link to it.
- A `TODO` says what is missing and links a GitHub issue.

### Tests

- Use `//` comments, never `///`.
- The test name says what behaviour is locked in. A comment above it says why that matters
  when it isn't obvious, such as the bug the test guards against.
- If a test pins a deliberate quirk, such as a one-tick delay, say so, or someone will "fix"
  it.

### Never in a comment

- Milestone numbers, audit IDs, step numbers or dates.
- References to external documents. They go stale, and the comment becomes inaccurate with
  them.
- History: "was X", "used to", "previously", "no longer", "originally". Git keeps history.
- Deliberation: "we decided", "per this pass's design", "matches X's existing behaviour", or
  options that were considered and rejected.

A placeholder may say plainly what it is reserved for, once: "Not read yet; reserved for
keyboard navigation."

`scripts/check-comments.sh` enforces the first two rules and the terminology below; review
catches the rest.

### TypeScript

- Every export gets TSDoc. Use `@param` when a parameter's name doesn't make its meaning
  obvious, and `@returns` / `@throws` where they apply. `mount`, `ProteusApp`, `component`
  and `signal` each carry an `@example`.
- Write for someone who only knows TypeScript. Don't send them to Rust items ("see
  `proteus_ui::SplitStrategy`"); explain the behaviour where they are reading.

### Terminology

| Use | Meaning | Instead of |
|---|---|---|
| **component** | A UI element created with `component()`. For the bevy_ecs meaning, say **ECS component**. | using both meanings unmarked |
| **transition** | The animated change from one geometry to another | "morph" |
| **1→1, 1→N (split), N→1 (merge)** | The three shapes a transition can take | "topology" in user-facing docs |
| **declared geometry** | The geometry a component returns to when idle, as set by `ComponentSpec::new` or `Handle::set_declared_geometry` | "rest state", "rest geometry", "declared state" |
| **tick** | One run of `Proteus::tick`, the update step | "frame" when a tick is meant |
| **frame** | One rendered frame. Each frame runs one tick. | |
| **host** | The crate that owns the window or canvas, the GPU surface, the loop and input: `proteus-host-winit`, `proteus-host-web` | "shell" |
| **shell** | Only the two thin entry crates, `proteus-shell-native` and `proteus-shell-web` | |
| **app** | Anything that implements `App` | |
| **bake** | Render into an atlas texture. Always say what: bake text, bake an image, or bake a component (flatten it and its children into one texture, permanently). | a bare "bake" |
| **virtual** | A temporary component a split or merge creates, and removes when it completes | |
| **all the other**, **any other** | Everything except the item just mentioned | "every other", which can also mean alternate items |

## Commit messages

Commits follow [Conventional Commits](https://www.conventionalcommits.org). The changelog is
generated from them.

```
type(scope): subject

Body: what changed and why, wrapped at 72 columns.
```

- The subject is imperative, lowercase after the colon, has no trailing period, and is at most
  72 characters.
- **Types:** `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`. A
  breaking change adds `!` after the type and a `BREAKING CHANGE:` footer.
- **Scopes:** the crate name without `proteus-` (`sdk`, `sdk-web`, `runtime`, `ui`, `render`,
  `gpu`, `host-winit`, `host-web`, `demo`, `shell`), or `examples`. For `docs:` commits, the
  document (`readme`, `planning`, `roadmap`, `guide`, `contributing`).
