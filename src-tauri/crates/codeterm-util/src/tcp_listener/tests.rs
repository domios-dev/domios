use super::*;

static LISTENERS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(windows)]
#[test]
fn inheritable_listener_reproduces_zombie_port_until_child_exits() {
    let _serial = LISTENERS.lock().unwrap();
    let socket = socket2::Socket::new_raw(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .unwrap();
    socket
        .bind(&"127.0.0.1:0".parse::<SocketAddr>().unwrap().into())
        .unwrap();
    socket.listen(1024).unwrap();
    let listener: TcpListener = socket.into();
    let address = listener.local_addr().unwrap();
    let child = super::test_support::InheritingChild::start();
    drop(listener);
    assert_eq!(bind(address).unwrap_err().kind(), io::ErrorKind::AddrInUse);
    assert!(child.is_alive());
    drop(child);
    assert_eq!(bind(address).unwrap().local_addr().unwrap(), address);
}

#[test]
fn listener_bind_owns_both_address_families() {
    let _serial = LISTENERS.lock().unwrap();
    for address in ["127.0.0.1:0", "[::1]:0"] {
        let listener = bind(address.parse().unwrap()).unwrap();
        let address = listener.local_addr().unwrap();
        assert!(bind(address).is_err());
        drop(listener);
        assert_eq!(bind(address).unwrap().local_addr().unwrap(), address);
    }
}

#[cfg(windows)]
#[test]
fn listener_handles_cannot_be_inherited_by_a_live_child() {
    let _serial = LISTENERS.lock().unwrap();
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Foundation::{GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT};
    let listeners: Vec<_> = ["127.0.0.1:0", "[::1]:0"]
        .into_iter()
        .map(|address| bind(address.parse().unwrap()).unwrap())
        .collect();
    for listener in &listeners {
        let mut flags = 0;
        assert_ne!(
            unsafe { GetHandleInformation(listener.as_raw_socket() as HANDLE, &mut flags) },
            0
        );
        assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
    }
    let child = super::test_support::InheritingChild::start();
    for listener in listeners {
        let address = listener.local_addr().unwrap();
        drop(listener);
        let rebound = bind(address).expect("live children must not retain the parent's listener");
        assert!(child.is_alive());
        drop(rebound);
    }
}
