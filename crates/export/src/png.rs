//! A small PNG writer (8-bit RGB or RGBA, zlib-compressed scanlines with the Paeth / Sub /
//! Up filters chosen per row): texture images of the built-in material library and
//! textures written next to an OBJ.

use std::io::Write;

/// PNG bytes of `w` x `h` pixels, `channels` 3 (RGB) or 4 (RGBA), rows top to bottom.
pub fn encode(w: u32, h: u32, channels: u8, pixels: &[u8]) -> Vec<u8> {
    let c = usize::from(channels);
    let stride = w as usize * c;
    assert_eq!(pixels.len(), stride * h as usize, "pixel buffer size");
    let mut raw = Vec::with_capacity((stride + 1) * h as usize);
    let zero = vec![0u8; stride];
    for y in 0..h as usize {
        let row = &pixels[y * stride..(y + 1) * stride];
        let up = if y == 0 {
            &zero[..]
        } else {
            &pixels[(y - 1) * stride..y * stride]
        };
        // Pick the filter with the smallest sum of absolute residuals (the usual heuristic).
        let mut best: (u64, u8, Vec<u8>) = (u64::MAX, 0, Vec::new());
        for f in 0..5u8 {
            let out: Vec<u8> = (0..stride)
                .map(|i| {
                    let a = if i >= c { row[i - c] } else { 0 };
                    let b = up[i];
                    let cc = if i >= c { up[i - c] } else { 0 };
                    let pred = match f {
                        0 => 0,
                        1 => a,
                        2 => b,
                        3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                        _ => paeth(a, b, cc),
                    };
                    row[i].wrapping_sub(pred)
                })
                .collect();
            let score: u64 = out
                .iter()
                .map(|&v| u64::from((v as i8).unsigned_abs()))
                .sum();
            if score < best.0 {
                best = (score, f, out);
            }
        }
        raw.push(best.1);
        raw.extend_from_slice(&best.2);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    z.write_all(&raw).expect("in-memory write");
    let idat = z.finish().expect("in-memory write");
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, if channels == 4 { 6 } else { 2 }, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &idat);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = (
        (p - i16::from(a)).abs(),
        (p - i16::from(b)).abs(),
        (p - i16::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let mut h = crc32fast::Hasher::new();
    h.update(kind);
    h.update(data);
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend(h.finalize().to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_png_its_header_reads_back() {
        let px: Vec<u8> = (0..4 * 3 * 3).map(|i| (i * 29) as u8).collect();
        let png = encode(4, 3, 3, &px);
        assert_eq!(
            sawhorse_kernel::looks::image_size(&png),
            Some(("image/png", (4, 3)))
        );
        // Decompress the IDAT and undo the filters: the pixels come back.
        let len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        assert_eq!(&png[37..41], b"IDAT");
        let mut d = flate2::read::ZlibDecoder::new(&png[41..41 + len]);
        let mut raw = Vec::new();
        std::io::Read::read_to_end(&mut d, &mut raw).unwrap();
        let (c, stride) = (3usize, 12usize);
        let mut back = vec![0u8; px.len()];
        for y in 0..3 {
            let f = raw[y * (stride + 1)];
            for i in 0..stride {
                let r = raw[y * (stride + 1) + 1 + i];
                let a = if i >= c { back[y * stride + i - c] } else { 0 };
                let b = if y > 0 { back[(y - 1) * stride + i] } else { 0 };
                let cc = if y > 0 && i >= c {
                    back[(y - 1) * stride + i - c]
                } else {
                    0
                };
                let pred = match f {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                    _ => paeth(a, b, cc),
                };
                back[y * stride + i] = r.wrapping_add(pred);
            }
        }
        assert_eq!(back, px);
    }
}
