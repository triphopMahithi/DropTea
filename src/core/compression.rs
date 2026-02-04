use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncWrite, AsyncRead, BufReader, ReadBuf};
use async_compression::tokio::write::{GzipEncoder, ZstdEncoder, ZlibEncoder};
use async_compression::tokio::bufread::{GzipDecoder, ZstdDecoder, ZlibDecoder};
use async_compression::Level;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompressionAlgo {
    Zstd,
    Gzip,
    Zlib,
    None, // 🔥 โหมดส่งสด
}

impl CompressionAlgo {
    pub fn as_str(&self) -> &'static str {
        match self {
            CompressionAlgo::Zstd => "zstd",
            CompressionAlgo::Gzip => "gzip",
            CompressionAlgo::Zlib => "zlib",
            CompressionAlgo::None => "none",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "zstd" => Some(CompressionAlgo::Zstd),
            "gzip" => Some(CompressionAlgo::Gzip),
            "zlib" => Some(CompressionAlgo::Zlib),
            "none" => Some(CompressionAlgo::None),
            _ => None,
        }
    }
}

// Wrapper Writer
pub enum Compressor<W: AsyncWrite + Unpin> {
    Zstd(ZstdEncoder<W>),
    Gzip(GzipEncoder<W>),
    Zlib(ZlibEncoder<W>),
    None(W), // Passthrough
}

impl<W: AsyncWrite + Unpin> Compressor<W> {
    pub fn new(writer: W, algo: CompressionAlgo) -> Self {
        match algo {
            CompressionAlgo::Zstd => Compressor::Zstd(ZstdEncoder::with_quality(writer, Level::Fastest)),
            CompressionAlgo::Gzip => Compressor::Gzip(GzipEncoder::new(writer)),
            CompressionAlgo::Zlib => Compressor::Zlib(ZlibEncoder::new(writer)),
            CompressionAlgo::None => Compressor::None(writer),
        }
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for Compressor<W> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        match &mut *self {
            Compressor::Zstd(inner) => Pin::new(inner).poll_write(cx, buf),
            Compressor::Gzip(inner) => Pin::new(inner).poll_write(cx, buf),
            Compressor::Zlib(inner) => Pin::new(inner).poll_write(cx, buf),
            Compressor::None(inner) => Pin::new(inner).poll_write(cx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut *self {
            Compressor::Zstd(inner) => Pin::new(inner).poll_flush(cx),
            Compressor::Gzip(inner) => Pin::new(inner).poll_flush(cx),
            Compressor::Zlib(inner) => Pin::new(inner).poll_flush(cx),
            Compressor::None(inner) => Pin::new(inner).poll_flush(cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut *self {
            Compressor::Zstd(inner) => Pin::new(inner).poll_shutdown(cx),
            Compressor::Gzip(inner) => Pin::new(inner).poll_shutdown(cx),
            Compressor::Zlib(inner) => Pin::new(inner).poll_shutdown(cx),
            Compressor::None(inner) => Pin::new(inner).poll_shutdown(cx),
        }
    }
}

// Wrapper Reader
pub enum Decompressor<R: AsyncRead + Unpin> {
    Zstd(ZstdDecoder<BufReader<R>>),
    Gzip(GzipDecoder<BufReader<R>>),
    Zlib(ZlibDecoder<BufReader<R>>),
    None(BufReader<R>),
}

impl<R: AsyncRead + Unpin> Decompressor<R> {
    pub fn new(reader: R, algo: CompressionAlgo) -> Self {
        let buf_reader = BufReader::new(reader);
        match algo {
            CompressionAlgo::Zstd => Decompressor::Zstd(ZstdDecoder::new(buf_reader)),
            CompressionAlgo::Gzip => Decompressor::Gzip(GzipDecoder::new(buf_reader)),
            CompressionAlgo::Zlib => Decompressor::Zlib(ZlibDecoder::new(buf_reader)),
            CompressionAlgo::None => Decompressor::None(buf_reader),
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for Decompressor<R> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match &mut *self {
            Decompressor::Zstd(inner) => Pin::new(inner).poll_read(cx, buf),
            Decompressor::Gzip(inner) => Pin::new(inner).poll_read(cx, buf),
            Decompressor::Zlib(inner) => Pin::new(inner).poll_read(cx, buf),
            Decompressor::None(inner) => Pin::new(inner).poll_read(cx, buf),
        }
    }
}