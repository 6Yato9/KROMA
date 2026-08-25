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

---

## Vignette

| Group | Parameter | Default | Range | Status |
|---|---|---|---|---|
| Main | Operating Mode | Basic | Basic / Advanced | confirmed |
| Shape | Size | 0.500 | 0…1 | confirmed |
| Shape | Anamorphism | 1.780 | 0…3 | confirmed |
| Appearance | Softness | 0.500 | 0…1 | confirmed |
| Appearance | Color | black | — | confirmed |
| Advanced | Border Shape, Rotation, Center X/Y | — | — | inferred (membership) |

**Anamorphism is a frame ratio**, not an offset — 1.0 is a circle and 1.78 is
16:9, which is why Resolve starts there. Ours ran −1…1 around zero; that was
the same control in different clothes and the numbers did not match.

**There is no Amount.** Resolve's Basic set has none: a subtle vignette is a
lower Global Blend, which is this row's own Blend. Ours also went negative to
*brighten* the corners, which Resolve cannot do — a real capability, given up
for the panel matching.

Which controls sit under Advanced is inferred: the screenshot shows Basic, so
it can only prove what Basic contains. Border Shape, Rotation and Center are
the plausible remainder, and they are dimmed until the mode is switched.

## Noise Reduction

Only the spatial half. Temporal NR — Frames Either Side, Mo. Est. Type, Motion
Range, and the entire Temporal Threshold group — compares a frame against its
neighbours, and a photograph has none.

| Group | Parameter | Default | Range | Status |
|---|---|---|---|---|
| Spatial NR | Mode | Faster | Faster / Better / Enhanced | confirmed |
| Spatial NR | Radius | Small | Small / Medium / Large | confirmed |
| Spatial Threshold | Split Luma Chroma | off | — | confirmed |
| Spatial Threshold | Threshold | 0.0 | 0…100 | confirmed |
| Spatial Threshold | Blend | 0.0 | 0…100 | confirmed |

Radius is a **dropdown**, not a slider — ours was a float. And Split Luma
Chroma is a real switch: unticked there is one Threshold, ticked there are two.
Ours always showed both, which meant every user of this effect was looking at a
control Resolve would have hidden from them.

Blend is how much of the *original* is blended back over the cleaned picture,
so zero is the full effect. That is why zero is the default: the control that
decides whether anything happens at all is the Threshold above it.

## Radial Blur and Zoom Blur

| Parameter | Radial | Zoom | Status |
|---|---|---|---|
| Smooth Strength / Zoom Amount | 0.400 | 0.400 | confirmed |
| Blur Type | Realistic | Realistic | confirmed |
| Blur Symmetry | Symmetric | — | confirmed |
| Center Exclusion | — | 0.000 | confirmed |
| Channel Adjustment R/G/B | 1.000, 0…2 | 1.000, 0…2 | confirmed |
| Center Position X/Y | 0.500 | 0.500 | confirmed |
| Quality | Better | Better | confirmed |
| Border Type | Replicate | greyed out | confirmed |
| Move With Sizing | on | on | confirmed |

The two are nearly the same effect and Resolve gives them nearly the same
panel, with two differences that are worth honouring rather than smoothing
over. **Zoom Blur has no Blur Symmetry** — a one-sided zoom reads as a scale
change rather than as motion. And **its Border Type is greyed out
permanently**, not conditionally, so there is no such control here; the edge is
simply held, which is what its Replicate would have done anyway.

**Center Exclusion** was missing from ours and is the control that makes a zoom
blur usable: it holds a disc around the centre sharp, so the subject sits
somewhere the blur is *absent* rather than merely weakest.

**Move With Sizing** was also missing. On, the centre belongs to the
photograph and stays put when the picture is cropped, panned or zoomed; off, it
belongs to the output and the blur stays where it is on screen while the
picture moves under it.

Channel Adjustment runs to two, not one. Past one the mix extrapolates — the
channel is pushed further from the original than the blur itself went — which
is a stronger streak rather than a dead half of a control.

The Alpha entry in Channel Adjustment and Use Alpha at the bottom are absent
from both: there is no alpha channel here.

---

## Cinematic Haze

Resolve's **AI Cinematic Haze**, minus the AI — and the name says so. Theirs
estimates depth with a network; ours estimates it from the picture with the
dark-channel prior, which is the same observation Dehaze already runs
backwards. Shipping a hand-written estimator under a label that says AI would
be a claim made to our own user, so the effect is called Cinematic Haze.

The prior's limitation is worth knowing: it reads bright neutral subjects —
snow, white walls, overcast sky — as distant, because "nothing in this patch is
dark" is exactly what haze looks like to it.

| Group | Parameter | Default | Range | Status |
|---|---|---|---|---|
| Depth Map | Depth Map Preview | off | — | confirmed |
| Depth Map | Quality | Better | Faster / Better / Best | confirmed |
| Depth Map | Invert | **on** | — | confirmed |
| Depth Map | Adjust Map Levels | **on** | — | confirmed |
| Depth Map | Far Limit / Near Limit | 0.000 / 1.000 | 0…1 | confirmed |
| Depth Map | Gamma | 1.000 | 0…2 | inferred (range) |
| Atmospheric Scattering | Airlight | 0.400 | 0…1 | confirmed |
| Atmospheric Scattering | Density | 0.100 | 0…1 | confirmed |
| Atmospheric Scattering | Resolution Loss | 0.500 | 0…1 | confirmed |
| Atmospheric Scattering | Colorize | white | — | confirmed |
| Light Halos | Halo Threshold | 0.650 | 0…1 | confirmed |
| Light Halos | Size | 1.000 | 0…2 | inferred (range) |
| Light Halos | Brightness | 0.250 | 0…1 | confirmed |
| Light Halos | Saturation | 1.000 | 0…2 | inferred (range) |
| Light Halos | Colorize | white | — | confirmed |
| Light Rays | Enable | off | — | confirmed |
| Light Rays | Preview Threshold | off | — | confirmed |
| Light Rays | Source Threshold | 0.700 | 0…1 | confirmed |
| Light Rays | Ray Directions | At An Angle | — | inferred (other options) |
| Light Rays | Angle | 0.0 | −180…180 | inferred (range) |
| Light Rays | Length / Soften | 0.750 / 0.150 | 0…1 | confirmed |
| Light Rays | Brightness / Saturation | 0.300 / 1.000 | 0…1, 0…2 | confirmed / inferred |
| Air Disturbance | Enable / Preview Influence | off / off | — | confirmed |
| Air Disturbance | Intensity | 0.250 | 0…1 | confirmed |
| Air Disturbance | Brightness | 1.000 | 0…1 | confirmed |
| Air Disturbance | Scale | 2.000 | 0…6 | inferred (range) |
| Air Disturbance | Detail | 7.00 | 1…16 | inferred (range) |
| Air Disturbance | Start Frame | 0 | 0…1000 | inferred (range) |

**Absent, and why.** Colour Space Overrides is the renderer's decision under
the two-space rule. **Depth Map Source** is a dropdown with one option here —
there is no external depth input to choose — and a dropdown with one option is
a dead control. **Advanced Depth Controls** reveals controls no screenshot
shows, and a switch that reveals nothing is worse than a missing one. **Follow
FX Tracker** needs a tracker. **Flow Speed**, **Seethe Rate** and **Randomize
Start Frame** describe how the field changes between exposures.

**Start Frame stays**, and it is the interesting one: for a single exposure it
is not a time at all, it is which slice of the turbulence you got — which is a
seed, and a useful control.

It is a **look** effect, so it ships visible. That is not obvious, given Dehaze
is corrective and this is its mirror image, but they are different kinds of
tool: Dehaze answers "there is haze here I did not want", and this answers "I
want haze here". A haze effect that adds no haze until you find the right
slider reads as broken.

---

## Sharpen, Sharpen Edges, Soften and Sharpen

Three takes on the same idea — take the picture apart into scales, change each
band, put it back — and Resolve ships all three because they answer different
questions. Sharpen pushes every band; Sharpen Edges pushes only where there is
an edge, so sky and skin keep their noise instead of having it amplified; and
Soften and Sharpen makes each band *bipolar*, so medium at −0.8 with small at
+0.3 is skin that keeps its pores and loses its blotches.

The decomposition lives once in `shaders/common.wgsl`. Three copies would put
"medium detail" at three different sizes, and the same slider would mean
different things in each effect.

| Effect | Parameter | Default | Range | Status |
|---|---|---|---|---|
| Sharpen | Sharpen Amount | 1.800 | 0…10 | confirmed / inferred range |
| Sharpen | Fine Detail Size | 0.050 | 0…0.12 | confirmed / inferred range |
| Sharpen | Fine, Medium, Large Details | 1.000 | 0…10 | confirmed / inferred range |
| Sharpen | Sharpen Chroma | 1.000 | 0…10 | confirmed / inferred range |
| Sharpen Edges | Sharpen Amount | 2.000 | 0…10 | confirmed / inferred range |
| Sharpen Edges | Sharpen Radius | 0.050 | 0…0.12 | confirmed / inferred range |
| Sharpen Edges | Display Edges | off | — | confirmed |
| Sharpen Edges | Pre Denoise | 0.100 | 0…1 | confirmed |
| Sharpen Edges | Edge Detect Thr | 0.200 | 0…1 | confirmed |
| Sharpen Edges | Edge Mask Strength | 2.000 | 0…5 | confirmed / inferred range |
| Sharpen Edges | Edge Blur | 0.200 | 0…1 | confirmed |
| Soften and Sharpen | Small / Medium / Large Texture | 0.000 | −1…1 | confirmed |
| Soften and Sharpen | Small Texture Size | 0.100 | 0…0.25 | confirmed / inferred range |

**Every range here is inferred from handle position**, which is worth being
plain about: the defaults are read off the panel and are right, but a slider
showing 1.000 with its handle hard left says only "the maximum is much larger
than one". Ten is a guess that makes the control usable.

Sharpen and Sharpen Edges ship *visible*, because Resolve ships them at 1.8
and 2.0. Both still carry a neutral of zero, so the reset arrow gives a row
that does nothing.

## Lens Distortion

| Parameter | Default | Range | Status |
|---|---|---|---|
| Split Channels | off | — | confirmed |
| Distortion | **−0.400** | −1…1 | confirmed (worth re-checking) |
| Fine Adjustment | off | — | confirmed |
| Position X / Y | 0.500 | 0…1 | confirmed |
| Edge Behaviour | Black | Black / Replicate / Mirror / Wrap | confirmed / inferred options |

**Split Channels is lateral chromatic aberration** — each channel distorted by
a slightly different amount. That is the same optical failure, so the same
control undoes it, which is why there is no separate CA effect.

The −0.400 default is flagged because a corrective tool that warps the picture
the moment it is added is unusual. It is what the panel showed, so it is what
we ship, with a neutral of zero.

## Dirt Removal

Resolve's **Automatic Dirt Removal**, made single-frame — and that is not a
trim, it is a weaker test.

Theirs finds dirt by *motion*: a speck is present in this frame and absent from
its neighbours, which is close to proof. Motion Est. Type, Neighbor Frames and
Motion Thr. are that test, and a photograph has no neighbours to run it
against.

What a still can test is weaker: a speck is a small spot that disagrees with
everything around it. That finds sensor dust and scanning dirt well, and it
will also find a distant bird. **Show Repair Mask is not a nicety here** — it
is how you check the weaker test did not take something you wanted.

| Parameter | Default | Range | Status |
|---|---|---|---|
| Repair Strength | 0.900 | 0…1 | confirmed |
| Dirt Size Thr. | 0.100 | 0…1 | confirmed |
| Show Repair Mask | off | — | confirmed |
| Edge Ignore | 0.000 | 0…1 | confirmed |

Edge Ignore is at the top level rather than under Resolve's "Fine Controls"
heading: the only other control there was Motion Thr., and a heading with one
control under it costs a click and buys nothing.

It is deliberately **not** a visible-default effect even though it opens at 0.9
— on a photograph with no dirt in it that is correctly no change at all, and
"visible at its defaults" has to mean visible on any picture.

---

## Colour Warper

Three windows onto one object. Resolve draws hue against saturation as a
hexagonal web, and chroma against luma as two rectangular grids about two
chromaticity axes — the axes change, the lattice does not. That is why the
views switch on an icon rather than being three tools, and why there is one
`pe_core::Warp` behind all of them.

| Parameter | Default | Range | Status |
|---|---|---|---|
| Hue Divisions | 6 | 4 / 6 / 8 / 12 / 16 | confirmed |
| Saturation Divisions | 6 | as above | confirmed |
| Chroma Divisions | 6 | as above | confirmed |
| Luma Divisions | 6 | as above | confirmed |
| Axis Angle | 0.00 | −180…180 | confirmed / inferred range |

**The lattice is not a slider set.** It is stored as a displacement per vertex
— zero for an untouched grid — and travels to the GPU inside the curve LUT,
two rows per grid from row 10. A 16 by 16 grid is 256 vertices, which is
exactly one LUT row per component, and that is where the 16 comes from.

**A pin is a chromaticity, not a fraction.** `Pin::at` and `Pin::to` are CIE xy
coordinates, and the plot they are drawn on runs from `PLOT_MIN` to
`PLOT_SPAN` — −0.03 to 0.88, chosen so the spectral locus, which reaches 0.8338
in y, sits inside the frame rather than running out of it. `plot_fraction` is
the conversion. This is worth stating because the field comments said "0..1"
for a long time, and 0.33 read as a fraction of the plot lands somewhere
entirely different from 0.33 read as a chromaticity: plausible, and completely
wrong.

**A divisions control and its lattice are one thing.** The shader reads a
grid's size from the divisions choice, while the lattice carries its own
`cols`/`rows` and is uploaded that many offsets, row-major. They have to agree:
disagree, and the renderer indexes a 6 by 6 grid as though it were 8 by 8 and
reads real displacements from the wrong vertices. `Session::set_choice` keeps
them in step, resizing the lattice whenever the choice moves, and does both
inside one interaction so a single undo cannot put them back out of step.

Resizing **resamples** rather than clearing. Somebody who has spent a minute
pulling a grid around and then wants it finer is asking for more control
points, not for their work back — and "changing the resolution discards the
edit" is the kind of behaviour that teaches people never to touch a control.

**Hue wraps and chroma does not.** The vertex at the far right of the hue grid
*is* the one at the far left; treating it as an edge leaves a seam at red no
amount of dragging can smooth. Chroma's two ends are grey and full colour,
which are as far apart as two colours get. The interpolation is written twice —
`Warp::sample` and `colour_warper.wgsl` — and each says so.

It lives on the **Colour page**, with the curves and the wheels, rather than
in the effects list. It was an ordinary effect first and that was the wrong
home: everything else on Resolve's colour page is simply *there*, and a tool
you have to remember the name of in order to find is a tool nobody finds.

**Chroma Warp** is the third view, and the one that is not the same object.
The grids ask what happens to *every* colour; a pin asks what happens to
**this** one, and a picture usually has two or three colours anybody has an
opinion about.

| Pin parameter | Default | Range | Status |
|---|---|---|---|
| Chroma Range | 0.040 | 0…0.5 | confirmed / inferred range |
| Tonal Range Low | 1.000 | 0…1 | confirmed |
| Tonal Range High | 1.000 | 0…1 | confirmed |
| Tonal Range Pivot | 0.500 | 0…1 | confirmed |
| Exposure | 0.000 | −2…2 | confirmed / inferred range |

They are greyed in the screenshot because no pin was selected, and they are
greyed here for the same reason.

**All three plots show the photograph's own colours**, as a translucent haze
over the space. This is what turns the warper from a diagram into a tool: a
grid over a plot of colour *in general* tells you nothing about the picture in
front of you, and you would be aiming a pin at where greens usually are rather
than at the green you can see. Resolve draws it on all three for that reason.

Three measurements rather than one, because the three views are three
projections and a cloud measured for one says nothing on another. They are
counted in a single pass, additive and white so the haze reads as measurement
rather than as part of the plot underneath, and scaled by a fourth root — a
photograph's colours are wildly unevenly distributed, and against a linear
scale the red jacket is invisible beside the sky. Seeing the jacket is the
entire point.

**It works on chromaticity, not on hue and saturation.** CIE xy is what the
plot draws and what a pin is placed on: moving a colour there changes its hue
and its purity together, holds its luminance still, and means the same thing to
a colourist and to a spectrophotometer. The AP1 matrices in the shader are
generated from the primaries by `pe-color/tests/print_matrix.rs` rather than
transcribed — a matrix typed in by hand is a matrix with a digit wrong in it.

**The reach is measured from where the pin was placed**, not from where it has
been dragged to. The pin marks a colour in the picture and then says where that
colour should go; measuring from the destination would make the selection slide
out from under you as you drag it.

Eight pins, twelve floats each, in one LUT row. No count is stored: pins are
written from index zero with no gaps and an unused slot is all zeros, so a
range of zero *is* the end of the list — and one texture fetch answers "are
there any pins at all", which is the answer on almost every frame.

---

## Primaries — Colour Wheels and Log Wheels

| Wheel | Default | Range | Master ring | Status |
|---|---|---|---|---|
| Lift | 0.00 | −1…1 | yes | confirmed / **inferred range** |
| Gamma | 0.00 | −1…1 | yes | confirmed / **inferred range** |
| Gain | 1.00 | 0.01…16 | yes | confirmed |
| Offset | 25.00 | −175…255 | readout: no | confirmed |
| Log Shadow | 0.00 | −8…1 | readout: no | confirmed |
| Log Midtone | 0.00 | −1…1 | readout: no | confirmed |
| Log Highlights | 0.00 | −1…8 | readout: no | confirmed |
| Log Offset | 25.00 | −175…255 | no | confirmed |
| ↓ Range | 0.333 | 0…1 | — | confirmed |
| ↑ Range | 0.550 | 0…1 | — | confirmed |

**The stored number is the number in the box.** Gain reads 1.00 when it is
doing nothing and Offset reads 25.00, so that is what the document holds — the
value in the box is what a colourist checks against a reference, and a panel
reading 0.00 where Resolve reads 1.00 would be lying about what it is. The
conversion happens in the shader: gain multiplies, and offset is measured from
25 and scaled.

**Every wheel has the ribbed bar under it, Offset included.** What Offset and
the log wheels lack is the fourth *readout box*. Those are two controls wearing
one idea: the box is an achromatic value you can read, the bar is an achromatic
nudge you cannot. Resolve draws four bars and three of Offset's boxes, and on a
wheel with no master the bar moves the three channels together.

**The ranges are lopsided on purpose, and in opposite directions.** Shadows
have a long way down and highlights a long way up, because that is where the
room in a picture actually is: crushing a shadow to nothing is a real thing to
want and crushing a highlight to nothing is not. The midtone band has neither
tail — there is nowhere for the middle of a picture to go except somewhere it
can come back from.

**Gain is a linear multiply**, so its range is in stops of light: 2.0 is one
stop up, 16.0 is four, 0.01 is most of the way to black. Done on the log signal
instead it would scale the encoding, which passes for a gain at small pushes
and is unrecognisable at the top of the range. That is the same deliberate
exception Film Look Creator's glow sections make, for the same reason.

A unit on a log wheel is worth 0.15 in log. An SDR picture spans about 0.48 of
the log range in total, so a unit cannot be a unit: at this scale a full push
of one is about a third of the range, and the long tails run out to a shadow
fully crushed or a highlight fully blown.

Lift and Gamma's ranges are still inferred — the screenshots give their values
but not their endpoints.

**One value deliberately not matched.** Resolve's Pivot reads 0.435; ours is
0.4136. Theirs is mid grey in *their* log space and ours is mid grey in
ACEScct, where 18% scene grey lands at 0.413589. Copying the number would put
our pivot slightly above mid grey for no reason at all, so the meaning is
matched instead of the digits. There is a test pinning it.

## Reset

Reset restores the parameter's **default** — the value the effect arrives
with — everywhere in the application.

It used to restore *neutral* for individual parameters, on the argument that a
reset should always mean "do nothing". That argument lost to a simpler one: the
reset arrow on an effect's title bar has always restored defaults, so the same
icon meant two different things depending on which row of the panel it sat in.
Radial Blur was where it showed — resetting Smooth Strength gave 0.000 where
resetting the whole effect gave 0.400.

`neutral` is still what a slider draws its fill from, which is the question it
actually answers: where does this control stop doing anything.
