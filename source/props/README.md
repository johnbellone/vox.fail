# Prop sources

The sources of the accepted props in `data/props/`. No `.vox` files yet: the seed props are prop recipes (the style of the `vox-art` recipe golden tests), and `source/vox/props/` does not exist.

- `palette.json`: the materials the recipes use (a `vox_art::Palette`).
- `<name>.json`: one prop, `{ "def": {...}, "recipe": {...} }`. `def` holds what a recipe cannot say: `id`, `name`, `tags`, `pivot`, `place`, `collide`, `palette_version`. Scale, size and named parts come from the recipe. `stool.json` differs from the golden test's stool in one number: its seat is radius 4, so the legs touch it (the prop checks refuse a floating leg).

Rebuild `data/props` from the repository root (it refuses a prop that fails the checks and writes nothing then):

```sh
cargo run -p vox-props -- --source source/props --out data/props
```

`cargo test -p vox-props` fails if `data/props` is out of step with these sources.
