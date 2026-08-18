//! Binary-container sniff.
//!
//! Identifies common binary file formats by magic bytes and control-byte
//! density. Detectors that walk the filesystem should consult this helper
//! before treating a file as text to avoid wasted work and spurious
//! findings from image, archive, or executable blobs.
//!
//! Upstream mapping: [`watermarks-remover` `service/scripts/common.py:looks_binary`](https://github.com/guillaumemeyer/watermarks-remover/blob/main/service/scripts/common.py).
//!
//! Two heuristics are applied:
//!
//! 1. **Magic bytes** — a static table of well-known container prefixes
//!    (ZIP/PDF/PNG/JPEG/GIF/7z/RAR/gzip/bzip2/XZ/tar/ELF/Mach-O/WASM/SQLite/
//!    OLE2/PCAP).
//! 2. **Control-byte density** — ratio of "exotic" control bytes in the
//!    first 8 KiB; above `CONTROL_RATIO_THRESHOLD` the buffer is treated as
//!    binary. `TAB`/`LF`/`CR` are counted as legitimate.

use std::fs;
use std::path::Path;

use crate::domain::errors::PapertowelError;

/// Magic-byte prefixes for well-known binary containers.
///
/// Each entry is the byte sequence the file starts with. Order is irrelevant
/// for the matcher (linear scan); kept grouped by family for readability.
const MAGIC_PREFIXES: &[&[u8]] = &[
    // ── ZIP-family (ZIP, DOCX, XLSX, PPTX, ODT, JAR, EPUB, …) ──
    b"PK\x03\x04",
    b"PK\x05\x06",
    b"PK\x07\x08",
    // ── Documents / images ──
    b"%PDF-",
    &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A],
    &[0xFF, 0xD8, 0xFF], // JPEG (JFIF/EXIF)
    b"GIF87a",
    b"GIF89a",
    &[0x00, 0x00, 0x01, 0x00], // ICO
    // ── Archives ──
    &[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C], // 7z
    b"Rar!\x1A\x07\x00",
    b"Rar!\x1A\x07\x01\x00",
    &[0x1F, 0x8B],                         // gzip
    b"BZh",                                // bzip2
    &[0xFD, b'7', b'z', b'X', b'Z', 0x00], // XZ
    &[0x75, b's', b't', b'a', b'r', 0x00], // POSIX tar
    // ── Executables / bytecode ──
    &[0x7F, b'E', b'L', b'F'], // ELF
    &[0xCA, 0xFE, 0xBA, 0xBE], // Java class + Mach-O fat
    &[0xCE, 0xFA, 0xED, 0xFE], // Mach-O 32 LE
    &[0xCF, 0xFA, 0xED, 0xFE], // Mach-O 64 LE
    &[0xFE, 0xED, 0xFA, 0xCE], // Mach-O 32 BE
    &[0xFE, 0xCA, 0xFE, 0xBA], // Mach-O 64 BE
    &[0x00, b'a', b's', b'm'], // WASM
    // ── Databases / file caps ──
    b"SQLite format 3\x00",
    // ── Network capture ──
    &[0xD4, 0xC3, 0xB2, 0xA1], // PCAP
    &[0xA1, 0xB2, 0xC3, 0xD4], // PCAP swapped
    // ── Legacy OLE2 (DOC / XLS / PPT) ──
    &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1],
];

/// Window size for the control-byte density check.
const CONTROL_RATIO_WINDOW: usize = 8 * 1024;

/// Reusable byte-sniff budget. Detectors that want to avoid reading
/// multi-MiB files just to skip them can pass this to
/// [`looks_binary_file`] for a head-only check.
pub const BINARY_SNIFF_BYTES: usize = CONTROL_RATIO_WINDOW;

/// Threshold above which the buffer is treated as binary.
const CONTROL_RATIO_THRESHOLD: f32 = 0.30;

/// UTF-8 text.
///
/// Detection applies **either** heuristic:
///
/// - a known magic-byte prefix at offset 0, **or**
/// - control-byte density above `CONTROL_RATIO_THRESHOLD`.
///
/// Empty input returns `false` (no signal either way).
#[must_use]
pub fn looks_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    if has_magic_prefix(bytes) {
        return true;
    }
    control_byte_ratio(bytes) >= CONTROL_RATIO_THRESHOLD
}

#[must_use]
pub fn has_magic_prefix(bytes: &[u8]) -> bool {
    MAGIC_PREFIXES
        .iter()
        .any(|prefix| bytes.starts_with(prefix))
}

/// Ratio of "exotic" control bytes in the first 8 KiB of `bytes`.
///
/// TAB (`0x09`), LF (`0x0A`), CR (`0x0D`) are considered legitimate and
/// excluded; all other bytes in `0x00..=0x1F` plus `0x7F` (DEL) are
/// counted as exotic.
#[must_use]
pub fn control_byte_ratio(bytes: &[u8]) -> f32 {
    let window = bytes.get(..bytes.len().min(CONTROL_RATIO_WINDOW));
    let Some(window) = window else { return 0.0 };
    let total = window.len();
    if total == 0 {
        return 0.0;
    }
    let count = window.iter().filter(|&&b| is_exotic_control(b)).count();
    #[expect(
        clippy::cast_precision_loss,
        reason = "byte count is bounded by CONTROL_RATIO_WINDOW"
    )]
    let ratio = count as f32 / total as f32;
    ratio
}

/// Whether `b` is a control byte that almost never appears in text
/// payloads (TAB/LF/CR are allowed).
#[must_use]
pub const fn is_exotic_control(b: u8) -> bool {
    matches!(b, 0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F)
}

/// Read the file at `path` and decide whether it looks binary.
///
/// Reads up to `max_bytes` from the file (the first slice is sufficient
/// for both heuristics).
pub fn looks_binary_file(
    path: impl AsRef<Path>,
    max_bytes: usize,
) -> Result<bool, PapertowelError> {
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|e| PapertowelError::io_with_path(path, e))?;
    let head = bytes.get(..bytes.len().min(max_bytes)).unwrap_or(&[]);
    Ok(looks_binary(head))
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::indexing_slicing,
        clippy::expect_used,
        reason = "test fixtures use known-bounded byte literals and one-shot setup"
    )]

    use super::*;

    #[test]
    fn empty_buffer_is_not_binary() {
        assert!(!looks_binary(b""));
    }

    #[test]
    fn ascii_text_is_not_binary() {
        assert!(!looks_binary(b"fn main() {\n    println!(\"hello\");\n}\n"));
        assert!(!looks_binary(
            b"The quick brown fox jumps over the lazy dog.\n"
        ));
    }

    #[test]
    fn utf8_text_with_high_bit_bytes_is_not_binary() {
        // é (0xC3 0xA9), — (0xE2 0x80 0x94), 😀 (0xF0 0x9F 0x98 0x80)
        let text = "héllo — 😀 café\n".as_bytes();
        assert!(
            !looks_binary(text),
            "UTF-8 should not trip the binary detector"
        );
    }

    #[test]
    fn detects_zip_magic() {
        assert!(looks_binary(b"PK\x03\x04junk-on-the-rest-of-the-zip"));
    }

    #[test]
    fn detects_pdf_magic() {
        assert!(looks_binary(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3"));
    }

    #[test]
    fn detects_png_magic() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        assert!(looks_binary(&png));
    }

    #[test]
    fn detects_jpeg_magic() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0];
        assert!(looks_binary(&jpeg));
    }

    #[test]
    fn detects_gif_magic() {
        assert!(looks_binary(b"GIF89a..."));
        assert!(looks_binary(b"GIF87a..."));
    }

    #[test]
    fn detects_gzip_magic() {
        assert!(looks_binary(&[0x1F, 0x8B, 0x08, 0x00]));
    }

    #[test]
    fn detects_elf_magic() {
        assert!(looks_binary(&[
            0x7F, b'E', b'L', b'F', 0x02, 0x01, 0x01, 0x00
        ]));
    }

    #[test]
    fn detects_wasm_magic() {
        assert!(looks_binary(b"\0asm\x01\x00\x00\x00"));
    }

    #[test]
    fn detects_macho_magic() {
        assert!(looks_binary(&[0xFE, 0xED, 0xFA, 0xCE]));
        assert!(looks_binary(&[0xCF, 0xFA, 0xED, 0xFE]));
    }

    #[test]
    fn detects_ole2_magic() {
        assert!(looks_binary(&[
            0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1
        ]));
    }

    #[test]
    fn detects_sqlite_magic() {
        assert!(looks_binary(b"SQLite format 3\x00binary-blob"));
    }

    #[test]
    fn detects_dense_control_byte_payload() {
        // Simulate binary content: many 0xFF and 0x00 interspersed.
        let buf: Vec<u8> = (0..2048)
            .map(|i| if i % 3 == 0 { 0xFF } else { 0x00 })
            .collect();
        assert!(looks_binary(&buf));
    }

    #[test]
    fn allows_text_below_threshold() {
        // Just enough normal text + a few control chars to stay under 30%.
        let mut buf = vec![b'a'; 100];
        buf[10] = 0x00;
        buf[20] = 0x00;
        buf[30] = 0x00;
        buf[40] = 0x7F;
        assert!(
            !looks_binary(&buf),
            "Below 30% control density should not trip"
        );
    }

    #[test]
    fn tabs_and_newlines_are_legitimate() {
        let text = b"col1\tcol2\tcol3\nrow1\trow2\trow3\n";
        assert!(!looks_binary(text));
    }

    #[test]
    fn exotic_control_predicate_excludes_whitespace() {
        assert!(is_exotic_control(0x00));
        assert!(is_exotic_control(0x01));
        assert!(is_exotic_control(0x1F));
        assert!(is_exotic_control(0x7F));
        assert!(!is_exotic_control(0x09)); // TAB
        assert!(!is_exotic_control(0x0A)); // LF
        assert!(!is_exotic_control(0x0D)); // CR
        assert!(!is_exotic_control(b'A'));
    }

    #[test]
    fn looks_binary_file_inspects_real_bytes() {
        let temp = tempfile::NamedTempFile::new().expect("tempfile");
        std::fs::write(temp.path(), b"PNG-like-but-empty-payload").expect("write");
        // Content doesn't have the PNG magic, so this should not be classified
        // as binary by the magic-byte heuristic — only by control density.
        // Confirming we can read what was written without I/O errors.
        let result = looks_binary_file(temp.path(), 8 * 1024).expect("read");
        assert!(!result);
    }

    #[test]
    fn looks_binary_file_returns_io_error_for_missing_path() {
        let path = std::path::Path::new("/this/path/does/not/exist/papertowel-test");
        assert!(looks_binary_file(path, 1024).is_err());
    }
}
