# Proteus — Planning

The decisions and definitions of done for the work underway. [ROADMAP.md](./ROADMAP.md) gives
the order of the releases and the candidates for what comes next.

## How this document works

- **It records decisions and definitions of done.** A decision says what was decided and when,
  in a paragraph at most. A definition of done is a checklist, ticked as the work lands.
- **Reasoning longer than a paragraph goes in a design note,** in
  [docs/design/](./docs/design/README.md), linked from here: what was found or measured, the
  approaches weighed, and how to tell whether a change worked.
- **The design v0.1.0 settled is described in [ARCHITECTURE.md](./ARCHITECTURE.md),** with its
  key decisions. Changing one of them is a decision, recorded here.
- **The planning for past releases is archived** in [docs/archive/](./docs/archive/), as
  history: v0.1.0's milestones, M0 to M14, and each release's section from here once it ships.
  Nothing here depends on it.

## Release planning

Decides the releases: which of ROADMAP's candidates each takes, and in what order. Until it's
done, the candidates are unordered.

- **Decided (2026-10-15): releases, not milestones.** Now that Proteus is released, work is
  planned by version, each release with a theme and features of its own. Milestone numbers end
  at M14. "V2" in older documents means everything after v0.1.0, not a release.
- **Decided (2026-10-15): a release's scope is fixed.** It ships when its features are done,
  not on a date. Small fixes ride along in the next feature release, or, if found after a
  release ships, go out in a patch release.
- **Decided (2026-10-15): candidates are weighed in this order:** fixing known limits and known
  issues; what outside developers need to build real apps; effects, such as blur; then other
  platforms.
- **Decided (2026-10-15): the origin stays at the viewport's center, with y up.** Instead of
  moving it, Rust gets equivalents of TypeScript's `topLeftToWorld` and `worldToTopLeft`, and
  the guides make the origin and the conversion clear in both languages. Nothing breaks.
- **Decided (2026-10-15): dogfooding is off the roadmap.** A personal website built with Proteus
  happens outside it, and what it finds comes back to ROADMAP as candidates.
- **Decided (2026-10-19): work independent of a release is tracked in ROADMAP too,** in a
  section of its own. Rewriting the history to shrink the repository is the first such task,
  done early.
- **Decided (2026-10-19): a draft order of releases,** v0.2.0 to v0.8.0, is in ROADMAP. Each
  release is planned in detail when it comes up, and its scope can change then. Only the next
  release has a section here.
- **Decided (2026-10-20): v0.2.0 skips drawing frames in which nothing changed.** Letting the
  frame loop sleep as well is risky for little gain, so it stays a candidate, to try as a proof
  of concept after the planned releases.
- [x] ROADMAP and PLANNING plan by release rather than milestone.
- [x] Every candidate in ROADMAP is placed in a release, kept as a candidate for later, or
  dropped with a reason.
- [x] The releases are in ROADMAP in order.
- [x] The two high-priority candidates, the shader fast path and rounded corners on baked
  slices, are decided first.

## v0.3.0 — Keyboard and focus

Keyboard input and focus navigation, on both hosts, and a reference demo reorganized around
them. The scope is from ROADMAP's draft order, with a focus example and the demo's new home page
added. The steps are in the order they're built; each step's definition of done is written when
it's planned.

- **Decided (2026-10-29): an app chooses which input devices it accepts.** It's a set of
  devices: the pointer and the keyboard in v0.3.0, then touch, TV remotes and game controllers
  as they're added. Pointer only, keyboard only and both are settings of the set rather than
  modes, so a new device adds a member, not new combinations. The default is every device the
  host supports, so existing apps are unchanged, and the app can change it while it runs. Input
  from a device the app doesn't accept is dropped before hit testing, navigation or the app's
  callbacks see it, key callbacks included.
- **Decided (2026-10-29): when an app accepts both styles of input, the last input wins.**
  Pointing devices (the mouse and pen, later touch) hover and click; navigating devices (the
  keyboard, later remotes and controllers) move focus. A key press shows the focused style, and
  the first arrow key starts from the hovered component, if there is one; moving the pointer
  hides the focused style again. A click still gives focus, but its focused style no longer
  shows, as with CSS's `:focus-visible`. Going from one navigating device to another, such as
  the keyboard then a controller, carries on from the focused component.
- **Decided (2026-10-29): a component can narrow the devices it accepts,** with the same set.
  It accepts what both it and the app accept. It's set when the component is created, and can
  change as the component's state changes. A component that doesn't accept the pointer lets it
  through to whatever is behind, as a non-interactive one does. The guides advise choosing up
  front which devices an app and each of its components accept.
- **Decided (2026-10-29): touch stays part of the pointer until it's built.** Browsers report
  it through pointer events, as they do the mouse. When touch is added, the web host tells them
  apart by the event's `pointerType`.
- **Decided (2026-10-29): the demo's home page becomes one grid of features,** headed "See
  what Proteus can do", with a one-line tagline saying what Proteus is. The Examples screen's
  categories join the home page's buttons in the grid, which gains a tile as each release ships
  a feature: Keyboard & Focus in this one. The stress tests are removed, since the benchmarks
  measure more, and so is the 3D placeholder, which is far off. An About tile, saying what
  Proteus is for a visitor, takes its place. The demo gets a setting for which devices it
  accepts, and Escape and Backspace go back a screen.
- **Decided (2026-10-29): hosts turn every device's keys into one set of keys.** winit's key
  events, the browser's `KeyboardEvent`, and later a TV remote's own key codes all arrive as the
  same values, named as the web names them, which winit mirrors. A key can be named by what it
  means, such as `Enter` or `z`, or by where it is, such as `KeyW`, so a binding by position
  works on any layout.
- **Decided (2026-10-29): a key map binds keys to actions.** The built-in actions are Up, Down,
  Left and Right, bound to the arrow keys; Next and Previous, to Tab and Shift+Tab; Activate, to
  Enter and Space; and Back, to Escape and Backspace. Remotes and controllers add bindings to
  the same actions when they're built. Apps can define actions of their own, and rebind or add
  to the map while they run. A binding's modifiers must match exactly. The navigation stage
  reads actions, never keys.
- **Decided (2026-10-29): each event carries its key, and its action if the key is bound.** It
  also says whether the key was pressed, released or repeated, the modifiers, and the text the
  key types, if any. Most handlers match on the action; the key is there for keys the map
  doesn't cover, such as a remote's colored buttons, and for typing, where a text field must
  take Backspace and Space before they act as Back and Activate.
- **Decided (2026-10-29): events go up from the focused component.** An event goes to the
  focused component, then its ancestors from the nearest, then an app-wide handler, which gets
  it even when nothing has focus, and last to the engine's default for its action: Up, Down,
  Left, Right, Next and Previous move focus, and Back has none. Any handler can claim the event,
  which stops it there, as `preventDefault` does on the web. Components that are disabled,
  non-interactive or don't accept the device are skipped, and the event carries on to their
  ancestors. Events are handled at the start of the next tick, in the order they arrived, before
  the update, so navigation sees only those no handler claimed.
- **Decided (2026-10-29): typing text with an input method is left for later.** Dead keys,
  input methods for Chinese, Japanese and Korean, phone keyboards and pasting produce text that
  doesn't match one key press, and need the text input events browsers and winit send apart from
  keys. Each key's own text is enough for simple typing in this release; full text input is a
  candidate in ROADMAP.

### Step 1 — Keys reach the engine

Both hosts read key events and report them to the engine as the common set of keys, with the
modifiers, whether the key repeats and the text it types. Keys still held are released when the
window or canvas loses focus.

### Step 2 — Input devices and focus

The set of devices an app and each component accept, and an API to give a component focus,
clear it and ask which component has it. To decide: what happens to focus when the focused
component is destroyed, hidden, disabled or made non-interactive.

### Step 3 — The key map and actions

The key map, its built-in actions and the app's own, and callbacks for key events on components
and on the app, in Rust and TypeScript, going up from the focused component. To decide: whether
Activate's default is to fire the focused component's click.

### Step 4 — Navigation

The schedule's navigation stage moves focus with the Up, Down, Left, Right, Next and Previous
actions, follows the last input wins, and reads `TransitionInteractionConfig::allow_navigation`. To decide: the Tab
order, how the arrow keys choose the next component, and whether focus can be kept inside a
component, such as a dialog, in this release.

### Step 5 — Interaction styles for glow and border

Interaction styles can change a component's glow and border, which the demo's hover needs.

### Step 6 — A focus example

A new example, in Rust and TypeScript, that's used with the keyboard.

### Step 7 — The demo's new home page

The home page's grid, header, tagline and About tile, with the stress tests, the 3D placeholder
and the Examples screen removed. To decide: whether a Layout tile shows before v0.6.0.

### Step 8 — The demo on the keyboard

The demo uses interaction styles instead of its own hover code, works with the keyboard on
every screen, and has its setting for which devices it accepts.

### Step 9 — Docs

The interaction and hosts guides, the notes on non-interactive and disabled components, and
ARCHITECTURE's known limits. The guides say to handle actions, and to use keys only for typing
and for keys the map doesn't cover.

## Standing decisions

- **Non-interactive and disabled components ignore all input** (2026-09-28), not just the
  pointer: mouse, touch, pen, keyboard, gamepad and TV remote. The Rust and TypeScript docs for
  making a component non-interactive or disabled state this, noting that only pointer input
  exists today. When keyboard navigation or any other input is built, focus and every other input
  path must skip non-interactive and disabled components, and those notes must be updated to
  match.
