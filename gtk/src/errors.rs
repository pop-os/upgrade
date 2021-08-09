use pop_upgrade::client::Error as ClientError;
use std::error::Error as ErrorTrait;

#[derive(Debug, Error)]
pub enum UiError {
    #[error("failed to cancel upgrade")]
    Cancel(#[source] ClientError),
    #[error("failed to dismiss notifications")]
    Dismiss(bool, #[source] UnderlyingError),
    #[error("failed to finalize release upgrade")]
    Finalize(#[source] ClientError),
    #[error("recovery upgrade failed")]
    Recovery(#[source] UnderlyingError),
    #[error("failed to set up OS refresh")]
    Refresh(#[source] UnderlyingError),
    #[error("failed to update system")]
    Updates(#[source] UnderlyingError),
    #[error("failed to upgrade OS")]
    Upgrade(#[source] UnderlyingError),
}

impl UiError {
    pub fn iter_sources(&self) -> ErrorIter<'_> { ErrorIter { current: self.source() } }
}

#[derive(Debug, Error)]
#[error("{}", _0)]
pub struct StatusError(Box<str>);

#[derive(Debug, Error)]
pub enum UnderlyingError {
    #[error("client error")]
    Client(#[from] ClientError),
    #[error("failed status")]
    Status(#[from] StatusError),
}

impl From<Box<str>> for UnderlyingError {
    fn from(why: Box<str>) -> Self { UnderlyingError::Status(StatusError(why)) }
}

pub struct ErrorIter<'a> {
    current: Option<&'a (dyn ErrorTrait + 'static)>,
}

impl<'a> Iterator for ErrorIter<'a> {
    type Item = &'a (dyn ErrorTrait + 'static);

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current;
        self.current = self.current.and_then(|why| why.source());
        current
    }
}
