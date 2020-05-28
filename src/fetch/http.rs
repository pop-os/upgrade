use anyhow::{bail, Context as _};

use future_parking_lot::rwlock::{FutureReadable, FutureWriteable, RwLock};
use futures::prelude::*;
use hreq::{prelude::*, Agent, Body};
use http::{Request, Response, Uri};
use rand::seq::SliceRandom;
use smol::Timer;
use std::{collections::HashMap, fs::File, path::Path, time::Duration};

pub struct Client {
    mirrors: RwLock<HashMap<Uri, Vec<Uri>>>,
}

impl Client {
    pub fn new() -> Self { Self { mirrors: RwLock::new(HashMap::new()) } }

    pub async fn fetch(&self, url: Uri) -> anyhow::Result<Response<Body>> {
        let mut retries = 0u32;

        loop {
            match self.fetch_by_scheme(&url).await {
                o @ Ok(_) => return o,
                e @ Err(_) if retries > 5 => return e,
                _ => {
                    retries += 1;
                    Timer::after(Duration::from_secs(retries as u64)).await;
                }
            }
        }
    }

    pub async fn fetch_by_scheme(&self, uri: &Uri) -> anyhow::Result<Response<Body>> {
        let scheme = uri.scheme_str().with_context(|| fomat!((uri) " lacks scheme"))?;

        let resp = match scheme {
            // HTTP requests
            "http" | "https" => request(uri).await?,

            // The mirrors protocol is a plain text list of addresses
            "mirror" => {
                // Fetch the mirrors associated with this request, unless they're already cached.
                let path = uri.path();
                let mirrors_idx = path
                    .find("mirrors.txt")
                    .context("cannot find mirrors.txt in mirrors protocol")?
                    + 11;

                let mirror_path = &path[..mirrors_idx];
                let package_path = &path[mirrors_idx..];

                let host = uri.host().context("cannot parse host")?.to_string();

                let url = fomat!("http://"(host)(mirror_path))
                    .parse::<Uri>()
                    .expect("reconstructed URL is not valid");

                let mut mirror = None;

                // Try to gain read access for the mirror list before upgrading to write access
                if let Some(mirrors) = self.mirrors.future_read().await.get(&url) {
                    mirror = Some(mirror_uri(&*mirrors, package_path)?);
                };

                if let Some(ref uri) = mirror {
                    return request(uri).await;
                }

                let req = Request::get(&url).with_body(()).unwrap();
                let fetched = self.fetch_mirrors(req).await?;

                self.mirrors.future_write().await.insert(url.clone(), fetched.clone());
                mirror = Some(mirror_uri(&*fetched, package_path)?);

                if let Some(ref uri) = mirror {
                    return request(uri).await;
                }

                // This shouldn't happen, but error if it does
                bail!("failed to find mirrors for {}", url);
            }

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

    async fn fetch_mirrors(&self, req: Request<Body>) -> anyhow::Result<Vec<Uri>> {
        let url = req.uri().clone();
        let response = request(&url).await?;

        let mut mirrors = Vec::new();

        let mut reader = futures::io::BufReader::new(response.into_body());
        let mut line = String::new();

        loop {
            let read = reader.read_line(&mut line).await?;

            if read == 0 {
                break;
            }

            if let Ok(mut url) = line.trim().parse::<Uri>() {
                // Filter mirrors which are broken, or correct those which have been moved.
                let mut agent = Agent::new();
                agent.redirects(0);

                loop {
                    if let Ok(response) =
                        agent.send(Request::head(&url).with_body(()).unwrap()).await
                    {
                        let status = response.status();

                        if status.is_success() {
                            mirrors.push(url);
                        } else if status.is_redirection() {
                            if let Some(location) = response.header("location") {
                                if let Ok(redirect) = location.parse::<Uri>() {
                                    url = redirect;
                                    continue;
                                }
                            }
                        }
                    }

                    break;
                }
            }

            line.clear();
        }

        if mirrors.is_empty() {
            bail!("mirror server at {} does not contain any mirrors", url);
        }

        Ok(mirrors)
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

fn mirror_uri(mirrors: &[Uri], package_path: &str) -> anyhow::Result<Uri> {
    let uri: &Uri = mirrors.choose(&mut rand::thread_rng()).expect("mirror list is empty");

    let scheme_and_host = || -> anyhow::Result<(&str, &str)> {
        let scheme = uri.scheme_str().context("lacks scheme")?;
        let host = uri.host().context("lacks host")?;
        Ok((scheme, host))
    };

    let (scheme, host) =
        scheme_and_host().with_context(|| fomat!("malformed mirror URI (" (uri) ")"))?;
    let uri = fomat!((scheme) "://" (host) "/" (uri.path()) "/" (package_path));
    uri.parse::<Uri>().with_context(|| fomat!("malformed URI (" (uri) ")"))
}
