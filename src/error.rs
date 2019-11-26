
use std::io::{Error as IoError, ErrorKind as IoErrorKind};

use tokio::timer::timeout::Error as TimeoutError;
use serde_json::{Error as JsonError};
use daemon_engine::{DaemonError};
use dsf_core::types::Error as DsfError;


#[derive(Debug)]
pub enum Error {
    Io(IoErrorKind),
    Json(JsonError),
    Daemon(DaemonError),
    Remote(DsfError),
    None(()),
    UnrecognizedResult,
    Unknown,
    Timeout,
    Socket,
}

impl From<IoError> for Error {
    fn from(e: IoError) -> Self {
        Error::Io(e.kind())
    }
}

impl From<JsonError> for Error {
    fn from(e: JsonError) -> Self {
        Error::Json(e)
    }
}

impl From<DaemonError> for Error {
    fn from(e: DaemonError) -> Self {
        Error::Daemon(e)
    }
}

impl From<DsfError> for Error {
    fn from(e: DsfError) -> Self {
        Error::Remote(e)
    }
}


impl From<TimeoutError<Error>> for Error {
    fn from(e: TimeoutError<Error>) -> Self {
        if let Some(e) = e.into_inner() {
            e
        } else {
            Error::Timeout
        }
    }
}

impl From<()> for Error {
    fn from(e: ()) -> Self {
        Error::None(e)
    }
}
