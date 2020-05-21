use crate::fetch::http::Client;
use std::sync::Arc;

pub struct DaemonRuntime {
    pub client: Arc<Client>,
}

impl DaemonRuntime {
    pub fn new() -> Self { DaemonRuntime { client: Arc::new(Client::new()) } }
}
