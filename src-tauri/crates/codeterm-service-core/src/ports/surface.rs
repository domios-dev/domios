pub use codeterm_surface::host::SurfaceHost;

#[cfg(test)]
mod tests {
    #[test]
    fn surface_host_is_object_safe() {
        super::super::assert_port::<dyn super::SurfaceHost>();
    }
}
