//! CRC-32/ISO-HDLC, the checksum used by Java's `CRC32` and ZIP.

/// A streaming ISO-HDLC checksum. Pick the SIMD backend only on CPUs that
/// meet its runtime requirements; other CPUs keep the portable backend.
pub struct Crc32(Backend);

enum Backend {
    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    Fast(crc_fast::Digest),
    Portable(crc32fast::Hasher),
}

impl Crc32 {
    pub fn new() -> Self {
        #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
        if accelerated() {
            return Self(Backend::Fast(crc_fast::Digest::new(
                crc_fast::CrcAlgorithm::Crc32IsoHdlc,
            )));
        }
        Self(Backend::Portable(crc32fast::Hasher::new()))
    }

    pub fn checksum(bytes: &[u8]) -> u32 {
        #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
        if accelerated() {
            return crc_fast::crc32_iso_hdlc(bytes);
        }
        crc32fast::hash(bytes)
    }

    pub fn update(&mut self, bytes: &[u8]) {
        match &mut self.0 {
            #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
            Backend::Fast(digest) => digest.update(bytes),
            Backend::Portable(digest) => digest.update(bytes),
        }
    }

    pub fn finalize(self) -> u32 {
        match self.0 {
            #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
            Backend::Fast(digest) => digest.finalize() as u32,
            Backend::Portable(digest) => digest.finalize(),
        }
    }
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
fn accelerated() -> bool {
    static USE_FAST: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *USE_FAST.get_or_init(|| {
        // crc-fast 1.10.0 selects its AES tiers without testing CRC.
        #[cfg(target_arch = "aarch64")]
        {
            std::arch::is_aarch64_feature_detected!("aes")
                && std::arch::is_aarch64_feature_detected!("crc")
        }
        // The VPCLMULQDQ tier additionally needs AVX-512BW; crc-fast does
        // not test that feature. Check the prerequisite AVX-512F too.
        #[cfg(target_arch = "x86_64")]
        {
            !std::arch::is_x86_feature_detected!("avx512vl")
                || (std::arch::is_x86_feature_detected!("avx512f")
                    && (!std::arch::is_x86_feature_detected!("vpclmulqdq")
                        || std::arch::is_x86_feature_detected!("avx512bw")))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamed_crc32_matches_the_iso_hdlc_check_value() {
        for mut checksum in [
            Crc32::new(),
            Crc32(Backend::Portable(crc32fast::Hasher::new())),
        ] {
            checksum.update(b"1234");
            checksum.update(b"");
            checksum.update(b"56789");
            assert_eq!(checksum.finalize(), 0xcbf4_3926);
        }
    }
}
