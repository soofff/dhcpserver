use std::fmt::{Display, Formatter};
use std::error::Error;
use std::num::ParseIntError;
use reqwest::header::{InvalidHeaderValue, InvalidHeaderName};
use dhcplib::MacAddress;
use tokio::task::JoinError;
use log::SetLoggerError;

pub type DhcpResult<T> = Result<T, DhcpError>;

#[derive(Debug)]
pub enum DhcpError {
    IoError(std::io::Error),
    SerdeYamlError(serde_yaml::Error),
    SerdeJsonError(serde_json::Error),
    SourceKindUnknown,
    SerdeErrorString(String),
    TeraError(tera::Error),
    ParseIntError(ParseIntError),
    ReqwestError(reqwest::Error),
    UrlError(url::ParseError),
    InvalidHeaderName(InvalidHeaderName),
    InvalidHeaderValue(InvalidHeaderValue),
    ClientIpAddressMissing(MacAddress),
    DhcpLibError(dhcplib::error::DhcpError),
    CustomRestTypeError,
    JoinError(JoinError),
    ConfigFileNotFound,
    SetLoggerError(SetLoggerError)
}

impl Display for DhcpError {
    fn fmt(&self, f: &mut Formatter) -> core::fmt::Result {
        let s = match self {
            DhcpError::IoError(e) => e.to_string(),
            DhcpError::SerdeYamlError(e) => e.to_string(),
            DhcpError::SourceKindUnknown => "Source unknown".to_string(),
            DhcpError::SerdeErrorString(e) => e.to_string(),
            DhcpError::TeraError(e) => e.to_string(),
            DhcpError::ParseIntError(e) => e.to_string(),
            DhcpError::ReqwestError(e) => e.to_string(),
            DhcpError::UrlError(e) => e.to_string(),
            DhcpError::InvalidHeaderName(e) => e.to_string(),
            DhcpError::InvalidHeaderValue(e) => e.to_string(),
            DhcpError::ClientIpAddressMissing(e) => e.to_string(),
            DhcpError::DhcpLibError(e) => e.to_string(),
            DhcpError::CustomRestTypeError => "Custom option type error".to_string(),
            DhcpError::SerdeJsonError(e) => e.to_string(),
            DhcpError::JoinError(e) => e.to_string(),
            DhcpError::ConfigFileNotFound => "no config file found".to_string(),
            DhcpError::SetLoggerError(e) => e.to_string(),
        };

        write!(f, "{}", s)
    }
}

impl Error for DhcpError { fn source(&self) -> Option<&(dyn Error + 'static)> { None } }

impl From<std::io::Error> for DhcpError {
    fn from(e: std::io::Error) -> Self {
        Self::IoError(e)
    }
}

impl From<serde_yaml::Error> for DhcpError {
    fn from(e: serde_yaml::Error) -> Self {
        Self::SerdeYamlError(e)
    }
}

impl From<serde_json::Error> for DhcpError {
    fn from(e: serde_json::Error) -> Self {
        Self::SerdeJsonError(e)
    }
}

impl From<tera::Error> for DhcpError {
    fn from(e: tera::Error) -> Self {
        Self::TeraError(e)
    }
}
impl From<ParseIntError> for DhcpError {
    fn from(e: ParseIntError) -> Self {
        Self::ParseIntError(e)
    }
}

impl From<reqwest::Error> for DhcpError {
    fn from(e: reqwest::Error) -> Self {
        Self::ReqwestError(e)
    }
}

impl From<url::ParseError> for DhcpError {
    fn from(e: url::ParseError) -> Self {
        Self::UrlError(e)
    }
}

impl From<InvalidHeaderValue> for DhcpError {
    fn from(e: InvalidHeaderValue) -> Self {
        Self::InvalidHeaderValue(e)
    }
}

impl From<InvalidHeaderName> for DhcpError {
    fn from(e: InvalidHeaderName) -> Self {
        Self::InvalidHeaderName(e)
    }
}

impl From<dhcplib::error::DhcpError> for DhcpError {
    fn from(e: dhcplib::error::DhcpError) -> Self {
        Self::DhcpLibError(e)
    }
}

impl From<JoinError> for DhcpError {
    fn from(e: JoinError) -> Self {
        Self::JoinError(e)
    }
}

impl From<SetLoggerError> for DhcpError {
    fn from(e: SetLoggerError) -> Self {
        Self::SetLoggerError(e)
    }
}
