use pop_upgrade::client::Error as ClientError;
use std::error::Error as ErrorTrait;

#[derive(Debug, Error)]
pub enum UiError {
    #[error(display = "failed to cancel upgrade")]
    Cancel(#[error(cause)] ClientError),
    #[error(display = "failed to dismiss notifications")]
    Dismiss(#[error(cause)] UnderlyingError),
    #[error(display = "failed to finalize release upgrade")]
    Finalize(#[error(cause)] ClientError),
    #[error(display = "recovery upgrade failed")]
    Recovery(#[error(cause)] UnderlyingError),
    #[error(display = "failed to set up OS refresh")]
    Refresh(#[error(cause)] UnderlyingError),
    #[error(display = "failed to modify repos")]
    Repos(#[error(cause)] UnderlyingError),
    #[error(display = "failed to update system")]
    Updates(#[error(cause)] UnderlyingError),
    #[error(display = "failed to upgrade OS")]
    Upgrade(#[error(cause)] UnderlyingError),
}

impl UiError {
    pub fn iter_sources(&self) -> ErrorIter<'_> { ErrorIter { current: self.source() } }
}

#[derive(Debug, Error)]
#[error(display = "{}", _0)]
pub struct StatusError(Box<str>);

#[derive(Debug, Error)]
pub enum UnderlyingError {
    #[error(display = "client error")]
    Client(#[error(cause)] ClientError),
    #[error(display = "failed status")]
    Status(#[error(cause)] StatusError),
}

impl From<Box<str>> for UnderlyingError {
    fn from(why: Box<str>) -> Self { UnderlyingError::Status(StatusError(why)) }
}

impl From<ClientError> for UnderlyingError {
    fn from(why: ClientError) -> Self { UnderlyingError::Client(why) }
}

pub struct ErrorIter<'a> {
    current: Option<&'a (dyn ErrorTrait + 'static)>,
}

impl<'a> Iterator for ErrorIter<'a> {
    type Item = &'a (dyn ErrorTrait + 'static);

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current;
        self.current = self.current.and_then(|ref why| why.source());
        current
    }
}
