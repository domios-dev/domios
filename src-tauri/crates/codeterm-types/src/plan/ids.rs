//! Short, collision-checked entity id minting (typed base36 slugs).

const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn base36(mut n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    buf.reverse();
    String::from_utf8(buf).expect("base36 digits are ascii")
}

fn slug(len: usize) -> String {
    let bytes = uuid::Uuid::new_v4().into_bytes();
    let n = u64::from_le_bytes(bytes[..8].try_into().expect("uuid has 16 bytes"));
    let modulus = 36u64.pow(len as u32);
    format!("{:0>width$}", base36(n % modulus), width = len)
}

/// Short id with a fresh salt and no store check (6 base36 chars). For standalone constructors.
pub fn short_id(prefix: &str) -> String {
    format!("{prefix}_{}", slug(6))
}

/// Collision-checked short id. Grows 6 -> 7 -> 8 chars, regenerating the salt on every collision.
pub fn mint_id(prefix: &str, taken: impl Fn(&str) -> bool) -> String {
    match try_mint_id(prefix, |candidate| {
        Ok::<_, std::convert::Infallible>(taken(candidate))
    }) {
        Ok(id) => id,
        Err(never) => match never {},
    }
}

pub fn try_mint_id<E>(
    prefix: &str,
    mut taken: impl FnMut(&str) -> Result<bool, E>,
) -> Result<String, E> {
    for len in [6usize, 7, 8] {
        for _ in 0..16 {
            let candidate = format!("{prefix}_{}", slug(len));
            if !taken(&candidate)? {
                return Ok(candidate);
            }
        }
    }
    Ok(format!("{prefix}_{}", uuid::Uuid::new_v4().simple()))
}

#[cfg(test)]
mod tests;
