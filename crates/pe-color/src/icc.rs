//! Recognising an embedded ICC profile, well enough to say which space it is.
//!
//! Not an ICC implementation and not trying to be. The question this answers is
//! narrow: a photograph arrived with a profile attached, and we would like to
//! know whether it is one of the handful of spaces the pipeline already
//! understands. Anything else, we say so and the caller keeps its default.
//!
//! That narrowness is the point. A full profile engine would have to handle
//! LUT-based profiles, rendering intents, black point compensation and a
//! decade of vendor quirks. Reading three tags does not, and three tags is all
//! it takes to tell an iPhone's Display P3 from a scanner's sRGB — which is the
//! difference that was making photographs render flat.

use crate::primaries::{self, Chromaticity, Primaries};
use crate::space::{self, ColorSpace};

/// Where the tag count sits: straight after the 128-byte header.
const TAG_COUNT_AT: usize = 128;
/// Signature, offset, size.
const TAG_ENTRY: usize = 12;

/// How close a profile's primaries must be to a known space's, in xy.
///
/// Loose enough for the rounding that a fixed-point round trip through the
/// profile connection space introduces, tight enough that sRGB and Adobe RGB
/// cannot be mistaken for each other — they share a red primary exactly and
/// differ in green by 0.09, which is forty times this.
const TOLERANCE: f64 = 0.0022;

/// Identify a matrix/TRC display profile by its primaries.
///
/// `None` means "not recognised", which covers a malformed profile, a
/// LUT-based one, and a perfectly good space we do not have. All three want the
/// same response from a caller: leave whatever you were going to assume.
pub fn identify(profile: &[u8]) -> Option<&'static ColorSpace> {
    let found = read_primaries(profile)?;
    space::ALL.iter().find(|candidate| {
        // The primaries alone, deliberately. Distinguishing sRGB from Linear
        // sRGB would mean parsing the tone curve, and `ALL` lists the encoded
        // variant first so the first match is the one a *file* would be in.
        // An 8-bit photograph carrying a linear profile is not a thing that
        // happens; a photograph carrying an sRGB one is nearly all of them.
        close(candidate.primaries.red, found.red)
            && close(candidate.primaries.green, found.green)
            && close(candidate.primaries.blue, found.blue)
    })
}

fn close(a: Chromaticity, b: Chromaticity) -> bool {
    (a.x - b.x).abs() < TOLERANCE && (a.y - b.y).abs() < TOLERANCE
}

/// Pull the three colorant tags out and turn them back into chromaticities.
///
/// The colorants are stored adapted to D50, because that is the profile
/// connection space — so they are not the numbers a specification would quote
/// for the same gamut. Adapting back to D65 with Bradford is what makes them
/// comparable, and it is the same adaptation the profile applied going the
/// other way.
/// The adaptation the profile says it applied, if it says.
///
/// `chad` holds the matrix that took the colorants from the medium's own white
/// to D50. Inverting it undoes exactly that, which beats guessing — and
/// guessing was what this did: it assumed D65 and so read every profile with
/// another white point, ACES among them, as a gamut it is not.
///
/// Optional in version 2 and frequently absent, so the D65 assumption stays as
/// the fallback. It is right for every display profile a photograph is likely
/// to carry, which is the case that matters; it is simply no longer the only
/// thing on offer.
fn undo_adaptation(profile: &[u8], count: usize) -> Option<crate::Mat3> {
    let m = sf32_tag(profile, count, b"chad")?;
    crate::Mat3(m).inverse()
}

fn read_primaries(profile: &[u8]) -> Option<Primaries> {
    // A profile declares its own length. Trusting the slice instead would let a
    // truncated file read whatever happens to follow it in memory.
    let declared = be_u32(profile, 0)? as usize;
    if declared < TAG_COUNT_AT + 4 || declared > profile.len() {
        return None;
    }
    let profile = &profile[..declared];

    let count = be_u32(profile, TAG_COUNT_AT)? as usize;
    // Sixty-four tags is already an unusual profile; ten thousand is a corrupt
    // length being taken at face value.
    if count > 1024 {
        return None;
    }

    let mut columns = [[0.0f64; 3]; 3];
    for (i, want) in [b"rXYZ", b"gXYZ", b"bXYZ"].iter().enumerate() {
        columns[i] = xyz_tag(profile, count, want)?;
    }

    // Columns are R, G, B in XYZ under D50. Undo the adaptation the profile
    // recorded; failing that, assume it came from D65, which every display
    // profile a photograph carries did.
    let (back, white) = match undo_adaptation(profile, count) {
        Some(m) => (m, wtpt_white(profile, count).unwrap_or(primaries::D65)),
        None => (
            primaries::bradford_adaptation(primaries::D50, primaries::D65),
            primaries::D65,
        ),
    };
    let red = chromaticity(back.mul_vec(columns[0]))?;
    let green = chromaticity(back.mul_vec(columns[1]))?;
    let blue = chromaticity(back.mul_vec(columns[2]))?;

    Some(Primaries {
        red,
        green,
        blue,
        white,
    })
}

/// Find one XYZType tag and read its three fixed-point numbers.
/// Where one tag's data starts and how long it is.
fn find_tag(profile: &[u8], count: usize, signature: &[u8; 4]) -> Option<(usize, usize)> {
    for i in 0..count {
        let entry = TAG_COUNT_AT + 4 + i * TAG_ENTRY;
        if profile.get(entry..entry + 4)? != signature {
            continue;
        }
        return Some((
            be_u32(profile, entry + 4)? as usize,
            be_u32(profile, entry + 8)? as usize,
        ));
    }
    None
}

fn xyz_tag(profile: &[u8], count: usize, signature: &[u8; 4]) -> Option<[f64; 3]> {
    {
        let (at, size) = find_tag(profile, count, signature)?;
        // 4 bytes of type signature, 4 reserved, then three s15Fixed16.
        if size < 20 {
            return None;
        }
        let data = profile.get(at..at.checked_add(20)?)?;
        if &data[0..4] != b"XYZ " {
            return None;
        }
        Some([
            s15_fixed16(data, 8)?,
            s15_fixed16(data, 12)?,
            s15_fixed16(data, 16)?,
        ])
    }
}

/// The medium's own white, recovered the same way the colorants are.
///
/// `wtpt` is stored adapted to D50 like everything else, so it needs the same
/// undoing. A profile without a usable one falls back on D65 at the call site.
fn wtpt_white(profile: &[u8], count: usize) -> Option<Chromaticity> {
    let stored = xyz_tag(profile, count, b"wtpt")?;
    let back = undo_adaptation(profile, count)?;
    chromaticity(back.mul_vec(stored))
}

/// Read a 3x3 from an `s15Fixed16ArrayType` tag.
fn sf32_tag(profile: &[u8], count: usize, signature: &[u8; 4]) -> Option<[[f64; 3]; 3]> {
    let (at, size) = find_tag(profile, count, signature)?;
    // Type signature, reserved, then nine numbers.
    if size < 8 + 9 * 4 {
        return None;
    }
    let data = profile.get(at..at.checked_add(8 + 9 * 4)?)?;
    if &data[0..4] != b"sf32" {
        return None;
    }
    let mut m = [[0.0f64; 3]; 3];
    for (i, cell) in m.iter_mut().flatten().enumerate() {
        *cell = s15_fixed16(data, 8 + i * 4)?;
    }
    Some(m)
}

fn chromaticity(xyz: [f64; 3]) -> Option<Chromaticity> {
    let sum = xyz[0] + xyz[1] + xyz[2];
    if !(sum.is_finite() && sum > 1e-9) {
        return None;
    }
    Some(Chromaticity::new(xyz[0] / sum, xyz[1] / sum))
}

fn be_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// ICC's s15Fixed16Number: a signed 32-bit integer with 16 fractional bits.
fn s15_fixed16(bytes: &[u8], at: usize) -> Option<f64> {
    let raw = i32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?);
    Some(f64::from(raw) / 65536.0)
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// How many points the tone curve is written with.
///
/// sRGB's curve is a line spliced to a power law, which no single gamma value
/// describes. A sampled table describes it exactly as far as an 8- or 16-bit
/// file can tell, and unlike the parametric curve types it is understood by
/// everything back to 1998.
const TRC_POINTS: usize = 1024;

/// Build a matrix/TRC profile describing a colour space.
///
/// Enough of one for a photograph: what the primaries are, what the white is,
/// and how the numbers relate to light. Not a colour engine — there is no LUT,
/// no gamut mapping table, no per-intent transform. A reader that wants those
/// is reading the wrong kind of profile, and a reader that wants to know what
/// space the file is in has everything it needs.
///
/// Deterministic, deliberately: the same space always produces the same bytes,
/// so exporting the same photograph twice produces the same file. That is why
/// the creation date is fixed rather than read off the clock.
pub fn profile_for(space: &ColorSpace) -> Vec<u8> {
    let p = &space.primaries;
    // The connection space is D50, so the colorants are stored adapted to it.
    let adapt = primaries::bradford_adaptation(p.white, primaries::D50);
    let colorants = adapt.mul(&p.rgb_to_xyz());
    let column = |i: usize| [colorants.0[0][i], colorants.0[1][i], colorants.0[2][i]];

    let curve = trc(space.transfer);
    let mut tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"desc", text_description(space.name)),
        (b"wtpt", xyz_type(primaries::D50.to_xyz())),
        (b"chad", sf32_type(&adapt.0)),
        (b"rXYZ", xyz_type(column(0))),
        (b"gXYZ", xyz_type(column(1))),
        (b"bXYZ", xyz_type(column(2))),
        (b"rTRC", curve.clone()),
        (b"gTRC", curve.clone()),
        (b"bTRC", curve),
        (b"cprt", text_type("Public Domain")),
    ];

    // Tag data starts after the header and the table.
    let table_at = TAG_COUNT_AT + 4;
    let mut out = vec![0u8; table_at + tags.len() * TAG_ENTRY];
    for (i, (signature, data)) in tags.iter_mut().enumerate() {
        // Every tag begins on a four-byte boundary.
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        let entry = table_at + i * TAG_ENTRY;
        let at = out.len() as u32;
        out[entry..entry + 4].copy_from_slice(*signature);
        out[entry + 4..entry + 8].copy_from_slice(&at.to_be_bytes());
        out[entry + 8..entry + 12].copy_from_slice(&(data.len() as u32).to_be_bytes());
        out.append(data);
    }
    out[TAG_COUNT_AT..TAG_COUNT_AT + 4].copy_from_slice(&(tags.len() as u32).to_be_bytes());

    write_header(&mut out);
    out
}

fn write_header(out: &mut [u8]) {
    let size = out.len() as u32;
    out[0..4].copy_from_slice(&size.to_be_bytes());
    // 4..8 preferred CMM: none.
    // Version 2.1. Version 4 would mean multi-localised text and parametric
    // curves, which buys nothing here and is understood by less.
    out[8..12].copy_from_slice(&0x0210_0000u32.to_be_bytes());
    out[12..16].copy_from_slice(b"mntr");
    out[16..20].copy_from_slice(b"RGB ");
    out[20..24].copy_from_slice(b"XYZ ");
    // A fixed creation date. The alternative is the clock, and then the same
    // export twice is two different files.
    for (i, v) in [2026u16, 1, 1, 0, 0, 0].iter().enumerate() {
        out[24 + i * 2..26 + i * 2].copy_from_slice(&v.to_be_bytes());
    }
    out[36..40].copy_from_slice(b"acsp");
    // 40..64 platform, flags, manufacturer, model, attributes: all unstated.
    // Rendering intent 0, perceptual.
    out[68..80].copy_from_slice(&{
        let mut bytes = [0u8; 12];
        for (i, v) in primaries::D50.to_xyz().iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&fixed16(*v).to_be_bytes());
        }
        bytes
    });
    // 80..128 creator, profile id, reserved: all zero. The id is an MD5 of the
    // profile and is optional; nothing reads it that does not also verify it.
}

/// `curveType`: the mapping from stored value to light.
fn trc(transfer: crate::TransferFn) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + TRC_POINTS * 2);
    out.extend_from_slice(b"curv");
    out.extend_from_slice(&[0; 4]);
    if transfer == crate::TransferFn::Linear {
        // A count of zero is the identity, which is both smaller and exact.
        out.extend_from_slice(&0u32.to_be_bytes());
        return out;
    }
    out.extend_from_slice(&(TRC_POINTS as u32).to_be_bytes());
    for i in 0..TRC_POINTS {
        let encoded = i as f64 / (TRC_POINTS - 1) as f64;
        let linear = transfer.decode(encoded).clamp(0.0, 1.0);
        out.extend_from_slice(&((linear * 65535.0).round() as u16).to_be_bytes());
    }
    out
}

fn xyz_type(xyz: [f64; 3]) -> Vec<u8> {
    let mut out = Vec::with_capacity(20);
    out.extend_from_slice(b"XYZ ");
    out.extend_from_slice(&[0; 4]);
    for v in xyz {
        out.extend_from_slice(&fixed16(v).to_be_bytes());
    }
    out
}

fn sf32_type(m: &[[f64; 3]; 3]) -> Vec<u8> {
    let mut out = Vec::with_capacity(44);
    out.extend_from_slice(b"sf32");
    out.extend_from_slice(&[0; 4]);
    for row in m {
        for v in row {
            out.extend_from_slice(&fixed16(*v).to_be_bytes());
        }
    }
    out
}

fn text_type(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"text");
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(text.as_bytes());
    out.push(0);
    out
}

/// `textDescriptionType`, the version 2 way of naming a profile.
///
/// The Unicode and Macintosh script-code halves are required to be present and
/// are allowed to be empty, which is what nearly every profile in the world
/// does — including the sRGB one everybody's files carry.
fn text_description(name: &str) -> Vec<u8> {
    let ascii = name.as_bytes();
    let mut out = Vec::with_capacity(90 + ascii.len());
    out.extend_from_slice(b"desc");
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(ascii.len() as u32 + 1).to_be_bytes());
    out.extend_from_slice(ascii);
    out.push(0);
    // Unicode: language code and a zero-length string.
    out.extend_from_slice(&[0; 8]);
    // ScriptCode: code, length, and a fixed 67-byte field.
    out.extend_from_slice(&[0; 3]);
    out.extend_from_slice(&[0; 67]);
    out
}

fn fixed16(v: f64) -> i32 {
    (v * 65536.0).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal but structurally real profile around three colorants.
    ///
    /// Assembled rather than checked in as a binary, so the test says what it
    /// is testing. The numbers are the D50-adapted colorants a real profile for
    /// that space carries, which is the whole thing being decoded.
    fn profile_with(r: [f64; 3], g: [f64; 3], b: [f64; 3]) -> Vec<u8> {
        let tags: [(&[u8; 4], [f64; 3]); 3] = [(b"rXYZ", r), (b"gXYZ", g), (b"bXYZ", b)];
        let table_at = TAG_COUNT_AT + 4;
        let data_at = table_at + tags.len() * TAG_ENTRY;

        let mut out = vec![0u8; data_at];
        out[TAG_COUNT_AT..TAG_COUNT_AT + 4].copy_from_slice(&(tags.len() as u32).to_be_bytes());

        for (i, (sig, xyz)) in tags.iter().enumerate() {
            let entry = table_at + i * TAG_ENTRY;
            let at = out.len() as u32;
            out[entry..entry + 4].copy_from_slice(*sig);
            out[entry + 4..entry + 8].copy_from_slice(&at.to_be_bytes());
            out[entry + 8..entry + 12].copy_from_slice(&20u32.to_be_bytes());

            out.extend_from_slice(b"XYZ ");
            out.extend_from_slice(&[0; 4]);
            for v in xyz {
                out.extend_from_slice(&(((v * 65536.0).round()) as i32).to_be_bytes());
            }
        }

        let size = out.len() as u32;
        out[0..4].copy_from_slice(&size.to_be_bytes());
        out
    }

    /// The colorants a real profile carries, taken from the space itself.
    ///
    /// Derived rather than transcribed: what a profile stores is the RGB→XYZ
    /// matrix adapted into the D50 connection space, so producing it here the
    /// same way the writer would is what makes the round trip a real test
    /// rather than a restatement of the constants.
    fn colorants(p: &Primaries) -> [[f64; 3]; 3] {
        let m = p.rgb_to_xyz();
        let to_d50 = primaries::bradford_adaptation(p.white, primaries::D50);
        let adapted = to_d50.mul(&m);
        // Columns are the primaries.
        [
            [adapted.0[0][0], adapted.0[1][0], adapted.0[2][0]],
            [adapted.0[0][1], adapted.0[1][1], adapted.0[2][1]],
            [adapted.0[0][2], adapted.0[1][2], adapted.0[2][2]],
        ]
    }

    #[test]
    fn an_srgb_profile_is_recognised() {
        let c = colorants(&primaries::SRGB);
        let found = identify(&profile_with(c[0], c[1], c[2])).expect("recognised");
        assert_eq!(found.name, "sRGB");
    }

    /// The one that was making photographs render flat.
    #[test]
    fn a_display_p3_profile_is_recognised() {
        let c = colorants(&primaries::DISPLAY_P3);
        let found = identify(&profile_with(c[0], c[1], c[2])).expect("recognised");
        assert_eq!(found.name, "Display P3");
    }

    #[test]
    fn a_rec2020_profile_is_recognised() {
        let c = colorants(&primaries::REC2020);
        let found = identify(&profile_with(c[0], c[1], c[2])).expect("recognised");
        assert_eq!(found.name, "Rec.2020");
    }

    /// Adobe RGB shares sRGB's red primary exactly and differs only in green.
    ///
    /// If the match ever drops to checking one primary, or the tolerance grows
    /// past the gap, this is the profile that starts being called sRGB — and a
    /// wide-gamut file read as sRGB is precisely the bug this module exists to
    /// stop.
    #[test]
    fn adobe_rgb_is_not_mistaken_for_srgb() {
        let adobe = Primaries {
            red: Chromaticity::new(0.64, 0.33),
            green: Chromaticity::new(0.21, 0.71),
            blue: Chromaticity::new(0.15, 0.06),
            white: primaries::D65,
        };
        let c = colorants(&adobe);
        assert!(
            identify(&profile_with(c[0], c[1], c[2])).is_none(),
            "Adobe RGB was matched to a space it is not"
        );
    }

    /// Nothing here may panic on a file that is not what it claims.
    ///
    /// A profile arrives from outside and is as trustworthy as the file it came
    /// in. Every one of these used to be a way to index past the end.
    #[test]
    fn rubbish_is_declined_rather_than_fatal() {
        assert!(identify(&[]).is_none());
        assert!(identify(&[0; 4]).is_none());
        assert!(identify(&[0xff; 200]).is_none(), "a huge declared size");

        let good = profile_with([0.4, 0.2, 0.0], [0.3, 0.7, 0.1], [0.1, 0.1, 0.7]);
        for cut in [0, 1, 100, 131, 140, good.len() - 1] {
            assert!(identify(&good[..cut]).is_none(), "survived a cut at {cut}");
        }
        // A length field promising more than arrived.
        let mut lying = good.clone();
        lying[0..4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(identify(&lying).is_none());
    }
}

#[cfg(test)]
mod writing {
    use super::*;

    /// The writer and the reader are two halves of the same claim, so making
    /// them agree is most of the test.
    ///
    /// It is not circular: the reader was written first, against profiles
    /// assembled from the specification's layout, and it declines Adobe RGB
    /// and rubbish. A profile that survives this round trip is one whose
    /// colorants are in the right tags, adapted to the right white, in the
    /// right fixed-point format — because that is what the reader is checking.
    /// Every space, including the ones with imaginary primaries.
    ///
    /// ACES2065-1's blue sits at y = −0.077 — chosen so the gamut encloses the
    /// whole visible spectrum — and it survives the trip intact, because a
    /// colorant column carries the sign through and comes back out the other
    /// side. The thing that used to break this was not the imaginary primary
    /// at all; it was the reader assuming every profile came from D65, which
    /// silently mislabelled every gamut on another white point.
    #[test]
    fn every_space_we_write_is_a_space_we_can_read_back() {
        for space in space::ALL {
            let bytes = profile_for(space);
            let found =
                identify(&bytes).unwrap_or_else(|| panic!("{} came back unrecognised", space.name));
            assert_eq!(
                found.primaries, space.primaries,
                "{} round-tripped to {}",
                space.name, found.name
            );
        }
    }

    /// The white point comes back too, not just the primaries.
    ///
    /// ACES sits on a white of its own, near D60. Reading it as D65 would put
    /// the gamut in very nearly the right place — near enough to look correct
    /// and be wrong, which is the failure this pair of functions is for.
    #[test]
    fn a_white_point_that_is_not_d65_survives() {
        let bytes = profile_for(&space::ACESCG);
        let found = identify(&bytes).expect("recognised");
        let want = space::ACESCG.primaries.white;
        assert!(
            (found.primaries.white.x - want.x).abs() < 1e-4
                && (found.primaries.white.y - want.y).abs() < 1e-4,
            "white came back at ({}, {}), should be ({}, {})",
            found.primaries.white.x,
            found.primaries.white.y,
            want.x,
            want.y
        );
    }

    /// Same space, same bytes, always.
    ///
    /// The obvious way to break this is a creation date read off the clock,
    /// which would make every export of the same photograph a different file.
    #[test]
    fn a_profile_is_the_same_bytes_every_time() {
        assert_eq!(
            profile_for(&space::DISPLAY_P3),
            profile_for(&space::DISPLAY_P3)
        );
    }

    /// The parts a reader is entitled to assume are there.
    #[test]
    fn the_header_says_what_it_should() {
        let p = profile_for(&space::SRGB);
        assert_eq!(
            u32::from_be_bytes(p[0..4].try_into().unwrap()) as usize,
            p.len(),
            "the declared size is not the actual size"
        );
        assert_eq!(&p[12..16], b"mntr");
        assert_eq!(&p[16..20], b"RGB ");
        assert_eq!(&p[20..24], b"XYZ ");
        assert_eq!(&p[36..40], b"acsp", "the signature every reader checks");
        // The connection space illuminant is D50 and is not negotiable.
        let pcs = [
            i32::from_be_bytes(p[68..72].try_into().unwrap()),
            i32::from_be_bytes(p[72..76].try_into().unwrap()),
            i32::from_be_bytes(p[76..80].try_into().unwrap()),
        ];
        assert_eq!(pcs[1], 65536, "Y of the PCS illuminant must be exactly 1");
    }

    /// Every tag has to sit inside the profile and start on a four-byte
    /// boundary, or readers that trust the table walk off the end.
    #[test]
    fn the_tag_table_is_well_formed() {
        let p = profile_for(&space::SRGB);
        let count = u32::from_be_bytes(p[128..132].try_into().unwrap()) as usize;
        assert!(count >= 9, "only {count} tags");
        for i in 0..count {
            let entry = 132 + i * 12;
            let at = u32::from_be_bytes(p[entry + 4..entry + 8].try_into().unwrap()) as usize;
            let size = u32::from_be_bytes(p[entry + 8..entry + 12].try_into().unwrap()) as usize;
            assert_eq!(at % 4, 0, "tag {i} is not aligned");
            assert!(at + size <= p.len(), "tag {i} runs past the end");
        }
    }

    /// The tone curve has to describe the curve the pipeline actually applies.
    ///
    /// A profile that says sRGB while the file was written with a plain 2.2
    /// gamma is worse than no profile: it is a wrong answer stated with
    /// confidence, and every reader will believe it.
    #[test]
    fn the_written_curve_is_the_transfer_function_we_use() {
        let p = profile_for(&space::SRGB);
        let count = u32::from_be_bytes(p[128..132].try_into().unwrap()) as usize;
        let mut curve = None;
        for i in 0..count {
            let entry = 132 + i * 12;
            if &p[entry..entry + 4] == b"rTRC" {
                let at = u32::from_be_bytes(p[entry + 4..entry + 8].try_into().unwrap()) as usize;
                curve = Some(at);
            }
        }
        let at = curve.expect("no red tone curve");
        assert_eq!(&p[at..at + 4], b"curv");
        let points = u32::from_be_bytes(p[at + 8..at + 12].try_into().unwrap()) as usize;
        assert_eq!(points, TRC_POINTS);

        // Sampled points, compared where they were sampled. The table has a
        // thousand and twenty-four entries over 0..1, so entry n describes
        // n/1023 and not the round number nearest it.
        for index in [0, 1, 256, 512, 900, TRC_POINTS - 1] {
            let encoded = index as f64 / (TRC_POINTS - 1) as f64;
            let at_point = at + 12 + index * 2;
            let stored = u16::from_be_bytes(p[at_point..at_point + 2].try_into().unwrap());
            let expected = crate::TransferFn::Srgb.decode(encoded) * 65535.0;
            assert!(
                (stored as f64 - expected).abs() <= 1.0,
                "entry {index} ({encoded}) says {stored}, the pipeline says {expected}"
            );
        }
    }
}
