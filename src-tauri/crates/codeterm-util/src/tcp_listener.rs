use std::{
    io,
    net::{SocketAddr, TcpListener},
};

pub fn bind(address: SocketAddr) -> io::Result<TcpListener> {
    // socket2 creates non-inheritable Windows handles atomically, before any child can launch.
    let socket = socket2::Socket::new(
        socket2::Domain::for_address(address),
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    #[cfg(unix)]
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&address.into())?;
    socket.listen(1024)?;
    Ok(socket.into())
}

#[cfg(test)]
mod tests;

#[cfg(all(windows, any(test, feature = "test-support")))]
pub mod test_support;
