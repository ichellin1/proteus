# Stepper

An onboarding flow of four steps, each a card with its own shape and color: a round welcome,
a wide card, a tall card and a large one. Next and Back turn the current card into the next or
previous one with a transition channel (1→1): nothing is swapped, the card becomes the next
step. Dots below show the progress, Back is disabled on the first step, and the last step starts
over.

Techniques worth copying are described at the top of each version's source:
[`rust/src/lib.rs`](./rust/src/lib.rs) and [`typescript/src/main.ts`](./typescript/src/main.ts).
They include driving a whole flow with one channel, and clicking while a card is still moving.
The Rust version's callbacks do all the work themselves, sharing the handles and the current
step in an `Rc`; compare the [gallery](../gallery), whose callbacks hand events to `update`.

## Run it

You need the tools in [What you need](../README.md#what-you-need).

### Rust

In a desktop window, from the repository's root:

```bash
cargo run -p stepper
```

In a browser:

```bash
make example-stepper-web
```

Then open <http://localhost:8080/examples/stepper/rust/>. Without make, build it for the web,
then serve the repository from its root:

```bash
wasm-pack build examples/stepper/rust --target web --release
python3 -m http.server 8080
```

### TypeScript

From the repository's root:

```bash
make example-stepper-ts
```

Then open the address Vite prints, usually <http://localhost:5173>.

Without make, build the TypeScript SDK, then start the example:

```bash
make build-sdk-web
cd examples/stepper/typescript
npm install
npm run dev
```
