use anyhow::{bail, Context as _};
use futures::prelude::*;
use hreq::{prelude::*, Body};
use http::{Request, Response, Uri};
use smol::Timer;
use std::{fs::File, path::Path, time::Duration};

pub struct Client;

impl Client {
    pub fn new() -> Self { Self }

    pub async fn fetch(&self, url: Uri) -> anyhow::Result<Response<Body>> {
        let mut retries = 0u32;

        loop {
            match self.fetch_(&url).await {
                o @ Ok(_) => return o,
                e @ Err(_) if retries > 5 => return e,
                _ => {
                    retries += 1;
                    Timer::after(Duration::from_secs(retries as u64)).await;
                }
            }
        }
    }

    pub async fn fetch_(&self, uri: &Uri) -> anyhow::Result<Response<Body>> {
        let scheme = uri.scheme_str().with_context(|| fomat!((uri) " lacks scheme"))?;

        let resp = match scheme {
            // HTTP requests
            "http" | "https" => request(uri).await?,

            scheme => bail!("unsupported scheme: {}", scheme),
        };

        Ok(resp)
    }

    pub async fn fetch_to_path(&self, uri: &str, path: &Path) -> anyhow::Result<()> {
        // The file where we shall store the body at.
        let mut partial = {
            let path_clone: Box<Path> = path.into();
            let partial = smol::blocking! {
                File::create(&path_clone)
                    .with_context(|| fomat!("failed to create partial at "[path_clone]))
            }?;

            smol::writer(partial)
        };

        let url = uri.parse::<Uri>().with_context(|| fomat!("failed to parse URL: "(uri)))?;

        let response = self.fetch(url).await.with_context(|| fomat!("failed to request "(uri)))?;

        futures::io::copy(response.into_body(), &mut partial)
            .await
            .with_context(|| fomat!("streaming to " [path] " failed"))?;

        let _ = partial.flush().await;

        Ok(())
    }
}

async fn request(uri: &Uri) -> anyhow::Result<Response<Body>> {
    let response = Request::get(uri)
        .with_body(())
        .unwrap()
        .send()
        .await
        .with_context(|| fomat!("request for " (uri) " failed"))?;

    let status = response.status();

    if !status.is_success() {
        let msg = fomat!(
            "HTTP error "
            (u16::from(status))
            " connecting to " (uri)
            if let Some(reason) = status.canonical_reason() {
                ": " (reason)
            }
        );
        return Err(anyhow!(msg));
    }

    Ok(response)
}
