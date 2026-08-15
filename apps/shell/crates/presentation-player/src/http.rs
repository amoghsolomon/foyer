//! GPUI's default client rejects network requests. This adapter keeps remote user-provided images
//! working while searched images normally arrive through the sidecar's validated local cache.

use std::{thread, time::Duration};

const MAX_REMOTE_IMAGE_BYTES: u64 = 32 * 1024 * 1024;

pub(crate) struct FoyerShellHttpClient {
    agent: ureq::Agent,
    user_agent: gpui::http_client::http::HeaderValue,
}

impl FoyerShellHttpClient {
    pub(crate) fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .build();
        Self {
            agent: config.into(),
            user_agent: gpui::http_client::http::HeaderValue::from_static(
                "FoyerShell/0.1 (+native image loader)",
            ),
        }
    }

    fn execute(
        agent: ureq::Agent,
        user_agent: gpui::http_client::http::HeaderValue,
        request: gpui::http_client::Request<gpui::http_client::AsyncBody>,
    ) -> gpui::http_client::Result<gpui::http_client::Response<gpui::http_client::AsyncBody>> {
        use gpui::http_client::http::header::{ACCEPT, USER_AGENT};

        if request.method() != gpui::http_client::Method::GET {
            return Err(gpui::http_client::anyhow!(
                "Foyer Shell image client only supports GET requests"
            ));
        }

        let (mut parts, _) = request.into_parts();
        parts.headers.entry(USER_AGENT).or_insert(user_agent);
        parts.headers.entry(ACCEPT).or_insert_with(|| {
            gpui::http_client::http::HeaderValue::from_static(
                "image/avif,image/webp,image/png,image/jpeg,image/*;q=0.9,*/*;q=0.5",
            )
        });

        let response = agent.run(gpui::http_client::Request::from_parts(parts, ()))?;
        let (parts, mut body) = response.into_parts();
        let bytes = body
            .with_config()
            .limit(MAX_REMOTE_IMAGE_BYTES)
            .read_to_vec()?;
        Ok(gpui::http_client::Response::from_parts(parts, bytes.into()))
    }
}

impl gpui::http_client::HttpClient for FoyerShellHttpClient {
    fn user_agent(&self) -> Option<&gpui::http_client::http::HeaderValue> {
        Some(&self.user_agent)
    }

    fn send(
        &self,
        request: gpui::http_client::Request<gpui::http_client::AsyncBody>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = gpui::http_client::Result<
                        gpui::http_client::Response<gpui::http_client::AsyncBody>,
                    >,
                > + Send
                + 'static,
        >,
    > {
        let agent = self.agent.clone();
        let user_agent = self.user_agent.clone();
        let (sender, receiver) = async_channel::bounded(1);
        let spawn = thread::Builder::new()
            .name("foyer-shell-image-fetch".into())
            .spawn(move || {
                let result = Self::execute(agent, user_agent, request);
                let _ = sender.send_blocking(result);
            });

        Box::pin(async move {
            spawn.map_err(|error| gpui::http_client::anyhow!(error))?;
            receiver
                .recv()
                .await
                .map_err(|error| gpui::http_client::anyhow!(error))?
        })
    }

    fn proxy(&self) -> Option<&gpui::http_client::Url> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn delivers_remote_image_bytes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let image_bytes = b"foyer-shell-image";
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                image_bytes.len()
            )
            .unwrap();
            stream.write_all(image_bytes).unwrap();
        });

        let client = FoyerShellHttpClient::new();
        let request = gpui::http_client::Request::builder()
            .uri(format!("http://{address}/image.png"))
            .body(gpui::http_client::AsyncBody::empty())
            .unwrap();
        let response =
            FoyerShellHttpClient::execute(client.agent.clone(), client.user_agent.clone(), request)
                .unwrap();
        server.join().unwrap();

        assert_eq!(response.status(), gpui::http_client::StatusCode::OK);
        let gpui::http_client::Inner::Bytes(bytes) = response.into_body().0 else {
            panic!("remote image response should be buffered in memory");
        };
        assert_eq!(bytes.into_inner().as_ref(), image_bytes);
    }
}
