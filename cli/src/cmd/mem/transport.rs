pub(crate) fn mem_transport_error(_url: &str, err: &impl std::fmt::Display) -> String {
    err.to_string()
}
