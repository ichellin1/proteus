# Browse

A TV-style browse screen, used with the arrow keys, Tab and Enter as well as the pointer. A bar
of genres, Drama, Action and Comedy, sits above two rows of tiles, Films and Series, which run
off the right edge and show the chosen genre's titles.

Enter on a tile, or a click, merges every tile into the title's page (N→1, with `mergeFrom`),
with a description, Play and Close. Play turns the page into a full-screen video through a
transition channel (1→1), as the genre bar fades out. Escape, Backspace or Delete comes back
from the video to the page, and again, or Close, splits the page back into the tiles (1→N,
with `splitTo`). Choosing another genre slides both rows back to their first tile. Holding
Enter on a tile for half a second makes it a favourite.

It shows each of the keyboard and focus features once:

- **Focus groups:** the genre bar wraps Left and Right round, and focus entering it lands on
  the chosen genre, its default focus. Each row keeps Left and Right, lets Up and Down leave,
  and focus entering it from the bar or the other row lands on the nearest tile. The page keeps
  the arrows, and wraps Tab round.
- **Scrolling:** a tile off screen can take focus, and the row slides to show it.
- **One highlight at a time:** the focused style, with a glow and a border, shows while the keys
  are in use, and the hover style while the pointer is. The first key after using the pointer
  only shows where focus is.
- **Focus set from code:** the app clears focus as each change of screen starts, and gives it to
  Play, or back to the tile the page opened from, as it ends.
- **Disabled components:** the genre buttons are disabled while the page shows, so neither
  focus nor the pointer reaches them.
- **Input during a transition:** the genre buttons change color as a transition, and still take
  focus and clicks while it runs.
- **A tap or a hold:** a tile opens on a tap of Enter, and becomes a favourite on a hold.
- **Back:** the app decides what it means. Here, it steps back from the video to the page, and
  from the page to the tiles. Escape and Backspace are bound to it already; the app binds
  Delete too, adding to the key map.
- **The input mode:** the hint at the bottom names the keys or the pointer, whichever is in use.

The genre bar's fade is eased by the app, frame by frame, since opacity isn't part of a
transition yet. A title too wide for its tile is cut short with "…", keeping 10 px from each
edge: the app measures the baked text and cuts it until it fits, since text is a single line.

On the web, the canvas takes the keyboard when the page opens, and Tab wraps round, as in a
full-screen app, with `autofocus` and `keepTab`. Without `keepTab`, Tab would leave the canvas
past the last component or before the first, as it leaves the end of a page. The canvas is
1100 by 720, the desktop window's size, and centered in the browser window, so the rows start
and scroll the same way in both.

Techniques worth copying are described at the top of each version's source:
[`rust/src/lib.rs`](./rust/src/lib.rs) and [`typescript/src/main.ts`](./typescript/src/main.ts).

## Run it

You need the tools in [What you need](../README.md#what-you-need).

### Rust

In a desktop window, from the repository's root:

```bash
cargo run -p browse
```

In a browser:

```bash
make example-browse-web
```

Then open <http://localhost:8080/examples/browse/rust/>. Without make, build it for the web,
then serve the repository from its root:

```bash
wasm-pack build examples/browse/rust --target web --release
python3 -m http.server 8080
```

### TypeScript

From the repository's root:

```bash
make example-browse-ts
```

Then open the address Vite prints, usually <http://localhost:5173>.

Without make, build the TypeScript SDK, then start the example:

```bash
make build-sdk-web
cd examples/browse/typescript
npm install
npm run dev
```
