# The colour pipeline

Decision record. Settled at M0; changing any of it later is a migration, not a
refactor.

## The two-space rule

Effects are not interchangeable operations on pixels. Each one either simulates
**light** or manipulates **perception**, and it must run in the matching space.

```
 input space ──▶ ACEScg (linear) ──▶ ACEScct (log) ──▶ ACEScg ──▶ output space
                       │                   │              │
                  exposure, WB        wheels, curves   vignette
                  bloom, halation     HSL, contrast    sharpen
                  blur, CA            grain
```

Two failure modes this prevents, both of which are visible and neither of which
users can name:

- **Blur in a gamma-encoded space** turns highlights grey and muddy. It is why
  cheap bloom looks like fog instead of glow.
- **A lift/gamma/gain wheel on linear data** crams every useful adjustment into
  the bottom ~3% of the control's travel.

Enforcement is structural. `EffectDef::space` has no default, so adding an
effect without deciding does not compile. The renderer inserts the transform;
**no effect ever converts its own input**. See `pe_effects::registry` for the
current assignments and the reasoning per effect.

## Why ACES

Working spaces are `ACEScg` (linear) and `ACEScct` (log), both on AP1 primaries.

- **Documented and freely implementable**, unlike Resolve's proprietary DaVinci
  Wide Gamut / Intermediate.
- **ACEScct is designed for grading.** Its linear toe below 0.0078125 is the
  reason lift controls behave in deep shadows; pure log has unbounded contrast
  near black and makes them erratic.
- **AP1 encloses Rec.709 and Display P3**, so importing an sRGB JPEG needs no
  gamut compression and is exactly lossless. Asserted by
  `space::tests::ap1_encloses_srgb`.
- **Third-party film-emulation LUTs already target ACES**, so the film-stock
  work has an ecosystem instead of a bespoke conversion to maintain forever.

## Things that are easy to get wrong

**Transfer functions belong to texture formats; gamut rotation belongs to
shaders.** The source is `Rgba8UnormSrgb` and the surface is an `...Srgb`
format, so the hardware applies the sRGB EOTF on sample and the OETF on write,
exactly and for free. Doing either in a shader as well double-applies it.

**Every intermediate is `Rgba16Float`.** 8-bit banding appears the instant a
user pushes a curve and is not recoverable downstream, and the working gamut
carries values a unorm format cannot represent at all: highlights above diffuse
white, and negative channels where a colour does not fit a narrower gamut.
`golden::an_8bit_intermediate_loses_precision_in_proportion_to_gamut_width`
measures the cost of getting this wrong.

**Transfer functions are sign-preserving.** Negatives are mirrored through the
origin, never clamped. Clamping inside a transfer function bakes in a hue shift
before gamut mapping has had a chance to handle it properly.

**Illuminants are defined by tristimulus values, not by rounded xy pairs.**
Deriving D65 from `(0.3127, 0.3290)` puts the sRGB matrix ~2e-4 away from every
published one. Deriving it from `(0.95047, 1.0, 1.08883)` lands within 5e-8.
See `primaries::D65`.

**AP1 is D60 and sRGB is D65.** Every conversion between them needs Bradford
chromatic adaptation or greys pick up a warm cast. `neutral_grey_stays_neutral`
is the most sensitive test in the suite for exactly this reason.

**ACEScct black is not zero.** Linear 0.0 encodes to 0.0729. Any shader
assuming "0 means black" is wrong in log space, and that assumption is the most
common cause of crushed shadows.

**Spatial effects scale with image dimensions, not pixels.** Grain size,
halation radius and vignette falloff are in physical or image-relative units.
A pixel radius makes a 6000px export look nothing like the 1200px preview the
user dialled in — grain becomes invisible fizz, halation shrinks to a rim.
`spatial_radii_are_not_expressed_in_pixels` guards it as far as a test can.

## The CPU reference path

`pe-color` computes in `f64` and is a complete, GPU-free implementation of the
pipeline. It is the oracle: golden tests render through it, and the GPU is
expected to agree to within a bit or two. Any larger disagreement is a shader
bug, and having somewhere to stand while diagnosing one is worth the small cost
of maintaining it.
