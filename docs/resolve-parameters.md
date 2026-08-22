# Resolve parameter reference

Researched from the DaVinci Resolve 20 Reference Manual and the 20.1 New
Features Guide, plus values read directly off the Resolve UI. This is a
**functional specification** — parameter names, ranges and defaults — used so
our controls behave the way a colourist already expects. The algorithms behind
them are our own.

Each entry marks whether a value is **confirmed** (documented, or read off the
UI) or **inferred** (a sensible default we chose because Resolve does not
publish one). Do not quietly promote an inferred value to confirmed.

## The Blend control

Every ResolveFX plugin carries a **Blend** slider in its title bar: default
`1.0`, range `0…1`, mixing the effect against its input. **Confirmed.**

Our equivalent already exists — `StackRow::opacity` — so it is labelled
"Blend" in the inspector to match. This is one of the payoffs of putting
opacity on the row rather than inside each effect: we get Resolve's per-effect
Blend for free on every effect, including ones Resolve does not have.

Film Look Creator additionally splits this into **Color Blend** and **Effects
Blend**, the latter described as "0% is the effects completely off, and 100% is
the full output".

---

## Split Tone

New in Resolve 20.1 as a standalone effect, and also embedded in Film Look
Creator. Defaults read directly off the UI.

| Parameter | Type | Default | Range | Status |
|---|---|---|---|---|
| Split Tone Mode | dropdown | Natural | Natural / Strong / Custom | confirmed |
| Preview Influence | checkbox | off | — | confirmed |
| Strength | slider | 0.500 | 0…1 | confirmed |
| Pivot | slider | 0.300 | 0…1 | confirmed |
| Hue Angle | slider | 20.0 | 0…360 | confirmed |
| Protect Neutrals | checkbox | off | — | confirmed |

Protect Neutrals is a bare checkbox in Resolve — no saturation band under it.
We had exposed Minimum and Maximum Saturation as sliders; both are gone, and
the band is fixed in the shader. They were a second way of asking the question
the checkbox already answers.

Custom mode additionally exposes Shadow Strength/Hue and Highlight
Strength/Hue — four values replacing the single Hue Angle. Not visible in any
screenshot we have, since a screenshot of Natural cannot show them; kept, and
dimmed until Custom is chosen.

The modes, per the manual and a colourist writeup: **Natural** is "designed to
mimic the effect of film" and keeps the brightest point white, which is why
Protect Neutrals exists on it; **Strong** is "a more stylized intentional look"
where even the brightest point carries colour.

Pivot is "the point where highlights and shadows diverge". At its extremes of
0.0 or 1.0 it applies a single tint to the whole image rather than a split.

---

**Implemented:** all of them, Film Look Creator included.

## Dehaze

| Parameter | Type | Default | Range | Status |
|---|---|---|---|---|
| Dehaze Strength | slider | 0.0 | −1…1 | inferred |
| Haze Color | colour picker | sampled | — | confirmed (control exists) |
| Display Depth | checkbox | off | — | confirmed |
| Shadow | slider | 0.0 | −1…1 | inferred |
| Highlight | slider | 0.0 | −1…1 | inferred |

Implemented with a dark-channel-prior estimate: in a haze-free patch of a
natural image at least one channel is nearly black, so whatever floor a patch
has is mostly haze. We approximate the patch minimum with a golden-angle disc
rather than a true min-filter and skip the guided-filter refinement, so edges
are softer than a full implementation — both are multi-pass work. Verified
against a synthetic haze built with the same scattering model, so there is a
real ground truth rather than only "it changed".

Bipolar by design: raising it "subtly increases contrast (especially in the
shadows) while rebalancing color toward the complement of the currently
selected Haze Color"; lowering it does the reverse and *adds* haze. Shadow and
Highlight adjust the generated depth matte, not the image — Display Depth
exists so you can see what you are adjusting.

---

## Film Grain

| Group | Parameter | Status |
|---|---|---|
| Main | Film Grain Presets (8mm / 16mm / 35mm) | confirmed |
| Main | Composite Type | confirmed |
| Main | Opacity | confirmed |
| Main | Grain Only | confirmed |
| Grain Params | Texture | confirmed |
| Grain Params | Grain Size | confirmed |
| Grain Params | Grain Aspect Ratio | confirmed |
| Grain Params | Grain Strength | confirmed |
| Grain Params | Offset | confirmed |
| Grain Params | Symmetry | confirmed |
| Grain Params | Softness | confirmed |
| Grain Params | Saturation (0 = monochrome) | confirmed |
| Advanced | Red / Green / Blue | confirmed |
| Advanced | Shadow / Midtone / Highlight Gain | confirmed |

Note **Offset** and **Symmetry**, which are not obvious and are worth having:
Offset "lightens or darkens the entire simulated grain layer… lower values
emphasize lighter grains"; Symmetry is "an asymmetrical contrast adjustment"
that darkens light grains or brightens dark ones.

The Shadow/Midtone/Highlight Gain trio is Resolve's version of what we called
`shadow_bias`, and theirs is better — three independent controls rather than
one slider sliding a peak around.

---

## Halation

| Group | Parameter | Status |
|---|---|---|
| Processing | Processing Color Space | confirmed |
| Isolation | Threshold — "the low clip level" | confirmed |
| Isolation | Normalization — "the high clip level" | confirmed |
| Isolation | Film Saturation Level | confirmed |
| Isolation | View Isolated Regions | confirmed |
| Dye Layer Reflections | Strength, Gamma, Saturation, Spread | confirmed |
| Dye Layer Reflections | Fine Tune Relative Spread (R/G/B) | confirmed |
| Secondary Glow | Strength, Gamma, Spread, Filter | confirmed |
| Basic Grain | Append Grain Internally | confirmed |
| Basic Grain | Strength 0.250, Size 0.500, Softness 0.100, Saturation 0.150 | confirmed |
| Global Adjustments | View Glow Alone | confirmed |
| Global Adjustments | Reduce Highlights 0.500, Aspect Ratio 1.000, Detail Loss 0.000 | confirmed |

Values read off the panel: Threshold **0.200**, Normalization **1.000**, Film
Saturation Level **1.00**; Dye Layer Reflections Strength **0.500**, Gamma
**1.350**, Saturation **1.000**, Spread **0.333**; Secondary Glow Strength
**0.000**, Gamma **1.350**, Spread **0.600**.

Two ranges are inferred from handle position rather than read: Film Saturation
Level's maximum (the handle sits far enough left to suggest something wider
than 10, but a saturation multiplier past 10 has no use) and Aspect Ratio's.

**There is no Hue control, and Resolve is right not to have one.** Ours had
one; it is gone. The red-orange is not a tint anybody chose — it is what light
coming back through the dye layers is. It is a constant now, and Saturation
says how much of it reaches the picture, which is exactly the control Resolve
gives.

Structurally more interesting than our M1 version: isolation is a **band**
(Threshold to Normalization) rather than a single threshold, and the glow has
**two** layers with independent spread.

**Fine Tune Relative Spread** is the one worth having. With it off the glow is
one radius coloured by a Hue tint — a colour applied after the fact. With it on
each channel scatters its own distance, and the red fringe emerges from the
geometry instead: longer wavelengths penetrate the emulsion further and scatter
wider, so red outlasts green, which outlasts blue. Defaults are ordered
1.0 / 0.7 / 0.5 so ticking the box immediately gives the characteristic look,
and the Hue tint stands down to neutral so the two do not double up.

The difference is testable, which is the point: a tint gives a red/blue ratio
that is *constant* with distance, while real per-channel radii make that ratio
*grow* as you move away from the source. Only the second is physics.

Film Look Creator's simplified version is just: Highlights Only, Amount,
Radius, Saturation, Hue.

---

## Vignette

Two modes. **Basic**: Size, Anamorphism, Softness, Color. **Advanced** adds
Border Shape, Rotation, Center, Transparency, Composite Type. All confirmed as
controls; no published defaults.

Note Resolve calls it **Anamorphism**, not roundness, and it has a **Color**
control — a vignette that tints as well as darkens.

Implemented with two deliberate departures, both because the control already
exists elsewhere in our model:

- **Composite Type** is the row's blend mode. Every effect here has one.
- **Transparency** is folded into **Amount**. Two controls for "how much
  vignette" is one too many, and our Amount is bipolar so it can also brighten
  the corners, which Transparency cannot.

Border Shape is a superellipse exponent: 2 is an ellipse, ~14 approaches a box.
Worth knowing when testing it — an ellipse and a box give almost the same
radius at an *edge midpoint*, where one coordinate dominates. They diverge on
the diagonal, where an ellipse reaches sqrt(2) further. Compare corners, not
edges.

---

## Film Damage

Grouped as Blur and Shift (Film Blur, Temp Shift, Tint Shift), Add Vignetting
(Focal Factor, Geometry Factor, Tilt Amount, Tilt Angle), Add Dirt (Dirt Color,
Changing Dirt, Density, Size, Blur, Seed), and **five independent** Add Scratch
groups (Color, Position, Width, Strength, Blur, Moving, Amplitude, Speed,
Randomness, Flickering Speed). All confirmed as controls.

Five separately configurable scratches is a good detail to copy — one scratch
control with a count parameter would not let you place them.

---

## Film Look Creator, for later reference

The single most useful map of what a film look decomposes into: Film Look
Blend, Skin Bias, then Color Settings (Exposure ±5 stops, Contrast, Highlights,
Fade, White Balance in Kelvin, Tint, Skin Bias, Subtractive Sat, Richness,
Bleach Bypass), Split Tone, Halation, Bloom (Amount, Radius), Grain, Flicker
(Amount, Rate), Gate Weave (Amount, Rate) and Film Gate (Preset, Ratio H/V,
Curvature, Padding).

**Exposure is documented as ±5 stops**, which matches the range we chose
independently.

Of these, Flicker and Gate Weave are temporal and do not port to stills.
Everything else does.

---

## Sources

- [DaVinci Resolve 20 Reference Manual](https://documents.blackmagicdesign.com/UserManuals/DaVinci_Resolve_20_Reference_Manual.pdf)
- [DaVinci Resolve 20.1 New Features Guide](https://documents.blackmagicdesign.com/SupportNotes/DaVinci_Resolve_20.1_New_Features_Guide.pdf)
- Split Tone Natural/Strong behaviour and the 0.3 pivot: [Ben Chan, colourist](https://www.threads.com/@benchan_colorist/post/DN-aS8vk2E-/)
- Halation Threshold/Normalization usage: [EasyEdit](https://easyedit.pro/blog/halation-effect-in-da-vinci-resolve-18-free-and-storage-version-tips)

---

## Film Look Creator

A bundle: a film response, then the five things a print does to the light on
the way to the screen. Every one of those five is also a row of its own here,
which was the argument against having them twice — so they are not written
twice. The gathers, the vignette falloff and the grain lattice live in
`shaders/common.wgsl` and both callers reach for the same function.

All values below read off the panel.

| Group | Parameter | Default | Range | Status |
|---|---|---|---|---|
| Main | Presets | Default 65mm | — | confirmed |
| Main | Color Blend | 1.000 | 0…1 | confirmed |
| Main | Effects Blend | 1.000 | 0…1 | confirmed |
| Main | 3D LUT Compatible | off | — | confirmed |
| Film Look | Film Look Blend | 1.000 | 0…1 | confirmed |
| Film Look | Core Look | Cinematic | — | confirmed |
| Film Look | Skin Bias | 0.000 | −1…1 | inferred (range) |
| Color Settings | Exposure | 0.00 | −2…2 | inferred (range) |
| Color Settings | Contrast | 1.250 | 0…2 | confirmed |
| Color Settings | Highlights | 0.350 | 0…1 | confirmed |
| Color Settings | Fade | 0.285 | 0…1 | confirmed |
| Color Settings | White Balance | 6500 | 2000…20000 | inferred (range) |
| Color Settings | Tint | 10.0 | −100…100 | inferred (range) |
| Color Settings | Subtractive Sat | 1.200 | 0…3 | confirmed |
| Color Settings | Richness | 1.000 | 0…3 | confirmed |
| Color Settings | Bleach Bypass | 0.000 | 0…1 | confirmed |
| Split Tone | Enable / Mode / Amount / Hue Angle / Pivot | off, Natural, 0.000, 20.0, 0.300 | | confirmed |
| Vignette | Enable / Amount / Size | on, 0.250, 0.250 | 0…1 | confirmed |
| Halation | Enable / Highlights Only | on, on | — | confirmed |
| Halation | Amount / Radius / Saturation / Hue | 0.250, 4.00, 1.000, 0.500 | 0…1, 0…10, 0…1, 0…1 | confirmed |
| Bloom | Enable / Amount / Radius | on, 0.250, 10.0 | 0…1, 0…100 | confirmed |
| Grain | Enable / Preset | on, 65mm | — | confirmed |
| Grain | Amount / Size / Softness / Saturation / Image Defocus | 0.125, 0.000, 0.100, 0.300, 1.000 | 0…1 | confirmed |
| Film Gate | Enable / Preset | off, 35mm Silent | — | confirmed |
| Film Gate | Ratio H / V, Enable Curvature, Padding | 1.33 / 1.00, on, 0.000 | | confirmed |

**Deliberately absent.** Flicker and Gate Weave describe what a frame does
between exposures, and a photograph has no next frame — the same reasoning
that dropped Film Damage's temporal controls. Global Blend is the row's own
Blend, and Use Alpha needs an alpha channel we do not carry. Color Space
Overrides is the renderer's decision under the two-space rule.

**Presets does not overwrite your sliders.** In Resolve it loads a whole
configuration; here it names the *format*, and the format scales the spatial
half — a Super 8 frame is about six times smaller than 65mm, so by the time it
reaches the same screen its grain is six times bigger and its halation six
times wider. That is what actually separates the formats, and it leaves the
numbers you set as the numbers you set.

**3D LUT Compatible is a real switch, not a label.** A LUT is a function of one
colour; halation, bloom, vignette, grain and the gate all read other pixels or
the pixel's own position. With it ticked they stop, and what remains is exactly
what a 3D LUT could reproduce. The test suite uses it to isolate the colour
half, which is the best evidence it works.

## Gating

Resolve dims controls that cannot do anything: the Basic Grain sliders inside
Halation until Append Grain Internally is ticked, Secondary Glow's Gamma and
Spread until its Strength leaves zero, every Split Tone control inside Film
Look Creator until it is enabled, every Film Gate control until it is.

Worth copying, and not only for the look. A panel where a third of the controls
silently do nothing teaches the user wrong things about the effect — they move
a slider, see no change, and conclude the slider is broken rather than switched
off. `EffectDef::gates` carries these, and the inspector draws a gated row
dimmed and dead rather than hiding it, because a control that vanishes takes
the knowledge that it exists with it.
