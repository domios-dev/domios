const MAX_HEADERS: usize = 16 * 1024;
const MAX_BODY: usize = 1024 * 1024;
const MAX_RESPONSE: usize = 2 * MAX_BODY;
const MAX_CHUNKS: usize = 1024;

use base64::Engine;

#[cfg(target_os = "macos")]
pub(super) fn macos_localapi(path: &str) -> Option<serde_json::Value> {
    let home = dirs::home_dir()?;
    let out = codeterm_util::process_util::disclaimed_capture(
        "/bin/sh",
        &[
            "-c",
            concat!(
                include_str!("macos_localapi_model.sh"),
                "\n",
                include_str!("macos_localapi.sh")
            ),
            "domios-tailscale-probe",
            home.to_str()?,
            path,
        ],
    )?;
    decode_probe_output(&out)
}

fn decode_probe_output(encoded: &str) -> Option<serde_json::Value> {
    if encoded.len() > 3 * MAX_RESPONSE {
        return None;
    }
    let encoded = encoded.split_ascii_whitespace().collect::<String>();
    let raw = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    serde_json::from_slice(&decode_response(&raw)?).ok()
}

fn decode_response(raw: &[u8]) -> Option<Vec<u8>> {
    if raw.len() > MAX_RESPONSE {
        return None;
    }
    let boundary = raw.windows(4).position(|bytes| bytes == b"\r\n\r\n")?;
    if boundary > MAX_HEADERS {
        return None;
    }
    let headers = std::str::from_utf8(raw.get(..boundary)?).ok()?;
    let mut lines = headers.split("\r\n");
    let mut status = lines.next()?.split_whitespace();
    if !matches!(status.next()?, "HTTP/1.0" | "HTTP/1.1") || status.next()? != "200" {
        return None;
    }
    let mut length = None;
    let mut chunked = false;
    for line in lines {
        let (name, value) = line.split_once(':')?;
        if name.is_empty() || name.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return None;
        }
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() || value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            length = Some(value.parse::<usize>().ok()?);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            if chunked || !value.eq_ignore_ascii_case("chunked") {
                return None;
            }
            chunked = true;
        }
    }
    let body = raw.get(boundary.checked_add(4)?..)?;
    if chunked {
        if length.is_some() {
            return None;
        }
        decode_chunks(body)
    } else if body.len() <= MAX_BODY && length.is_none_or(|length| body.len() == length) {
        Some(body.to_vec())
    } else {
        None
    }
}

fn take_line<'a>(remaining: &mut &'a [u8]) -> Option<&'a [u8]> {
    let end = remaining.windows(2).position(|bytes| bytes == b"\r\n")?;
    if end > MAX_HEADERS {
        return None;
    }
    let line = remaining.get(..end)?;
    *remaining = remaining.get(end.checked_add(2)?..)?;
    Some(line)
}

fn decode_chunks(mut remaining: &[u8]) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    for _ in 0..MAX_CHUNKS {
        let line = std::str::from_utf8(take_line(&mut remaining)?).ok()?;
        let size = line.split(';').next()?;
        if size.is_empty() || !size.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let size = usize::from_str_radix(size, 16).ok()?;
        if size == 0 {
            let trailer_start = remaining.len();
            loop {
                let trailer = take_line(&mut remaining)?;
                if trailer_start - remaining.len() > MAX_HEADERS {
                    return None;
                }
                if trailer.is_empty() {
                    return remaining.is_empty().then_some(body);
                }
                if !trailer.contains(&b':') {
                    return None;
                }
            }
        }
        if body.len().checked_add(size)? > MAX_BODY {
            return None;
        }
        body.extend_from_slice(remaining.get(..size)?);
        remaining = remaining.get(size..)?;
        if !remaining.starts_with(b"\r\n") {
            return None;
        }
        remaining = remaining.get(2..)?;
    }
    None
}

#[cfg(test)]
mod tests;
