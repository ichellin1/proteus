# The demo's fonts

The Text panel's fonts, besides the built-in Inter, cut down to the characters the panel shows
in them, so the web demo downloads about 30 KB rather than 1.2 MB before its first frame:

| File | Font | Kept |
|---|---|---|
| `Lora-Variable.ttf` | Lora, variable in weight | "Lora", 3.5 KB |
| `Lora-Italic-Variable.ttf` | Lora Italic, variable in weight | "Aa", 2.7 KB |
| `NotoSerifDevanagari-Variable.ttf` | Noto Serif Devanagari, variable in weight | "नमस्ते" and "आगे पढ़ें", 23 KB |

Each keeps its weight axis and all its layout rules, so Devanagari's letters still join and
take their vowel marks. Each draws its text exactly as the full font does.

Lora and Noto Serif Devanagari are from Google Fonts, under the SIL Open Font License 1.1, which
allows cutting a font down. The license texts are [`OFL-Lora.txt`](./OFL-Lora.txt) and
[`OFL-NotoSerifDevanagari.txt`](./OFL-NotoSerifDevanagari.txt).

## Cutting them again

When the panel's text in these fonts changes, cut them again from the full ones in
`crates/proteus-test-fonts/fonts`, with fontTools (`pip install fonttools`), from the
repository's root, with the new text in `--text`. The text is in `screens/example_detail.rs`:
`IN_LORA`, `IN_LORA_ITALIC`, `HINDI_GREETING` and `HINDI_BUTTON`. A letter a cut font lacks
isn't drawn as a box but in Inter, so it's easy to miss: the demo's tests check each font has
every letter of that text.

```bash
python3 -m fontTools.subset crates/proteus-test-fonts/fonts/Lora-Variable.ttf --text="Lora" --layout-features='*' --output-file=crates/proteus-demo/assets/images/fonts/Lora-Variable.ttf
```

```bash
python3 -m fontTools.subset crates/proteus-test-fonts/fonts/Lora-Italic-Variable.ttf --text="Aa" --layout-features='*' --output-file=crates/proteus-demo/assets/images/fonts/Lora-Italic-Variable.ttf
```

```bash
python3 -m fontTools.subset crates/proteus-test-fonts/fonts/NotoSerifDevanagari-Variable.ttf --text="नमस्ते आगे पढ़ें" --layout-features='*' --output-file=crates/proteus-demo/assets/images/fonts/NotoSerifDevanagari-Variable.ttf
```
