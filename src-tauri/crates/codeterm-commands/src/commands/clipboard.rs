use codeterm_service_core::attachments::CHAT_UPLOAD_CACHE_BYTES;

pub fn save_clipboard_image_core(bytes: Vec<u8>, ext: String) -> Result<String, String> {
    let path = codeterm_service_core::attachments::save_with_uuid(
        "images",
        &ext,
        &bytes,
        codeterm_service_core::attachments::EvictionPolicy::MaxCount(20),
    )?;
    Ok(path.to_string_lossy().to_string())
}

pub fn save_clipboard_file_core(bytes: Vec<u8>, filename: String) -> Result<String, String> {
    let path = codeterm_service_core::attachments::save_with_prefixed_name(
        "files",
        &filename,
        &bytes,
        // A single chat upload may reach 150 MB, so the shared 100 MB cap would evict it at once.
        codeterm_service_core::attachments::EvictionPolicy::MaxBytes(CHAT_UPLOAD_CACHE_BYTES),
    )?;
    Ok(path.to_string_lossy().to_string())
}
