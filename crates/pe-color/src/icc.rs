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

    // Columns are R, G, B in XYZ under D50; move the whole thing to D65 before
    // reading chromaticities off it.
    let to_d65 = primaries::bradford_adaptation(primaries::D50, primaries::D65);
    let red = chromaticity(to_d65.mul_vec(columns[0]))?;
    let green = chromaticity(to_d65.mul_vec(columns[1]))?;
    let blue = chromaticity(to_d65.mul_vec(columns[2]))?;

    Some(Primaries {
        red,
        green,
        blue,
        white: primaries::D65,
    })
}

/// Find one XYZType tag and read its three fixed-point numbers.
fn xyz_tag(profile: &[u8], count: usize, signature: &[u8; 4]) -> Option<[f64; 3]> {
    for i in 0..count {
        let entry = TAG_COUNT_AT + 4 + i * TAG_ENTRY;
        let sig = profile.get(entry..entry + 4)?;
        if sig != signature {
            continue;
        }
        let at = be_u32(profile, entry + 4)? as usize;
        let size = be_u32(profile, entry + 8)? as usize;
        // 4 bytes of type signature, 4 reserved, then three s15Fixed16.
        if size < 20 {
            return None;
        }
        let data = profile.get(at..at.checked_add(20)?)?;
        if &data[0..4] != b"XYZ " {
            return None;
        }
        return Some([
            s15_fixed16(data, 8)?,
            s15_fixed16(data, 12)?,
            s15_fixed16(data, 16)?,
        ]);
    }
    None
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
