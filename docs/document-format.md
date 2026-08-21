# The document format

Decision record. `schema_version` is `1`.

## The stack is the document

There is no other representation of an edit, and nothing is ever destructive to
the source image. Open a file six months later, change grain from 20 to 5, and
nothing has degraded, because nothing was ever baked.

## Rules

**JSON, not binary.** Diffable and readable, which is worth days of debugging.
The file is kilobytes regardless of image size, so there is nothing to win by
making it opaque.

**`schema_version` from the first commit.** `Document::from_json` refuses a
document from a newer schema rather than misreading it, and routes older ones
through `migrate()`. That function is empty at v1 by definition — it exists so
the *call site* is already wired up. Retrofitting migration into a loader after
files exist is where compatibility bugs come from.

**Unknown fields are preserved.** A file touched by a newer build and saved by
an older one keeps everything it arrived with, via `#[serde(flatten)]` into
`Document::unknown`. Guarded by
`unknown_top_level_fields_survive_a_round_trip`.

**Parameters are dynamically typed.** A `BTreeMap<String, ParamValue>`, not a
struct per effect, so a document containing an effect this build does not know
about still round-trips. `BTreeMap` specifically, not `HashMap`: the golden
tests diff serialised documents, and hash ordering would make every save
produce different bytes for identical content.

**Effects are referenced by string key, not by enum.** Same reason. Keys are
stable forever; renaming one is a migration.

**Missing or wrong-typed parameters fall back to defaults** rather than
erroring. A partially-written or hand-edited document should degrade to
something sensible, not fail to open mid-render.

## Every row carries its own opacity, blend and key

The single most important shape decision in the format:

```json
{
  "id": 2,
  "effect": "halation",
  "params": { "strength": { "t": "float", "v": 0.4 } },
  "enabled": true,
  "opacity": 0.4,
  "blend": "screen",
  "key": { "kind": "generated", "source": "sky", "params": {} }
}
```

This mirrors a Resolve node's anatomy — RGB in, key in, opacity, composite
mode. Because those four fields live on the *row* rather than inside particular
effects, "grain at 40% in Screen mode, sky only" needs no special code
anywhere. Put them inside individual effects instead and each becomes a
separate feature request against every effect in the registry, forever.

The `key` variants are declared but unimplemented until M3. Declaring them now
means the format does not need a version bump when masks land, and the
renderer's row loop handles an optional key from the very first version.

## Open question: embedded or referenced source

`Source` supports both. Referenced is smaller; embedded survives the user
reorganising their photo library. **Not yet decided**, and it should be settled
before anything is saved that matters — changing the default afterwards is a
migration rather than a preference.

## Colour management is stored by name

```json
"color": { "input": "sRGB", "output": "Display P3" }
```

Names, not matrices. A document must not bake in a numeric matrix that a later,
more accurate derivation would contradict — see the D65 precision note in
[color-pipeline.md](color-pipeline.md). Unknown names fall back to sRGB so a
document referencing a space this build has not learned about still opens.
