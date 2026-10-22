use core_foundation::{
    base::TCFType,
    string::{CFString, CFStringRef},
    url::{CFURLCreateWithString, CFURLRef, CFURL},
};

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn LSCopyDefaultApplicationURLForURL(
        url: CFURLRef,
        roles: u32,
        error: *mut *const std::ffi::c_void,
    ) -> CFURLRef;
    fn LSRegisterURL(url: CFURLRef, update: u8) -> i32;
    fn LSSetDefaultHandlerForURLScheme(scheme: CFStringRef, bundle: CFStringRef) -> i32;
}

pub fn repair_registration() -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let bundle = exe
        .ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .ok_or("Production deep links require an app bundle")?;
    let bundle_url = CFURL::from_path(bundle, true).ok_or("Invalid app bundle path")?;
    let callback_string = CFString::new("domios://auth/callback");
    let callback = unsafe {
        CFURLCreateWithString(
            std::ptr::null(),
            callback_string.as_concrete_TypeRef(),
            std::ptr::null(),
        )
    };
    if callback.is_null() {
        return Err("Invalid scheme URL".into());
    }
    let callback = unsafe { CFURL::wrap_under_create_rule(callback) };
    let current = unsafe {
        LSCopyDefaultApplicationURLForURL(
            callback.as_concrete_TypeRef(),
            u32::MAX,
            std::ptr::null_mut(),
        )
    };
    if !current.is_null() {
        let current = unsafe { CFURL::wrap_under_create_rule(current) };
        if current.to_path().as_deref() == Some(bundle) {
            return Ok(());
        }
    }
    let status = unsafe { LSRegisterURL(bundle_url.as_concrete_TypeRef(), 1) };
    if status != 0 {
        return Err(format!("Launch Services registration failed ({status})").into());
    }
    let scheme = CFString::new("domios");
    let identifier = CFString::new("com.domios.app");
    let status = unsafe {
        LSSetDefaultHandlerForURLScheme(
            scheme.as_concrete_TypeRef(),
            identifier.as_concrete_TypeRef(),
        )
    };
    if status != 0 {
        return Err(format!("Launch Services default handler repair failed ({status})").into());
    }
    Ok(())
}
