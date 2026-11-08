//! Roku External Control Protocol: plain HTTP on port 8060, SSDP discovery.
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
pub struct Device {
    pub host: String,
    pub name: String,
    pub is_tv: bool,
    /// "Control by mobile apps" is Limited: only volume keys are accepted.
    pub limited: bool,
}

#[derive(Clone, Debug)]
pub struct App {
    pub id: String,
    pub name: String,
}

fn addr(host: &str) -> Result<SocketAddr, String> {
    let with_port = if host.contains(':') { host.to_string() } else { format!("{host}:8060") };
    with_port
        .to_socket_addrs()
        .map_err(|e| format!("{host}: {e}"))?
        .next()
        .ok_or_else(|| format!("{host}: no address"))
}

fn request(host: &str, method: &str, path: &str) -> Result<String, String> {
    request_bytes(host, method, path).map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn request_bytes(host: &str, method: &str, path: &str) -> Result<Vec<u8>, String> {
    let sock = addr(host)?;
    let mut stream = TcpStream::connect_timeout(&sock, TIMEOUT).map_err(|e| format!("{host}: {e}"))?;
    stream.set_read_timeout(Some(TIMEOUT)).ok();
    stream.set_write_timeout(Some(TIMEOUT)).ok();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {sock}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let head_end = buf.windows(4).position(|w| w == b"\r\n\r\n");
    let head = String::from_utf8_lossy(&buf[..head_end.unwrap_or(buf.len())]);
    let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        return Err(format!("{host}: chunked responses are not supported"));
    }
    match status {
        200..=299 => Ok(head_end.map(|i| buf[i + 4..].to_vec()).unwrap_or_default()),
        403 => Err("TV refused the command. On the TV, set Settings > System > Advanced system \
                    settings > Control by mobile apps to Enabled."
            .into()),
        0 => Err(format!("{host}: bad response")),
        s => Err(format!("{host}: HTTP {s}")),
    }
}

pub fn keypress(host: &str, key: &str) -> Result<(), String> {
    request(host, "POST", &format!("/keypress/{key}")).map(drop)
}

/// Type text into the focused on-screen keyboard, one `Lit_` key per char.
pub fn type_text(host: &str, text: &str) -> Result<(), String> {
    for c in text.chars() {
        let mut bytes = [0u8; 4];
        let encoded: String = c.encode_utf8(&mut bytes).bytes().map(|b| format!("%{b:02X}")).collect();
        keypress(host, &format!("Lit_{encoded}"))?;
    }
    Ok(())
}

pub fn launch(host: &str, id: &str) -> Result<(), String> {
    request(host, "POST", &format!("/launch/{id}")).map(drop)
}

/// A channel's logo (WebP, PNG or JPEG).
pub fn icon(host: &str, id: &str) -> Result<Vec<u8>, String> {
    request_bytes(host, "GET", &format!("/query/icon/{id}"))
}

fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    Some(&xml[start..end])
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&apos;", "'").replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">")
}

pub fn device_info(host: &str) -> Result<Device, String> {
    let xml = request(host, "GET", "/query/device-info")?;
    let name = tag(&xml, "user-device-name")
        .or_else(|| tag(&xml, "friendly-device-name"))
        .or_else(|| tag(&xml, "model-name"))
        .filter(|n| !n.is_empty())
        .map(unescape)
        .unwrap_or_else(|| host.to_string());
    Ok(Device {
        host: host.to_string(),
        name,
        is_tv: tag(&xml, "is-tv") == Some("true"),
        limited: tag(&xml, "ecp-setting-mode") == Some("limited"),
    })
}

pub fn apps(host: &str) -> Result<Vec<App>, String> {
    let xml = request(host, "GET", "/query/apps")?;
    let mut out = Vec::new();
    for chunk in xml.split("<app ").skip(1) {
        let Some(id) = chunk.split("id=\"").nth(1).and_then(|s| s.split('"').next()) else { continue };
        let Some(name) = chunk.split_once('>').and_then(|(_, r)| r.split("</app>").next()) else { continue };
        out.push(App { id: id.to_string(), name: unescape(name) });
    }
    Ok(out)
}

/// SSDP search for Roku devices on the local network.
pub fn discover() -> Vec<Device> {
    let mut hosts: Vec<String> = Vec::new();
    if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
        let msg = "M-SEARCH * HTTP/1.1\r\nHost: 239.255.255.250:1900\r\nMan: \"ssdp:discover\"\r\nMX: 2\r\nST: roku:ecp\r\n\r\n";
        for _ in 0..2 {
            let _ = sock.send_to(msg.as_bytes(), "239.255.255.250:1900");
        }
        let deadline = Instant::now() + Duration::from_millis(2500);
        let mut buf = [0u8; 2048];
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            sock.set_read_timeout(Some(left.max(Duration::from_millis(1)))).ok();
            let Ok((n, _)) = sock.recv_from(&mut buf) else { break };
            let text = String::from_utf8_lossy(&buf[..n]);
            let location = text.lines().find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim().eq_ignore_ascii_case("location").then(|| v.trim().to_string())
            });
            if let Some(host) = location.and_then(|l| {
                l.strip_prefix("http://").map(|r| r.trim_end_matches('/').trim_end_matches(":8060").to_string())
            }) && !hosts.contains(&host)
            {
                hosts.push(host);
            }
        }
    }
    hosts.iter().map(|h| device_info(h).unwrap_or(Device { host: h.clone(), name: h.clone(), is_tv: false, limited: false })).collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum State {
    #[default]
    Stopped,
    Playing,
    Paused,
}

/// What the TV is showing: the open channel and, when a video is loaded,
/// its playback state and progress.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Playing {
    pub app_id: String,
    pub app: String,
    pub state: State,
    pub position_ms: u64,
    pub duration_ms: u64,
}

fn millis(xml: &str, name: &str) -> u64 {
    tag(xml, name).and_then(|v| v.trim().trim_end_matches("ms").trim().parse().ok()).unwrap_or(0)
}

pub fn now_playing(host: &str) -> Result<Playing, String> {
    let xml = request(host, "GET", "/query/active-app")?;
    let app = xml.split("<app").nth(1).unwrap_or_default();
    let app_id = app.split("id=\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or_default();
    let name = app.split_once('>').and_then(|(_, r)| r.split("</app>").next()).map(unescape).unwrap_or_default();
    let mut now = Playing {
        app_id: app_id.to_string(),
        // With no channel open the TV reports just "Roku".
        app: if app_id.is_empty() { "Home".into() } else { name },
        ..Default::default()
    };
    // Media state is refused in Limited mode; the channel name still shows.
    if !app_id.is_empty()
        && let Ok(xml) = request(host, "GET", "/query/media-player")
    {
        let state = xml.split("state=\"").nth(1).and_then(|s| s.split('"').next()).unwrap_or_default();
        now.state = match state {
            "play" | "buffering" => State::Playing,
            "pause" => State::Paused,
            _ => State::Stopped,
        };
        if now.state != State::Stopped {
            now.position_ms = millis(&xml, "position");
            now.duration_ms = millis(&xml, "duration");
        }
    }
    Ok(now)
}
