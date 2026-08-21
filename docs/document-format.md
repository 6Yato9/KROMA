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

## Geometry is not an effect

Crop, straighten, flip and quarter-turn live in `Document::geometry`, outside
the stack, and run *before* row zero. The reason is structural: every row in
the stack takes an image and returns an image of the same size. Cropping
changes the size, so a crop row would force every later row — and the stage
cache, and the export path — to cope with the frame changing shape underneath
it.

Putting it first also settles a question that otherwise has no good answer. A
vignette darkens the corners of the photograph the user is making, not the
corners of the sensor. Because the crop happens first, "the frame" already
means the cropped frame everywhere downstream, and no effect had to learn
about cropping at all. `effects_see_the_cropped_frame` is the guard.

**The model is Lightroom's.** The whole image is rotated about its centre into
*straightened space*, and the crop is an axis-aligned rectangle in there. That
is what makes the overlay tractable: while the crop tool is open the viewer
shows straightened space directly, so the rectangle stays axis-aligned on
screen at any angle.

**It costs no extra pass.** The crop, the angle, the turns, the flips and the
preview's own zoom and pan are all affine, so they compose into a single map
that the colour transform was already reading through. The source is sampled
exactly once however many of them are set. A pass per operation would resample
the picture two or three times over and soften it a little each time for
nothing. `a_document_with_no_crop_does_not_resample` is what keeps the identity
case honest — half a texel of drift would blur every photograph that ever
passed through the program and look like nothing at all until someone compared
at 400%.

**`Document::resize` is separate and applies last.** The crop decides what is
in the picture; the resize decides how many pixels it is delivered in. Running
it after the stack means grain and sharpening were rendered at full resolution
and are scaled down along with the picture, which is the only order that looks
like the preview did. It never enlarges: a request for 2048 on the long edge
from a 1600px file gives back the file, not a soft version of it.

## One edit per photograph, one photograph in memory

A set is a list of paths, a few kilobytes of edit each, and a 128-pixel
thumbnail. Only the photograph being worked on is decoded: a 24-megapixel
frame is 96 MB of RGBA, so a folder of two hundred would be twenty gigabytes,
and making a set navigable *without* holding it is the entire reason a
filmstrip exists.

What is parked per photograph is the whole `History`, not just the
`Document`. Clicking the wrong thumbnail and clicking back must not cost an
undo stack — losing an hour of work that way is not a tolerable thing for an
editor to do. `History` deliberately does not implement `Clone` for the same
reason: an undo stack with two owners is a bug waiting to be written, so
switching moves it out wholesale instead.

**A pasted grade is the stack only.** A crop belongs to the frame it was drawn
on, and pasting a 16:9 crop onto a portrait shot is almost never what anyone
meant. The grade travels between photographs; the framing does not.

**Nothing in the program deletes a file.** Taking a photograph out of the set
removes it from a list, and a batch export always writes into a folder the
user chose — never beside the original, which would quietly turn the next
run's input into its own output.

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
