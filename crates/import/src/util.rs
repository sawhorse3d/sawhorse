//! Small helpers shared by the readers.

use sawhorse_kernel::meshbuild::ImportItem;

/// Standard base64 (with or without padding; whitespace ignored), for glTF data URIs.
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        })
    };
    let clean: Vec<u8> = s
        .bytes()
        .filter(|c| !c.is_ascii_whitespace() && *c != b'=')
        .collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        match chunk.len() {
            4 => out.extend_from_slice(&bytes),
            3 => out.extend_from_slice(&bytes[..2]),
            2 => out.push(bytes[0]),
            _ => return None,
        }
    }
    Some(out)
}

/// A triangle-soup item: vertices as given, coplanar triangles to be merged.
pub fn soup(name: &str, positions: Vec<[f64; 3]>, faces: Vec<Vec<Vec<u32>>>) -> ImportItem {
    ImportItem {
        name: name.to_string(),
        group: None,
        tag: None,
        material: None,
        positions,
        faces,
        lines: Vec::new(),
        merge_coplanar: true,
        face_labels: Vec::new(),
        face_materials: vec![],
        face_uvs: vec![],
    }
}

/// sRGB byte from a linear 0..1 channel (glTF colour factors are linear).
pub fn linear_to_srgb(c: f64) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

pub fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        assert_eq!(base64_decode("SGVsbG8=").unwrap(), b"Hello");
        assert_eq!(base64_decode("SGVsbG8").unwrap(), b"Hello");
        assert_eq!(base64_decode("AAEC/w==").unwrap(), [0, 1, 2, 255]);
        assert!(base64_decode("@@@@").is_none());
        assert_eq!(linear_to_srgb(0.0), 0);
        assert_eq!(linear_to_srgb(1.0), 255);
        assert_eq!(linear_to_srgb(0.215_860_5), 128);
    }
}
