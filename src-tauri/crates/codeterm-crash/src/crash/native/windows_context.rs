use std::ffi::c_void;
use std::mem::{size_of, MaybeUninit};

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn ReadProcessMemory(
        process: *mut c_void,
        address: *const c_void,
        buffer: *mut c_void,
        size: usize,
        read: *mut usize,
    ) -> i32;
    fn GetModuleHandleExW(flags: u32, address: *const u16, module: *mut *mut c_void) -> i32;
    fn GetModuleFileNameW(module: *mut c_void, name: *mut u16, size: u32) -> u32;
    fn FreeLibrary(module: *mut c_void) -> i32;
}

unsafe fn read_native<T>(address: *const T) -> Option<T> {
    if address.is_null() {
        return None;
    }
    let mut value = MaybeUninit::<T>::uninit();
    let mut read = 0;
    // SAFETY: the OS checks the source; the destination has exactly size_of::<T>() writable bytes.
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address.cast(),
            value.as_mut_ptr().cast(),
            size_of::<T>(),
            &mut read,
        )
    };
    if ok == 0 || read != size_of::<T>() {
        return None;
    }
    // SAFETY: the caller requires T to accept every bit pattern, and all bytes were initialized.
    Some(unsafe { value.assume_init() })
}

pub(super) fn exception_address(ctx: &crash_handler::CrashContext) -> Option<usize> {
    if ctx.process_id != std::process::id() {
        return None;
    }
    // SAFETY: both Windows records contain only integers and raw pointers.
    let pointers = unsafe { read_native(ctx.exception_pointers) }?;
    // SAFETY: the OS checks the pointer; this record accepts every bit pattern.
    let record = unsafe { read_native(pointers.ExceptionRecord) }?;
    (!record.ExceptionAddress.is_null()).then_some(record.ExceptionAddress as usize)
}

pub(super) fn module_at(address: usize) -> Option<(String, usize)> {
    if address == 0 {
        return None;
    }
    let mut module = std::ptr::null_mut();
    // SAFETY: FROM_ADDRESS interprets the value as an address, never as a string.
    let ok = unsafe { GetModuleHandleExW(4, address as *const u16, &mut module) };
    if ok == 0 || module.is_null() {
        return None;
    }
    let mut name = [0u16; 1024];
    // SAFETY: the acquired module reference stays live through this bounded lookup.
    let length = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32) };
    let offset = address.checked_sub(module as usize);
    // SAFETY: release exactly the reference acquired by GetModuleHandleExW.
    unsafe { FreeLibrary(module) };
    if length == 0 || length as usize >= name.len() {
        return None;
    }
    Some((String::from_utf16_lossy(&name[..length as usize]), offset?))
}
