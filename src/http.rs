#[cfg(target_arch = "wasm32")]
use waki::bindings::wasi::{
    http::{outgoing_handler, types as h},
    io::streams::{InputStream, OutputStream, StreamError},
};

const MAX_ATTEMPTS: u8 = 2;
const REQUEST_BUDGET_MS: u64 = 18_000;

fn retryable(error: &str) -> bool {
    error.starts_with("网络")
        || error.starts_with("读取")
        || error.starts_with("响应")
        || error == "请求超时"
        || ["HTTP 408", "HTTP 502", "HTTP 503", "HTTP 504"].contains(&error)
}
fn public_fallback(url: &str, error: &str) -> Option<&'static str> {
    (error.contains("TlsProtocolError") && url == "https://www.people.com.cn/rss/military.xml")
        .then_some("http://www.people.com.cn/rss/military.xml")
}

#[cfg(target_arch = "wasm32")]
pub struct Response {
    pub bytes: Vec<u8>,
    pub cookies: Vec<String>,
}

// Poll nonblocking WASI p2 resources from short host timer events. No socket waits inside an
// event, so other sources and UI actions continue while a slow publisher is downloading.
#[cfg(target_arch = "wasm32")]
enum Phase {
    Sending(Upload),
    Headers(h::FutureIncomingResponse),
    Body(Download),
    Trailers(h::FutureTrailers),
}

// Struct fields drop in declaration order. Keep streams ahead of their parent bodies even
// on errors, retries and cancellation; destructuring them into locals would reverse that order.
#[cfg(target_arch = "wasm32")]
struct Upload {
    stream: Option<OutputStream>,
    body: Option<h::OutgoingBody>,
    pending: Option<h::FutureIncomingResponse>,
    offset: usize,
    flushing: bool,
}
#[cfg(target_arch = "wasm32")]
struct Download {
    stream: Option<InputStream>,
    body: Option<h::IncomingBody>,
    expected: Option<usize>,
}

#[cfg(target_arch = "wasm32")]
pub struct Request {
    phase: Option<Phase>,
    url: String,
    headers: Vec<(String, String)>,
    payload: Option<String>,
    bytes: Vec<u8>,
    cookies: Vec<String>,
    started: std::time::Instant,
    attempts: u8,
    redirects: u8,
}

#[cfg(target_arch = "wasm32")]
impl Request {
    pub fn new(
        url: &str,
        headers: &[(String, String)],
        payload: Option<&str>,
    ) -> Result<Self, String> {
        let mut request = Self {
            phase: None,
            url: url.into(),
            headers: headers.to_vec(),
            payload: payload.map(str::to_owned),
            bytes: vec![],
            cookies: vec![],
            started: std::time::Instant::now(),
            attempts: 1,
            redirects: 0,
        };
        request.open()?;
        Ok(request)
    }

    fn open(&mut self) -> Result<(), String> {
        self.phase = None;
        self.bytes.clear();
        let parsed = url::Url::parse(&self.url).map_err(|_| "无效来源地址")?;
        let scheme = match parsed.scheme() {
            "https" => h::Scheme::Https,
            "http"
                if matches!(
                    parsed.host_str(),
                    Some("www.people.com.cn" | "military.people.com.cn")
                ) =>
            {
                h::Scheme::Http
            }
            _ => return Err("仅支持 HTTPS 来源".into()),
        };
        let mut fields = vec![
            ("user-agent".into(), b"Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/121.0.0.0 Safari/537.36".to_vec()),
            ("accept-encoding".into(), b"identity".to_vec()),
        ];
        for (key, value) in &self.headers {
            fields.retain(|(name, _)| name != &key.to_ascii_lowercase());
            fields.push((key.to_ascii_lowercase(), value.as_bytes().to_vec()));
        }
        if self.payload.is_some() {
            fields.push(("content-type".into(), b"application/json".to_vec()));
        }
        let request =
            h::OutgoingRequest::new(h::Headers::from_list(&fields).map_err(|e| format!("{e:?}"))?);
        request
            .set_method(&if self.payload.is_some() {
                h::Method::Post
            } else {
                h::Method::Get
            })
            .map_err(|_| "无效方法")?;
        request.set_scheme(Some(&scheme)).map_err(|_| "无效协议")?;
        request
            .set_authority(Some(
                &parsed[url::Position::BeforeHost..url::Position::AfterPort],
            ))
            .map_err(|_| "无效主机")?;
        request
            .set_path_with_query(Some(
                &parsed[url::Position::BeforePath..url::Position::AfterQuery],
            ))
            .map_err(|_| "无效路径")?;
        let body = request.body().map_err(|_| "请求体失败")?;
        let options = h::RequestOptions::new();
        let _ = options.set_connect_timeout(Some(5_000_000_000));
        let _ = options.set_first_byte_timeout(Some(12_000_000_000));
        let _ = options.set_between_bytes_timeout(Some(8_000_000_000));
        let pending = outgoing_handler::handle(request, Some(options))
            .map_err(|e| format!("网络请求失败: {e:?}"))?;
        self.phase = Some(if self.payload.is_some() {
            Phase::Sending(Upload {
                stream: Some(body.write().map_err(|_| "无法写入请求体")?),
                body: Some(body),
                pending: Some(pending),
                offset: 0,
                flushing: false,
            })
        } else {
            h::OutgoingBody::finish(body, None).map_err(|e| format!("{e:?}"))?;
            Phase::Headers(pending)
        });
        Ok(())
    }

    fn redirect(&mut self, next: &str, status: u16, cookies: &[String]) -> Result<(), String> {
        if self.redirects >= 4 {
            return Err("来源重定向次数过多".into());
        }
        let previous = url::Url::parse(&self.url).map_err(|_| "无效来源地址")?;
        let next = crate::news::resolve_url(&self.url, next).ok_or("无效重定向")?;
        let same_origin = url::Url::parse(&next)
            .ok()
            .is_some_and(|u| u.origin() == previous.origin());
        self.headers.retain(|(key, _)| {
            same_origin || !["cookie", "authorization"].contains(&key.to_ascii_lowercase().as_str())
        });
        if same_origin && !cookies.is_empty() {
            let mut jar = std::collections::BTreeMap::new();
            for (_, value) in self
                .headers
                .iter()
                .filter(|(key, _)| key.eq_ignore_ascii_case("cookie"))
            {
                for pair in value.split(';') {
                    if let Some((key, value)) = pair.trim().split_once('=') {
                        jar.insert(key.to_owned(), value.to_owned());
                    }
                }
            }
            for cookie in cookies {
                if let Some((key, value)) = cookie.split(';').next().and_then(|s| s.split_once('='))
                {
                    if value.is_empty() {
                        jar.remove(key);
                    } else {
                        jar.insert(key.to_owned(), value.to_owned());
                    }
                }
            }
            self.headers
                .retain(|(key, _)| !key.eq_ignore_ascii_case("cookie"));
            self.headers.push((
                "cookie".into(),
                jar.into_iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            ));
        }
        self.url = next;
        if status == 303 {
            self.payload = None;
        }
        self.redirects += 1;
        self.open()
    }

    fn step(&mut self) -> Result<Option<Response>, String> {
        let phase = self.phase.take().ok_or("请求已结束")?;
        match phase {
            Phase::Sending(mut upload) => {
                let stream = upload.stream.as_ref().unwrap();
                if !stream.subscribe().ready() {
                    self.phase = Some(Phase::Sending(upload));
                    return Ok(None);
                }
                let payload = self.payload.as_deref().unwrap_or("").as_bytes();
                let permit = stream
                    .check_write()
                    .map_err(|e| format!("写入请求体失败: {e:?}"))?
                    as usize;
                if upload.flushing && permit > 0 {
                    upload.stream.take();
                    h::OutgoingBody::finish(upload.body.take().unwrap(), None)
                        .map_err(|e| format!("{e:?}"))?;
                    self.phase = Some(Phase::Headers(upload.pending.take().unwrap()));
                } else {
                    let end = (upload.offset + permit.min(4096)).min(payload.len());
                    if end > upload.offset {
                        stream
                            .write(&payload[upload.offset..end])
                            .map_err(|e| format!("写入请求体失败: {e:?}"))?;
                        upload.offset = end;
                    }
                    if upload.offset == payload.len() && !upload.flushing {
                        stream
                            .flush()
                            .map_err(|e| format!("写入请求体失败: {e:?}"))?;
                        upload.flushing = true;
                    }
                    self.phase = Some(Phase::Sending(upload));
                }
            }
            Phase::Headers(pending) => {
                // Wasmtime advances Pending -> Ready while polling the subscription. Calling
                // get() alone only inspects that state and can return None forever.
                if !pending.subscribe().ready() {
                    self.phase = Some(Phase::Headers(pending));
                    return Ok(None);
                }
                let Some(response) = pending.get() else {
                    self.phase = Some(Phase::Headers(pending));
                    return Ok(None);
                };
                let response = response
                    .map_err(|_| "响应不可用")?
                    .map_err(|e| format!("网络错误: {e:?}"))?;
                let cookies: Vec<_> = response
                    .headers()
                    .get(&"set-cookie".into())
                    .into_iter()
                    .filter_map(|v| String::from_utf8(v).ok())
                    .collect();
                self.cookies.extend(cookies.clone());
                let status = response.status();
                if matches!(status, 301 | 302 | 303 | 307 | 308) {
                    let location = response
                        .headers()
                        .get(&"location".into())
                        .first()
                        .and_then(|v| String::from_utf8(v.clone()).ok())
                        .ok_or("重定向缺少地址")?;
                    drop(response);
                    drop(pending);
                    self.redirect(&location, status, &cookies)?;
                } else {
                    if status != 200 {
                        return Err(format!("HTTP {status}"));
                    }
                    let expected = response
                        .headers()
                        .get(&"content-length".into())
                        .first()
                        .and_then(|v| std::str::from_utf8(v).ok()?.parse::<usize>().ok());
                    if expected.is_some_and(|n| n > 2 * 1024 * 1024) {
                        return Err("来源响应超过 2 MB".into());
                    }
                    let body = response.consume().map_err(|_| "无法读取响应")?;
                    let stream = body.stream().map_err(|_| "无法读取响应流")?;
                    self.phase = Some(Phase::Body(Download {
                        stream: Some(stream),
                        body: Some(body),
                        expected,
                    }));
                }
            }
            Phase::Body(mut download) => {
                if !download.stream.as_ref().unwrap().subscribe().ready() {
                    self.phase = Some(Phase::Body(download));
                    return Ok(None);
                }
                // Drain a bounded amount per source per tick, yielding fairly to the other sockets.
                for _ in 0..8 {
                    match download.stream.as_ref().unwrap().read(32768) {
                        Ok(bytes) if bytes.is_empty() => break,
                        Ok(bytes) => {
                            if self.bytes.len() + bytes.len() > 2 * 1024 * 1024 {
                                return Err("来源响应超过 2 MB".into());
                            }
                            self.bytes.extend_from_slice(&bytes);
                        }
                        Err(StreamError::Closed) => {
                            download.stream.take();
                            if download.expected.is_some_and(|n| n != self.bytes.len()) {
                                return Err("读取失败: 响应内容不完整".into());
                            }
                            self.phase = Some(Phase::Trailers(h::IncomingBody::finish(
                                download.body.take().unwrap(),
                            )));
                            return Ok(None);
                        }
                        Err(StreamError::LastOperationFailed(error)) => {
                            return Err(format!("读取失败: {}", error.to_debug_string()));
                        }
                    }
                }
                self.phase = Some(Phase::Body(download));
            }
            Phase::Trailers(pending) => {
                if !pending.subscribe().ready() {
                    self.phase = Some(Phase::Trailers(pending));
                    return Ok(None);
                }
                let Some(result) = pending.get() else {
                    self.phase = Some(Phase::Trailers(pending));
                    return Ok(None);
                };
                result
                    .map_err(|_| "响应结束状态不可用")?
                    .map_err(|e| format!("读取失败: {e:?}"))?;
                return Ok(Some(Response {
                    bytes: std::mem::take(&mut self.bytes),
                    cookies: std::mem::take(&mut self.cookies),
                }));
            }
        }
        Ok(None)
    }

    pub fn poll(&mut self) -> Option<Result<Response, String>> {
        if self.started.elapsed().as_millis() >= u128::from(REQUEST_BUDGET_MS) {
            self.phase = None;
            return Some(Err("请求超时".into()));
        }
        match self.step() {
            Ok(Some(response)) => Some(Ok(response)),
            Ok(None) => None,
            Err(error) => {
                self.phase = None;
                if let Some(url) = public_fallback(&self.url, &error) {
                    self.url = url.into();
                } else if self.attempts < MAX_ATTEMPTS && retryable(&error) {
                    self.attempts += 1;
                    self.redirects = 0;
                    self.cookies.clear();
                } else {
                    return Some(Err(error));
                }
                self.open().err().map(Err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_policy_retries_transient_failures_without_hammering_rate_limited_sources() {
        for error in [
            "读取失败: connection reset",
            "网络错误: ConnectionReadTimeout",
            "HTTP 503",
            "请求超时",
        ] {
            assert!(retryable(error), "{error}");
        }
        for error in ["HTTP 404", "HTTP 429", "来源响应超过 2 MB", "无效来源地址"] {
            assert!(!retryable(error), "{error}");
        }
        assert_eq!(MAX_ATTEMPTS, 2);
        assert_eq!(REQUEST_BUDGET_MS, 18_000);
    }

    #[test]
    fn tls_fallback_is_limited_to_the_official_public_people_feed() {
        assert_eq!(
            public_fallback(
                "https://www.people.com.cn/rss/military.xml",
                "网络错误: TlsProtocolError"
            ),
            Some("http://www.people.com.cn/rss/military.xml")
        );
        assert_eq!(
            public_fallback("https://example.com/feed.xml", "网络错误: TlsProtocolError"),
            None
        );
        assert_eq!(
            public_fallback("https://www.people.com.cn/rss/military.xml", "HTTP 404"),
            None
        );
    }
}
