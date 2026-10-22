use std::path::Path;

const MAX_SPEC_BYTES: usize = 256 * 1024;

pub(super) fn read_spec(path: &Path) -> Result<String, String> {
    if path == Path::new("-") {
        use std::io::Read;
        let mut input = String::new();
        std::io::stdin()
            .take((MAX_SPEC_BYTES + 1) as u64)
            .read_to_string(&mut input)
            .map_err(|error| format!("read pipeline specification: {error}"))?;
        if input.len() > MAX_SPEC_BYTES {
            return Err("pipeline specification exceeds 256 KiB".into());
        }
        return Ok(input);
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| format!("read Blueprint document: {error}"))?
        .take((MAX_SPEC_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_SPEC_BYTES {
        return Err("Blueprint document exceeds 256 KiB".into());
    }
    String::from_utf8(bytes).map_err(|_| "Blueprint document must be UTF-8".into())
}
