use std::io::ErrorKind;
use std::time::Instant;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use super::config::{DavConfig, MAX_HEADER_BYTES, MAX_REDIRECTS, USER_AGENT, basic_auth_header};
use super::error::DavError;
use super::path::{DavHref, HttpUrl};

#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub method: String,
    pub href: DavHref,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn new(method: impl Into<String>, href: DavHref) -> Self {
        Self {
            method: method.into(),
            href,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>, content_type: &str) -> Self {
        self.body = body.into();
        self.headers
            .push(("Content-Type".into(), content_type.into()));
        self.headers
            .push(("Content-Length".into(), self.body.len().to_string()));
        self
    }

    fn allows_redirect(&self) -> bool {
        matches!(
            self.method.as_str(),
            "GET" | "HEAD" | "OPTIONS" | "PROPFIND" | "REPORT"
        )
    }
}

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find_map(|(key, value)| {
            if key.eq_ignore_ascii_case(name) {
                Some(value.as_str())
            } else {
                None
            }
        })
    }

    pub fn text(&self) -> Result<&str, DavError> {
        std::str::from_utf8(&self.body)
            .map_err(|_| DavError::MalformedRemote("DAV response body is not valid UTF-8".into()))
    }
}

pub async fn exchange(config: &DavConfig, request: HttpRequest) -> Result<HttpResponse, DavError> {
    let base = config.origin()?;
    timeout(
        config.request_timeout,
        exchange_follow(config, &base, request),
    )
    .await
    .map_err(|_| DavError::Timeout)?
}

async fn exchange_follow(
    config: &DavConfig,
    base: &HttpUrl,
    mut request: HttpRequest,
) -> Result<HttpResponse, DavError> {
    let mut hops = 0;
    loop {
        let response = send_once(config, base, &request).await?;
        if !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            return Ok(response);
        }
        if !request.allows_redirect() {
            return Err(DavError::Protocol {
                status: response.status,
                detail: "redirect is not followed for this DAV method".into(),
            });
        }
        hops += 1;
        if hops > MAX_REDIRECTS {
            return Err(DavError::Protocol {
                status: response.status,
                detail: "too many DAV redirects".into(),
            });
        }
        let location = response
            .header("location")
            .ok_or_else(|| DavError::Protocol {
                status: response.status,
                detail: "redirect is missing Location".into(),
            })?;
        request.href = resolve_redirect(base, location)?;
    }
}

async fn send_once(
    config: &DavConfig,
    base: &HttpUrl,
    request: &HttpRequest,
) -> Result<HttpResponse, DavError> {
    validate_headers(&request.headers)?;
    let mut stream = timeout(config.connect_timeout, TcpStream::connect(base.authority()))
        .await
        .map_err(|_| DavError::Timeout)?
        .map_err(|error| transport(config, error))?;
    stream
        .set_nodelay(true)
        .map_err(|error| transport(config, error))?;

    let wire = encode_request(config, base, request)?;
    stream
        .write_all(&wire)
        .await
        .map_err(|error| transport(config, error))?;
    stream
        .flush()
        .await
        .map_err(|error| transport(config, error))?;

    read_response(&mut stream, config).await
}

fn encode_request(
    config: &DavConfig,
    base: &HttpUrl,
    request: &HttpRequest,
) -> Result<Vec<u8>, DavError> {
    let mut out = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nAuthorization: {}\r\nConnection: close\r\nAccept: */*\r\n",
        request.method,
        request.href.request_target(),
        base.authority(),
        USER_AGENT,
        basic_auth_header(&config.username, config.password.expose())?
    );
    let mut has_content_length = false;
    for (name, value) in &request.headers {
        if name.eq_ignore_ascii_case("authorization")
            || name.eq_ignore_ascii_case("host")
            || name.eq_ignore_ascii_case("connection")
        {
            continue;
        }
        if name.eq_ignore_ascii_case("content-length") {
            has_content_length = true;
        }
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    if !request.body.is_empty() && !has_content_length {
        out.push_str(&format!("Content-Length: {}\r\n", request.body.len()));
    }
    out.push_str("\r\n");
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(&request.body);
    Ok(bytes)
}

async fn read_response(
    stream: &mut TcpStream,
    config: &DavConfig,
) -> Result<HttpResponse, DavError> {
    let mut buf = Vec::new();
    let header_end = read_until_headers(stream, &mut buf).await?;
    let header_text = std::str::from_utf8(&buf[..header_end]).map_err(|_| {
        DavError::MalformedRemote("DAV response headers are not valid UTF-8".into())
    })?;
    let (status, reason, headers) = parse_headers(header_text)?;
    let mut body = buf.split_off(header_end);
    if let Some(length) = content_length(&headers)? {
        if length > config.max_response_bytes {
            return Err(DavError::ResponseTooLarge {
                limit: config.max_response_bytes,
            });
        }
        while body.len() < length {
            read_more(stream, &mut body, config.max_response_bytes).await?;
        }
        body.truncate(length);
    } else if is_chunked(&headers) {
        body = decode_chunked(stream, body, config.max_response_bytes).await?;
    } else {
        while read_more(stream, &mut body, config.max_response_bytes)
            .await
            .is_ok()
        {}
        if body.len() > config.max_response_bytes {
            return Err(DavError::ResponseTooLarge {
                limit: config.max_response_bytes,
            });
        }
    }
    Ok(HttpResponse {
        status,
        reason,
        headers,
        body,
    })
}

async fn read_until_headers(stream: &mut TcpStream, buf: &mut Vec<u8>) -> Result<usize, DavError> {
    let started = Instant::now();
    loop {
        if let Some(index) = find_header_end(buf) {
            return Ok(index);
        }
        if buf.len() > MAX_HEADER_BYTES {
            return Err(DavError::ResponseTooLarge {
                limit: MAX_HEADER_BYTES,
            });
        }
        if started.elapsed().as_secs() > 30 {
            return Err(DavError::Timeout);
        }
        let mut chunk = [0_u8; 1024];
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|error| DavError::Transport(format!("failed to read DAV headers: {error}")))?;
        if read == 0 {
            return Err(DavError::MalformedRemote(
                "DAV response ended before headers completed".into(),
            ));
        }
        buf.extend_from_slice(&chunk[..read]);
    }
}

async fn read_more(
    stream: &mut TcpStream,
    body: &mut Vec<u8>,
    limit: usize,
) -> Result<(), DavError> {
    if body.len() > limit {
        return Err(DavError::ResponseTooLarge { limit });
    }
    let mut chunk = [0_u8; 4096];
    let read = stream
        .read(&mut chunk)
        .await
        .map_err(|error| DavError::Transport(format!("failed to read DAV body: {error}")))?;
    if read == 0 {
        return Err(DavError::Transport("DAV body ended early".into()));
    }
    if body.len() + read > limit {
        return Err(DavError::ResponseTooLarge { limit });
    }
    body.extend_from_slice(&chunk[..read]);
    Ok(())
}

async fn decode_chunked(
    stream: &mut TcpStream,
    mut pending: Vec<u8>,
    limit: usize,
) -> Result<Vec<u8>, DavError> {
    let mut body = Vec::new();
    loop {
        let line = take_line(stream, &mut pending).await?;
        let size_hex = line
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .trim_end_matches('\r');
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| DavError::MalformedRemote("DAV chunk size is invalid".into()))?;
        if size == 0 {
            let _ = take_line(stream, &mut pending).await;
            return Ok(body);
        }
        if body.len() + size > limit {
            return Err(DavError::ResponseTooLarge { limit });
        }
        while pending.len() < size + 2 {
            read_more(stream, &mut pending, limit.saturating_add(8)).await?;
        }
        body.extend_from_slice(&pending[..size]);
        pending.drain(..size);
        if pending.starts_with(b"\r\n") {
            pending.drain(..2);
        } else if pending.starts_with(b"\n") {
            pending.drain(..1);
        } else {
            return Err(DavError::MalformedRemote(
                "DAV chunk is missing a trailing CRLF".into(),
            ));
        }
    }
}

async fn take_line(stream: &mut TcpStream, pending: &mut Vec<u8>) -> Result<String, DavError> {
    loop {
        if let Some(index) = pending.iter().position(|b| *b == b'\n') {
            let line = pending.drain(..=index).collect::<Vec<_>>();
            return String::from_utf8(line).map_err(|_| {
                DavError::MalformedRemote("DAV chunk line is not valid UTF-8".into())
            });
        }
        if pending.len() > 128 {
            return Err(DavError::MalformedRemote(
                "DAV chunk line is too long".into(),
            ));
        }
        read_more(stream, pending, MAX_HEADER_BYTES).await?;
    }
}

type ParsedHeaders = (u16, String, Vec<(String, String)>);

fn parse_headers(header_text: &str) -> Result<ParsedHeaders, DavError> {
    let mut lines = header_text.split('\n');
    let status_line = lines
        .next()
        .ok_or_else(|| DavError::MalformedRemote("DAV response is missing a status line".into()))?
        .trim_end_matches('\r');
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("");
    if version != "HTTP/1.0" && version != "HTTP/1.1" {
        return Err(DavError::MalformedRemote(
            "DAV response is not HTTP/1.x".into(),
        ));
    }
    let status = parts
        .next()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| DavError::MalformedRemote("DAV status code is invalid".into()))?;
    let reason = parts.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    for line in lines {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(DavError::MalformedRemote(
                "DAV header is missing a colon".into(),
            ));
        };
        headers.push((name.trim().to_string(), value.trim().to_string()));
    }
    Ok((status, reason, headers))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .or_else(|| {
            buf.windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| index + 2)
        })
}

fn content_length(headers: &[(String, String)]) -> Result<Option<usize>, DavError> {
    match headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
    {
        Some((_, value)) => value
            .parse()
            .map(Some)
            .map_err(|_| DavError::MalformedRemote("Content-Length is invalid".into())),
        None => Ok(None),
    }
}

fn is_chunked(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-encoding")
            && value
                .to_ascii_lowercase()
                .split(',')
                .any(|part| part.trim() == "chunked")
    })
}

fn validate_headers(headers: &[(String, String)]) -> Result<(), DavError> {
    for (name, value) in headers {
        if name.is_empty()
            || name
                .chars()
                .any(|ch| ch.is_control() || ch == ':' || ch == ' ')
            || value.chars().any(|ch| matches!(ch, '\r' | '\n' | '\0'))
        {
            return Err(DavError::InvalidRequest(
                "DAV request header contains reserved characters".into(),
            ));
        }
    }
    Ok(())
}

fn resolve_redirect(base: &HttpUrl, location: &str) -> Result<DavHref, DavError> {
    DavHref::from_dav(base, location)
}

fn transport(config: &DavConfig, error: std::io::Error) -> DavError {
    if error.kind() == ErrorKind::TimedOut {
        return DavError::Timeout;
    }
    DavError::Transport(config.redact(&error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_header_injection() {
        let error = validate_headers(&[("X-Test".into(), "one\r\nX-Injected: yes".into())]);
        assert!(error.is_err());
    }

    #[test]
    fn parses_status_and_headers() {
        let (status, reason, headers) =
            parse_headers("HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\n\r\n")
                .unwrap();
        assert_eq!(status, 207);
        assert_eq!(reason, "Multi-Status");
        assert_eq!(headers[0].1, "application/xml");
    }
}
