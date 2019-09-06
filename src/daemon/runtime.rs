use reqwest::r#async::Client;
use std::{sync::Arc, time::Duration};
use tokio::runtime::Runtime;

pub struct DaemonRuntime<'a> {
    pub runtime: &'a mut Runtime,
    pub client:  Arc<Client>,
}

impl<'a> DaemonRuntime<'a> {
    pub fn new(runtime: &'a mut Runtime) -> Self {
        // This client contains a thread pool for performing HTTP/s requests.
        let client =
            Arc::new(Client::builder().build().expect("failed to initialize reqwest client"));

        DaemonRuntime { runtime, client }
    }
}
