use std::io::{Error as IoError, ErrorKind as IoErrorKind};

use async_std::future::TimeoutError;
use dsf_core::error::Error as DsfError;

use futures_codec::JsonCodecError as CodecError;

#[derive(Debug)]
#[cfg_attr(feature="thiserror", derive(thiserror::Error))]
pub enum Error {
    #[cfg_attr(feature="thiserror", error("io: {0:?}"))]
    Io(IoErrorKind),
    #[cfg_attr(feature="thiserror", error("codec: {0:?}"))]
    Codec(CodecError),
    #[cfg_attr(feature="thiserror", error("remote: {0}"))]
    Remote(DsfError),
    #[cfg_attr(feature="thiserror", error("no error?!"))]
    None(()),
    #[cfg_attr(feature="thiserror", error("unrecognised result"))]
    UnrecognizedResult,
    #[cfg_attr(feature="thiserror", error("no matching service found"))]
    NoServiceFound,
    #[cfg_attr(feature="thiserror", error("no matching page found"))]
    NoPageFound,
    #[cfg_attr(feature="thiserror", error("unknown error"))]
    Unknown,
    #[cfg_attr(feature="thiserror", error("timeout"))]
    Timeout,
    #[cfg_attr(feature="thiserror", error("socket error"))]
    Socket,
}

impl From<IoError> for Error {
    fn from(e: IoError) -> Self {
        Error::Io(e.kind())
    }
}

impl From<CodecError> for Error {
    fn from(e: CodecError) -> Self {
        Error::Codec(e)
    }
}

impl From<DsfError> for Error {
    fn from(e: DsfError) -> Self {
        Error::Remote(e)
    }
}

impl From<TimeoutError> for Error {
    fn from(_e: TimeoutError) -> Self {
        Error::Timeout
    }
}

impl From<()> for Error {
    fn from(e: ()) -> Self {
        Error::None(e)
    }
}
