use std::{
    pin::Pin,
    task::{ready, Context, Poll}, io, collections::LinkedList,
};

use futures_core::Stream;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf, split, AsyncWriteExt},
    sync::mpsc::{Receiver, Sender},
};
use webparse::{BinaryMut, Buf, BufMut};

use crate::{ProxyResult, Helper};

use super::{ProtFrame};

pub struct TransStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    stream: T,
    id: u32,
    read: BinaryMut,
    write: BinaryMut,
    in_sender: Sender<ProtFrame>,
    out_receiver: Receiver<ProtFrame>,
}

impl<T> TransStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    pub fn new(
        stream: T,
        id: u32,
        in_sender: Sender<ProtFrame>,
        out_receiver: Receiver<ProtFrame>,
    ) -> Self {
        Self {
            stream,
            id,
            read: BinaryMut::new(),
            write: BinaryMut::new(),
            in_sender,
            out_receiver,
        }
    }

    pub fn reader_mut(&mut self) -> &mut BinaryMut {
        &mut self.read
    }

    
    pub fn write_mut(&mut self) -> &mut BinaryMut {
        &mut self.read
    }

    pub async fn inner_copy_wait(mut self) -> Result<(), std::io::Error> {
        println!("copy wait!!!!");
        let mut buf = vec![0u8; 2048];
        let mut link = LinkedList::<ProtFrame>::new();
        let (mut reader, mut writer) = split(self.stream);
        loop {
            println!("rad!!!!!!!!");
            if self.read.has_remaining() {
                link.push_back(ProtFrame::new_data(self.id, self.read.copy_to_binary()));
                self.read.clear();
            }

            tokio::select! {
                n = reader.read(&mut buf) => {
                    println!("read = {:?}", n);
                    let n = n?;
                    if n == 0 {
                        return Ok(())
                    } else {
                        println!("read content xxxx = {:?}", String::from_utf8_lossy(&buf[..n]));
                        self.read.put_slice(&buf[..n]);
                    }
                },
                r = writer.write(self.write.chunk()), if self.write.has_remaining() => {
                    println!("write = {:?}", r);
                    println!("write content = {:?}", String::from_utf8_lossy(self.write.chunk()));
                    match r {
                        Ok(n) => {
                            self.write.advance(n);
                            println!("write remain len = {:?}", self.write.remaining());
                            if !self.write.has_remaining() {
                                self.write.clear();
                            }
                        }
                        Err(_) => todo!(),
                    }
                }
                r = self.out_receiver.recv() => {
                    println!("recv = {:?}", r);
                    if let Some(v) = r {
                        if v.is_close() || v.is_create() {
                            return Ok(())
                        } else if v.is_data() {
                            match v {
                                ProtFrame::Data(d) => {
                                    self.write.put_slice(&d.data().chunk());
                                }
                                _ => unreachable!(),
                            }
                        }
                    } else {
                        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid frame"))
                    }
                }
                p = self.in_sender.reserve(), if link.len() > 0 => {
                    println!("send = {:?}", p);
                    match p {
                        Err(_)=>{
                            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid frame"))
                        }
                        Ok(p) => {
                            p.send(link.pop_front().unwrap())
                        }, 
                    }
                }
            }
            // while let Some(v) = Helper::decode_frame(&mut self.read).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid frame"))? {
            //     link.push_back(v);
            // }

            println!("rad!!!!!!!! 2222222");
            // let x = self.next().await;
            // self.next()
            // poll_fn(|cx| {
            //     Poll::Pending
            // });
            // self.stream.po
            // self.stream.ready(Interest::READABLE | Interest::WRITABLE).await;
        }
    }

    pub async fn copy_wait(self) -> Result<(), std::io::Error> {
        let sender = self.in_sender.clone();
        let id = self.id;
        let ret = self.inner_copy_wait().await;
        let _ = sender.send(ProtFrame::new_close(id)).await;
        ret
    }

    pub fn stream_read(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<usize>> {
        self.read.reserve(1);
        let n = {
            let mut buf = ReadBuf::uninit(self.read.chunk_mut());
            let ptr = buf.filled().as_ptr();
            ready!(Pin::new(&mut self.stream).poll_read(cx, &mut buf)?);
            assert_eq!(ptr, buf.filled().as_ptr());
            buf.filled().len()
        };

        unsafe {
            self.read.advance_mut(n);
        }
        Poll::Ready(Ok(n))
    }

    pub fn poll_read_all(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<usize>> {
        let mut size = 0;
        loop {
            match self.stream_read(cx)? {
                Poll::Ready(0) => return Poll::Ready(Ok(0)),
                Poll::Ready(n) => size += n,
                Poll::Pending => {
                    if size == 0 {
                        return Poll::Pending;
                    } else {
                        break;
                    }
                }
            }
        }
        Poll::Ready(Ok(size))
    }

}

impl<T> AsyncRead for TransStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if !self.read.has_remaining() {
            ready!(self.stream_read(cx))?;
        }
        if self.read.has_remaining() {
            let copy = std::cmp::min(self.read.remaining(), buf.remaining());
            buf.put_slice(&self.read.chunk()[..copy]);
            self.read.advance(copy);
            return Poll::Ready(Ok(()));
        }
        return Poll::Ready(Ok(()));
    }
}

impl<T> AsyncWrite for TransStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

impl<T> Stream for TransStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    type Item = ProxyResult<ProtFrame>;
    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if let Some(v) = Helper::decode_frame(&mut self.read)? {
            return Poll::Ready(Some(Ok(v)));
        }
        match ready!(self.poll_read_all(cx)?) {
            0 => {
                println!("test:::: recv client end!!!");
                return Poll::Ready(None);
            }
            _ => {
                if let Some(v) = Helper::decode_frame(&mut self.read)? {
                    return Poll::Ready(Some(Ok(v)));
                } else {
                    return Poll::Pending;
                }
            }
        }
    }
}