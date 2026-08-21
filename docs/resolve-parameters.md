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
| Minimum Saturation | slider | 0.0 | 0…1 | inferred |
| Maximum Saturation | slider | 1.0 | 0…1 | inferred |

Custom mode additionally exposes Shadow Strength/Hue and Highlight
Strength/Hue — four values replacing the single Hue Angle.

The modes, per the manual and a colourist writeup: **Natural** is "designed to
mimic the effect of film" and keeps the brightest point white, which is why
Protect Neutrals exists on it; **Strong** is "a more stylized intentional look"
where even the brightest point carries colour.

Pivot is "the point where highlights and shadows diverge". At its extremes of
0.0 or 1.0 it applies a single tint to the whole image rather than a split.

---

## Dehaze

| Parameter | Type | Default | Range | Status |
|---|---|---|---|---|
| Dehaze Strength | slider | 0.0 | −1…1 | inferred |
| Haze Color | colour picker | sampled | — | confirmed (control exists) |
| Display Depth | checkbox | off | — | confirmed |
| Shadow | slider | 0.0 | −1…1 | inferred |
| Highlight | slider | 0.0 | −1…1 | inferred |

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
| Basic Grain | Strength, Size | confirmed |

Structurally more interesting than our M1 version: isolation is a **band**
(Threshold to Normalization) rather than a single threshold, and the glow has
**two** layers with independent spread. Per-channel spread is what makes the
red/orange fringe come out naturally rather than being tinted after the fact.

Film Look Creator's simplified version is just: Highlights Only, Amount,
Radius, Saturation, Hue.

---

## Vignette

Two modes. **Basic**: Size, Anamorphism, Softness, Color. **Advanced** adds
Border Shape, Rotation, Center, Transparency, Composite Type. All confirmed as
controls; no published defaults.

Note Resolve calls it **Anamorphism**, not roundness, and it has a **Color**
control — a vignette that tints as well as darkens.

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
