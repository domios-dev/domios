use codeterm_types::native_host::{
    NativeHostFrame as Frame, MAX_NATIVE_HOST_FRAME_BYTES, NATIVE_HOST_REQUEST_DEADLINE,
};
use serde::{de::DeserializeOwned, Serialize};
use std::io;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::timeout,
};

pub(super) async fn read_frame<R, UpdateRequest, UpdateResult>(
    reader: &mut R,
) -> io::Result<Frame<UpdateRequest, UpdateResult>>
where
    R: tokio::io::AsyncRead + Unpin,
    UpdateRequest: DeserializeOwned,
    UpdateResult: DeserializeOwned,
{
    let count = reader.read_u32().await? as usize;
    if count == 0 || count > MAX_NATIVE_HOST_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid native host frame length",
        ));
    }
    let mut bytes = vec![0; count];
    reader.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

pub(super) async fn send_frame<W, UpdateRequest, UpdateResult>(
    writer: &mut W,
    frame: &Frame<UpdateRequest, UpdateResult>,
) -> io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
    UpdateRequest: Serialize,
    UpdateResult: Serialize,
{
    timeout(NATIVE_HOST_REQUEST_DEADLINE, write_frame(writer, frame))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Native host write deadline"))?
}

async fn write_frame<W, UpdateRequest, UpdateResult>(
    writer: &mut W,
    frame: &Frame<UpdateRequest, UpdateResult>,
) -> io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
    UpdateRequest: Serialize,
    UpdateResult: Serialize,
{
    let bytes = encode_frame(frame)?;
    write_encoded_frame(writer, &bytes).await
}

pub(super) fn encode_frame<UpdateRequest, UpdateResult>(
    frame: &Frame<UpdateRequest, UpdateResult>,
) -> io::Result<Vec<u8>>
where
    UpdateRequest: Serialize,
    UpdateResult: Serialize,
{
    let bytes = serde_json::to_vec(frame).map_err(io::Error::other)?;
    if bytes.is_empty() || bytes.len() > MAX_NATIVE_HOST_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Native host frame exceeds its bound",
        ));
    }
    Ok(bytes)
}

pub(super) async fn write_encoded_frame<W>(writer: &mut W, bytes: &[u8]) -> io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    writer.write_u32(bytes.len() as u32).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await
}
