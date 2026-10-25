#[cfg(windows)]
mod platform {
    use std::{ffi::c_void, io};
    use tokio::sync::watch;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        CancelMibChangeNotify2, NotifyUnicastIpAddressChange, MIB_NOTIFICATION_TYPE,
        MIB_UNICASTIPADDRESS_ROW,
    };
    use windows_sys::Win32::Networking::WinSock::AF_UNSPEC;

    pub struct Notifications {
        handle: usize,
        _sender: Box<watch::Sender<()>>,
        receiver: watch::Receiver<()>,
    }

    unsafe extern "system" fn changed(
        context: *const c_void,
        _row: *const MIB_UNICASTIPADDRESS_ROW,
        _kind: MIB_NOTIFICATION_TYPE,
    ) {
        // The boxed sender lives until cancellation has joined all native callbacks.
        let sender = unsafe { &*context.cast::<watch::Sender<()>>() };
        sender.send_replace(());
    }

    pub fn subscribe() -> io::Result<Notifications> {
        let (sender, receiver) = watch::channel(());
        let sender = Box::new(sender);
        let mut handle = std::ptr::null_mut();
        let error = unsafe {
            NotifyUnicastIpAddressChange(
                AF_UNSPEC,
                Some(changed),
                (&*sender as *const watch::Sender<()>).cast(),
                false,
                &mut handle,
            )
        };
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        Ok(Notifications {
            handle: handle as usize,
            _sender: sender,
            receiver,
        })
    }

    impl Notifications {
        pub async fn changed(&mut self) -> io::Result<()> {
            self.receiver.changed().await.map_err(io::Error::other)
        }
    }

    impl Drop for Notifications {
        fn drop(&mut self) {
            unsafe {
                CancelMibChangeNotify2(self.handle as _);
            }
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use tokio::io::unix::AsyncFd;

    pub struct Notifications(AsyncFd<OwnedFd>);

    pub fn subscribe() -> io::Result<Notifications> {
        #[cfg(target_os = "linux")]
        let raw = unsafe { libc::socket(libc::AF_NETLINK, libc::SOCK_RAW, libc::NETLINK_ROUTE) };
        #[cfg(target_os = "macos")]
        let raw = unsafe { libc::socket(libc::PF_ROUTE, libc::SOCK_RAW, 0) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        if unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
        #[cfg(target_os = "linux")]
        {
            let mut address: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
            address.nl_family = libc::AF_NETLINK as _;
            address.nl_groups =
                (libc::RTMGRP_IPV4_IFADDR | libc::RTMGRP_IPV6_IFADDR | libc::RTMGRP_LINK) as _;
            if unsafe {
                libc::bind(
                    raw,
                    (&address as *const libc::sockaddr_nl).cast(),
                    std::mem::size_of_val(&address) as _,
                )
            } < 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        let flags = unsafe { libc::fcntl(raw, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Notifications(AsyncFd::new(fd)?))
    }

    impl Notifications {
        pub async fn changed(&mut self) -> io::Result<()> {
            loop {
                let mut ready = self.0.readable().await?;
                match ready.try_io(|fd| {
                    let mut buffer = [0u8; 8192];
                    let count = unsafe {
                        libc::recv(
                            fd.get_ref().as_raw_fd(),
                            buffer.as_mut_ptr().cast(),
                            buffer.len(),
                            0,
                        )
                    };
                    if count < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                }) {
                    Ok(result) => return result,
                    Err(_) => continue,
                }
            }
        }
    }
}

pub use platform::{subscribe, Notifications};
