use std::path::PathBuf;

use colored::Colorize;
use thiserror::Error;

#[derive(Error, Debug)]
#[error("{}")]
pub enum Error {
    #[error("{2} failed on \"{0}\": {1}")]
    FileIo(PathBuf, std::io::Error, &'static str),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{1}: \n{0}")]
    Json(String, serde_json::Error),
    #[error("{1}: \n{0}")]
    Toml(String, toml::de::Error),
    #[error("failed to parse \"{0}\": {1}")]
    TomlFile(PathBuf, toml::de::Error),
    #[error("{0}: {1}\nresponse body:\n{2}")]
    Response(i32, String, String),
    #[error("{0}")]
    MinReq(minreq::Error),
    #[error("no {0} in the pack root, which --json needs for pack metadata")]
    MissingPackToml(String),
    #[error("{0}")]
    TextParser(#[from] crate::parser::text::ParseError),
    #[error("{0}")]
    Format(#[from] crate::format::FormatError),
    #[error("{0}")]
    Other(String),
    #[error(
        "{0} is not set -- export it, put it in .env, or add it under [secrets] in a .sculk file"
    )]
    MissingEnv(&'static str),
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self::Other(message.to_owned())
    }
}

impl From<minreq::Response> for Error {
    fn from(req: minreq::Response) -> Self {
        let message = format!("{}: ({})", req.reason_phrase, req.url.bright_cyan());
        let body = crate::request::describe_body(&req);
        Self::Response(req.status_code.into(), message, body)
    }
}

impl From<minreq::Error> for Error {
    fn from(err: minreq::Error) -> Self {
        match err {
            minreq::Error::SerdeJsonError(err) => err.into(),
            err => Self::MinReq(err),
        }
    }
}

impl From<(&str, minreq::Error)> for Error {
    fn from((res, err): (&str, minreq::Error)) -> Self {
        match err {
            minreq::Error::SerdeJsonError(err) => (res, err).into(),
            err => Self::MinReq(err),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self::Json("[No Json Provided]".into(), err)
    }
}

impl From<(&str, serde_json::Error)> for Error {
    fn from((json, err): (&str, serde_json::Error)) -> Self {
        Self::Json(json.into(), err)
    }
}

impl From<toml::de::Error> for Error {
    fn from(err: toml::de::Error) -> Self {
        Self::Toml("[No Toml Provided]".into(), err)
    }
}

impl From<(&str, toml::de::Error)> for Error {
    fn from((toml, err): (&str, toml::de::Error)) -> Self {
        Self::Toml(toml.into(), err)
    }
}

impl From<(PathBuf, std::io::Error, &'static str)> for Error {
    fn from((path, err, op): (PathBuf, std::io::Error, &'static str)) -> Self {
        Self::FileIo(path, err, op)
    }
}

pub trait IoContext<T> {
    fn path_ctx<P>(self, path: P, op: &'static str) -> Result<T, Error>
    where
        P: Into<PathBuf>;
}

impl<T> IoContext<T> for Result<T, std::io::Error> {
    fn path_ctx<P>(self, path: P, op: &'static str) -> Result<T, Error>
    where
        P: Into<PathBuf>,
    {
        self.map_err(|err| Error::FileIo(path.into(), err, op))
    }
}
