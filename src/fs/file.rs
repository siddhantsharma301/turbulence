use std::ffi::OsString;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use crate::fs::error;
use crate::fs::open_options::OpenOptions;
use crate::world::World;

pub struct File {
    pub buffer: Vec<u8>,
    cursor: usize,
    path: OsString,
    open_options: OpenOptions,
}

impl File {
    pub(crate) fn new(
        buffer: Vec<u8>,
        cursor: usize,
        path: OsString,
        open_options: OpenOptions,
    ) -> Self {
        Self {
            buffer,
            cursor,
            path,
            open_options,
        }
    }

    pub async fn create(path: OsString) -> io::Result<Self> {
        OpenOptions::new()
            .create(true)
            .truncate(true)
            .open(path)
            .await
    }

    pub async fn open(path: OsString) -> io::Result<Self> {
        OpenOptions::new().open(path).await
    }

    pub fn options() -> OpenOptions {
        OpenOptions::new()
    }
}

impl AsyncRead for File {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        dst: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.cursor == self.buffer.len() {
            return Poll::Ready(Ok(()));
        }
        let file = self.get_mut();
        dst.put_slice(&file.buffer);
        file.cursor = file.buffer.len();
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for File {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        if !self.open_options.write && !self.open_options.append {
            return Poll::Ready(Err(error::read_only_fs()));
        }
        let file = self.get_mut();
        for data in buf {
            file.buffer.push(*data);
        }
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let _ = World::current(|world| {
            world
                .current_host_mut()
                .file_system
                .update(self.path.clone(), self.buffer.clone())
        });
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        self.poll_flush(cx)
    }

    fn is_write_vectored(&self) -> bool {
        true
    }
}
