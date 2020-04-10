
use std::io::{Error as IoError, ErrorKind as IoErrorKind};

use async_std::future::TimeoutError;
use dsf_core::types::Error as DsfError;

use futures_codec::JsonCodecError as CodecError;

#[derive(Debug)]
pub enum Error {
    Io(IoErrorKind),
    Codec(CodecError),
    Remote(DsfError),
    None(()),
    UnrecognizedResult,
    NoServiceFound,
    Unknown,
    Timeout,
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
