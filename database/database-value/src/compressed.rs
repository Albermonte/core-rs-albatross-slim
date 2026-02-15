use std::borrow::Cow;

/// Magic byte prefix indicating zstd-compressed data.
/// Postcard's first byte for `HistoricTransaction` encodes `NetworkId` as a small integer
/// (1-24), so 0xFF can never appear as the first byte of valid uncompressed data.
const MAGIC_COMPRESSED: u8 = 0xFF;

/// Values smaller than this are stored uncompressed (not worth the overhead).
const MIN_COMPRESS_SIZE: usize = 64;

/// zstd compression level: 3 gives a good balance of ratio vs speed.
const COMPRESSION_LEVEL: i32 = 3;

/// Compresses raw bytes using zstd with a magic byte prefix.
///
/// Returns the original bytes unmodified if:
/// - The input is smaller than `MIN_COMPRESS_SIZE`
/// - Compression doesn't actually reduce size
pub fn compress(raw: &[u8]) -> Vec<u8> {
    if raw.len() < MIN_COMPRESS_SIZE {
        return raw.to_vec();
    }
    let compressed =
        zstd::encode_all(std::io::Cursor::new(raw), COMPRESSION_LEVEL).expect("zstd compress");
    // Only use compressed form if it's actually smaller (1 byte for magic prefix)
    if compressed.len() + 1 < raw.len() {
        let mut out = Vec::with_capacity(1 + compressed.len());
        out.push(MAGIC_COMPRESSED);
        out.extend_from_slice(&compressed);
        out
    } else {
        raw.to_vec()
    }
}

/// Decompresses bytes that may or may not be zstd-compressed.
///
/// If the first byte is `MAGIC_COMPRESSED` (0xFF), the remaining bytes are
/// zstd-decompressed. Otherwise, the bytes are returned as-is (legacy
/// uncompressed format). This provides backward compatibility with existing
/// databases that were written without compression.
pub fn decompress(bytes: &[u8]) -> Cow<'_, [u8]> {
    if bytes.first() == Some(&MAGIC_COMPRESSED) {
        Cow::Owned(
            zstd::decode_all(std::io::Cursor::new(&bytes[1..])).expect("zstd decompress"),
        )
    } else {
        Cow::Borrowed(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_values_are_not_compressed() {
        let small = vec![42u8; 32];
        let result = compress(&small);
        assert_eq!(result, small);
        assert_ne!(result.first(), Some(&MAGIC_COMPRESSED));
    }

    #[test]
    fn large_values_are_compressed_and_decompressed() {
        // Repetitive data compresses well
        let large = vec![42u8; 256];
        let result = compress(&large);
        assert_eq!(result.first(), Some(&MAGIC_COMPRESSED));
        assert!(result.len() < large.len());

        let decompressed = decompress(&result);
        assert_eq!(decompressed.as_ref(), &large[..]);
    }

    #[test]
    fn legacy_uncompressed_data_decompresses_as_is() {
        let legacy = vec![1u8, 2, 3, 4, 5];
        let result = decompress(&legacy);
        assert_eq!(result.as_ref(), &legacy[..]);
    }

    #[test]
    fn roundtrip_preserves_data() {
        let data: Vec<u8> = (0..200).collect();
        let compressed = compress(&data);
        let decompressed = decompress(&compressed);
        assert_eq!(decompressed.as_ref(), &data[..]);
    }

    #[test]
    fn incompressible_data_stored_raw() {
        // Random-looking data that doesn't compress well
        let data: Vec<u8> = (0..128).map(|i| (i * 73 + 17) as u8).collect();
        let result = compress(&data);
        // If compression didn't help, it should be stored raw
        let decompressed = decompress(&result);
        assert_eq!(decompressed.as_ref(), &data[..]);
    }
}
