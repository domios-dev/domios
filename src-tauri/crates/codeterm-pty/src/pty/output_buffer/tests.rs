use super::*;

fn retain_tail(bytes: &mut Vec<u8>, cap: usize) {
    let excess = bytes.len().saturating_sub(cap);
    bytes.drain(..excess);
}

#[test]
fn ring_matches_vec_model_for_append_prepend_and_absolute_reads() {
    for short_cap in [0, 1, 7, 16] {
        for full_cap in [0, 1, 5, 19] {
            let mut buffer = OutputBuffer::new_with_caps(short_cap, full_cap);
            let mut short = Vec::new();
            let mut full = Vec::new();
            let mut total = 0;
            let mut seed = 42_u32;
            for step in 0..80 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let data: Vec<u8> = (0..(seed as usize % 25))
                    .map(|index| (index + step) as u8)
                    .collect();
                if step % 3 == 0 {
                    buffer.prepend("p", &data);
                    if !data.is_empty() {
                        short.splice(..0, data.iter().copied());
                        full.splice(..0, data.iter().copied());
                    }
                } else {
                    buffer.append("p", &data);
                    short.extend_from_slice(&data);
                    full.extend_from_slice(&data);
                }
                total += data.len() as u64;
                retain_tail(&mut short, short_cap);
                retain_tail(&mut full, full_cap);
                assert_eq!(buffer.total_written("p"), total);
                assert_eq!(buffer.copy("p").unwrap_or_default(), short);
                assert_eq!(buffer.copy_full("p").unwrap_or_default(), full);
                for offset in [0, total / 2, total.saturating_sub(3), total, total + 1] {
                    let start = offset.max(total - full.len() as u64).min(total);
                    let local = (start - (total - full.len() as u64)) as usize;
                    assert_eq!(
                        buffer.read_since("p", offset),
                        (full[local..].to_vec(), total)
                    );
                    for cap in [0, 1, 3, 8, 30] {
                        let read = buffer.read_since_limited("p", offset, cap);
                        assert_eq!(read.start, start);
                        assert_eq!(read.total, total);
                        let next = local + read.bytes.len();
                        assert_eq!(
                            read.following_continuation,
                            full.get(next).is_some_and(|byte| byte & 0xc0 == 0x80)
                        );
                        assert_eq!(
                            read.bytes,
                            full[local..local + (full.len() - local).min(cap)]
                        );
                        assert!(read.bytes.len() <= cap);
                    }
                }
            }
            buffer.remove("p");
            assert!(buffer.copy("p").is_none());
            assert_eq!(buffer.read_since("p", 0), (Vec::new(), 0));
        }
    }
}

#[test]
fn bounded_read_crosses_physical_wrap_without_linearizing_storage() {
    let mut buffer = OutputBuffer::new_with_caps(8, 8);
    buffer.append("p", b"012345");
    buffer.append("p", b"6789");
    let read = buffer.read_since_limited("p", 6, 3);
    assert_eq!(read.bytes, b"678");
    assert_eq!((read.start, read.total), (6, 10));
    assert_eq!(buffer.full_buffers["p"].head, 2);
    assert_eq!(buffer.copy_full("p").unwrap(), b"23456789");
}
