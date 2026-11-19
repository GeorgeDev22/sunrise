//! OptiFine capes: which player names a cape server may be asked for, which of its answers can be
//! a cape, and how a cape's picture sits on the cape texture. Written from the public descriptions
//! cape makers and players give of the format (a 46x22 picture on the 64x32 cape layout, a 92x44
//! HD one on 128x64); no OptiFine code was read. The game downloads them (`skins::Downloader`);
//! the cape server's address comes from above the platform.

/// The longest answer taken, in bytes: the Sunrise launcher's bound for the same pictures.
pub const MAX_BYTES: usize = 256 * 1024;

/// The largest picture taken. The biggest HD capes are 2048x1024; a larger header is refused
/// before the picture is decoded, so a small file can't claim a huge one.
const MAX_WIDTH: u32 = 2048;
const MAX_HEIGHT: u32 = 1024;

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// Whether a cape server may be asked for `name`'s cape: a Minecraft name, 1 to 16 of `A-Z`,
/// `a-z`, `0-9` and `_`. Nothing else ever goes into the address.
pub fn valid_name(name: &str) -> bool {
    (1..=16).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Whether a cape server's answer can be a cape: a PNG of at most [`MAX_BYTES`] whose header
/// names a picture of at most 2048x1024. Anything else counts as no cape.
pub fn accepts(body: &[u8]) -> bool {
    if body.len() > MAX_BYTES || !body.starts_with(&PNG_SIGNATURE) {
        return false;
    }
    // The first chunk is IHDR: its length, its type, then the width and height, big-endian.
    let Some(
        &[
            _,
            _,
            _,
            _,
            b'I',
            b'H',
            b'D',
            b'R',
            w0,
            w1,
            w2,
            w3,
            h0,
            h1,
            h2,
            h3,
        ],
    ) = body.get(8..24)
    else {
        return false;
    };
    let (width, height) = (
        u32::from_be_bytes([w0, w1, w2, w3]),
        u32::from_be_bytes([h0, h1, h2, h3]),
    );
    (1..=MAX_WIDTH).contains(&width) && (1..=MAX_HEIGHT).contains(&height)
}

/// The cape texture a `width`x`height` picture is laid on, its top left corner on the texture's:
/// the smallest 64·2^n by 32·2^n that holds it. A 46x22 or 64x32 picture gives 64x32, a 92x44 or
/// 128x64 one 128x64, so an HD cape keeps the layout's coordinates at twice the scale.
pub fn canvas(width: u32, height: u32) -> (u32, u32) {
    let (mut w, mut h) = (64u32, 32u32);
    while (w < width || h < height) && w <= u32::MAX / 2 {
        w *= 2;
        h *= 2;
    }
    (w, h)
}

/// Whether a `width`x`height` picture also dresses an elytra: it reaches into the elytra's part
/// of the cape layout, which starts 22 texels in on the 64-wide texture (and scales with it). An
/// old 22x17 cape stops short of it, so the elytra keeps its own texture instead of a blank one.
pub fn dresses_elytra(width: u32, height: u32) -> bool {
    let (canvas_width, _) = canvas(width, height);
    u64::from(width) * 64 > 22 * u64::from(canvas_width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_minecraft_names_are_asked_for() {
        for name in ["a", "Notch", "jeb_", "Player_1234", "ABCDEFGHIJKLMNOP"] {
            assert!(valid_name(name), "{name}");
        }
        for name in [
            "",
            "ABCDEFGHIJKLMNOPQ",
            "two words",
            "../capes",
            "name.png",
            "a?b=c",
            "100%",
            "Jörg",
            "名前",
            "tab\t",
            "nul\0",
        ] {
            assert!(!valid_name(name), "{name:?}");
        }
    }

    /// A PNG's first bytes: the signature and an IHDR naming `width`x`height`.
    fn png_head(width: u32, height: u32) -> Vec<u8> {
        let mut out = PNG_SIGNATURE.to_vec();
        out.extend(13u32.to_be_bytes());
        out.extend(b"IHDR");
        out.extend(width.to_be_bytes());
        out.extend(height.to_be_bytes());
        out.extend([8, 6, 0, 0, 0]);
        out
    }

    #[test]
    fn only_small_pngs_of_a_cape_size_count() {
        assert!(accepts(&png_head(46, 22)));
        assert!(accepts(&png_head(2048, 1024)));
        let mut largest = png_head(92, 44);
        largest.resize(MAX_BYTES, 0);
        assert!(accepts(&largest));
        largest.push(0);
        assert!(!accepts(&largest), "one byte over the bound");
        assert!(!accepts(&png_head(2049, 1024)));
        assert!(!accepts(&png_head(64, 1025)));
        assert!(!accepts(&png_head(0, 32)));
        assert!(!accepts(&png_head(u32::MAX, u32::MAX)));
        assert!(!accepts(b"<html>404 Not Found</html>"));
        assert!(!accepts(&PNG_SIGNATURE), "a signature with no header");
        let mut other_chunk = png_head(46, 22);
        other_chunk[12..16].copy_from_slice(b"tEXt");
        assert!(!accepts(&other_chunk));
        assert!(!accepts(&[]));
    }

    #[test]
    fn pictures_sit_on_the_smallest_cape_texture_that_holds_them() {
        assert_eq!(canvas(64, 32), (64, 32));
        assert_eq!(canvas(46, 22), (64, 32));
        assert_eq!(canvas(22, 17), (64, 32));
        assert_eq!(canvas(1, 1), (64, 32));
        assert_eq!(canvas(92, 44), (128, 64));
        assert_eq!(canvas(128, 64), (128, 64));
        assert_eq!(canvas(65, 10), (128, 64));
        assert_eq!(canvas(10, 33), (128, 64));
        assert_eq!(canvas(184, 88), (256, 128));
        assert_eq!(canvas(2048, 1024), (2048, 1024));
        // No size loops for ever or overflows.
        assert_eq!(canvas(u32::MAX, u32::MAX), (1 << 31, 1 << 30));
    }

    #[test]
    fn a_picture_that_reaches_the_elytras_part_dresses_it() {
        for (w, h) in [(46, 22), (64, 32), (92, 44), (128, 64), (184, 88)] {
            assert!(dresses_elytra(w, h), "{w}x{h}");
        }
        for (w, h) in [(22, 17), (10, 10), (44, 34)] {
            assert!(!dresses_elytra(w, h), "{w}x{h}");
        }
    }
}
