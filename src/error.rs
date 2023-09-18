use std::io;

use tokio::net::TcpStream;
use webparse::{WebError, BinaryMut};

#[derive(Debug)]
pub enum ProxyError {
    IoError(io::Error),
    WebError(WebError),
    Continue((Option<BinaryMut>, TcpStream)),
    VerifyFail,
    UnknowHost,
    SizeNotMatch,
    ProtErr,
    ProtNoSupport,
    Extension(&'static str)
}

impl ProxyError {
    pub fn extension(value: &'static str) -> ProxyError {
        ProxyError::Extension(value)
    }

    pub fn is_weberror(&self) -> bool {
        match self {
            ProxyError::WebError(_) => true,
            _ => false,
        }
    }
}

pub type ProxyResult<T> = Result<T, ProxyError>;

impl From<io::Error> for ProxyError {
    fn from(value: io::Error) -> Self {
        ProxyError::IoError(value)
    }
}

impl From<WebError> for ProxyError {
    fn from(value: WebError) -> Self {
        ProxyError::WebError(value)
    }
}
