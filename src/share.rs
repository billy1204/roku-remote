//! Share a folder with Roku Media Player.
//!
//! Rokus can't be sent a file to play, but Roku Media Player browses DLNA
//! media servers on the network. This is a minimal one: SSDP so the TV finds
//! it, a ContentDirectory that lists the folder, and HTTP (with ranges, for
//! seeking) that serves the files. Read-only; nothing outside the folder is
//! reachable.
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Roku Media Player's channel id.
pub const MEDIA_PLAYER: &str = "2213";

const SSDP: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const PORTS: std::ops::RangeInclusive<u16> = 8200..=8209;
const DEVICE: &str = "urn:schemas-upnp-org:device:MediaServer:1";
const CD: &str = "urn:schemas-upnp-org:service:ContentDirectory:1";
const CM: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";

pub struct Share {
    server: Arc<Server>,
    port: u16,
    /// Announcements went out; false when another program holds port 1900
    /// in a way we can't share, so the TV may not find us.
    pub announced: bool,
}

struct Server {
    root: PathBuf,
    base: String,
    uuid: String,
    name: String,
    stop: AtomicBool,
    /// When each address last talked to us over HTTP.
    seen: Mutex<HashMap<String, Instant>>,
}

impl Share {
    pub fn start(folder: &Path) -> Result<Self, String> {
        let root = fs::canonicalize(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        if !root.is_dir() {
            return Err(format!("{} is not a folder", root.display()));
        }
        let ip = local_ip().ok_or("This computer isn't on a network")?;
        let (listener, port) = PORTS
            .clone()
            .find_map(|p| TcpListener::bind(("0.0.0.0", p)).ok().map(|l| (l, p)))
            .ok_or("Ports 8200–8209 are all in use")?;
        let server = Arc::new(Server {
            uuid: uuid_for(&hostname()),
            name: server_name(),
            base: format!("http://{ip}:{port}"),
            root,
            stop: AtomicBool::new(false),
            seen: Mutex::default(),
        });
        let s = server.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if s.stop.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let s = s.clone();
                std::thread::spawn(move || {
                    let _ = s.handle(stream);
                });
            }
        });
        let announced = match ssdp_socket(ip) {
            Ok(sock) => {
                let s = server.clone();
                std::thread::spawn(move || s.ssdp(sock));
                true
            }
            Err(_) => false,
        };
        Ok(Self { server, port, announced })
    }

    pub fn folder(&self) -> &Path {
        &self.server.root
    }

    /// When the TV at `host` last browsed or played from the share.
    pub fn last_seen(&self, host: &str) -> Option<Instant> {
        self.server.seen.lock().unwrap().get(host).copied()
    }
}

impl Drop for Share {
    fn drop(&mut self) {
        self.server.stop.store(true, Ordering::Relaxed);
        // Tell the TV we're gone, then wake the accept loop so it sees the flag.
        if let Ok(sock) = UdpSocket::bind("0.0.0.0:0") {
            for (nt, usn) in self.server.types() {
                let msg = format!("NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nNT: {nt}\r\nNTS: ssdp:byebye\r\nUSN: {usn}\r\n\r\n");
                let _ = sock.send_to(msg.as_bytes(), (SSDP, 1900));
            }
        }
        let _ = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], self.port)), Duration::from_millis(200));
    }
}

/// What the TV lists the share as.
pub fn server_name() -> String {
    format!("Roku Remote ({})", hostname())
}

fn hostname() -> String {
    #[cfg(windows)]
    let name = std::env::var("COMPUTERNAME").ok();
    #[cfg(not(windows))]
    let name = fs::read_to_string("/etc/hostname").ok();
    name.map(|h| h.trim().to_string()).filter(|h| !h.is_empty()).unwrap_or_else(|| "PC".into())
}

/// The address the TV can reach us on: the one the OS would route from.
fn local_ip() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect((SSDP, 1900)).ok()?;
    match s.local_addr().ok()? {
        SocketAddr::V4(a) if !a.ip().is_unspecified() => Some(*a.ip()),
        _ => None,
    }
}

/// Stable per machine, so the TV sees the same server each time.
fn uuid_for(host: &str) -> String {
    // FNV-1a: stable across Rust versions, unlike DefaultHasher.
    let mut h: u128 = 0x6c62272e07bb014262b821756295c58d;
    for b in format!("roku-remote-media-share:{host}").bytes() {
        h ^= b as u128;
        h = h.wrapping_mul(0x0000000001000000000000000000013B);
    }
    let x = format!("{h:032x}");
    format!("{}-{}-4{}-a{}-{}", &x[0..8], &x[8..12], &x[13..16], &x[17..20], &x[20..32])
}

// ---- Firewall ----

/// A firewall that may keep the TV out, and how to let it in.
pub struct Firewall {
    pub name: &'static str,
    pub fix: String,
}

/// Look for a firewall that filters incoming connections. This can't see
/// its rules without admin rights, so a hit means "may be blocking"; the TV
/// actually connecting is what proves it isn't.
pub fn firewall() -> Option<Firewall> {
    #[cfg(target_os = "linux")]
    {
        let net = local_ip().map(|ip| {
            let [a, b, c, _] = ip.octets();
            format!("{a}.{b}.{c}.0/24")
        });
        let net = net.as_deref().unwrap_or("192.168.0.0/16");
        let ufw_on = fs::read_to_string("/etc/ufw/ufw.conf")
            .is_ok_and(|c| c.lines().any(|l| l.trim().eq_ignore_ascii_case("ENABLED=yes")));
        if ufw_on {
            return Some(Firewall {
                name: "ufw",
                fix: format!(
                    "sudo ufw allow from {net} to any port 8200:8209 proto tcp\nsudo ufw allow from {net} to any port 1900 proto udp"
                ),
            });
        }
        let firewalld_on = std::process::Command::new("systemctl")
            .args(["is-active", "--quiet", "firewalld"])
            .status()
            .is_ok_and(|s| s.success());
        if firewalld_on {
            return Some(Firewall {
                name: "firewalld",
                fix: "sudo firewall-cmd --permanent --add-port=8200-8209/tcp --add-port=1900/udp\nsudo firewall-cmd --reload".into(),
            });
        }
        None
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = std::process::Command::new("netsh")
            .args(["advfirewall", "show", "currentprofile", "state"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        let on = String::from_utf8_lossy(&out.stdout).lines().any(|l| l.trim_start().starts_with("State") && l.contains("ON"));
        on.then(|| Firewall {
            name: "Windows Firewall",
            fix: "Open Windows Security > Firewall & network protection > Allow an app through firewall, \
                  and tick Private for Roku Remote."
                .into(),
        })
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    None
}

// ---- SSDP ----

fn ssdp_socket(ip: Ipv4Addr) -> io::Result<UdpSocket> {
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    #[cfg(unix)]
    sock.set_reuse_port(true)?;
    sock.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 1900).into())?;
    sock.join_multicast_v4(&SSDP, &ip)?;
    sock.set_multicast_if_v4(&ip)?;
    sock.set_read_timeout(Some(Duration::from_secs(1)))?;
    Ok(sock.into())
}

fn header<'a>(msg: &'a str, name: &str) -> Option<&'a str> {
    msg.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

impl Server {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn types(&self) -> Vec<(String, String)> {
        let uuid = format!("uuid:{}", self.uuid);
        vec![
            ("upnp:rootdevice".to_string(), format!("{uuid}::upnp:rootdevice")),
            (uuid.clone(), uuid.clone()),
            (DEVICE.to_string(), format!("{uuid}::{DEVICE}")),
            (CD.to_string(), format!("{uuid}::{CD}")),
            (CM.to_string(), format!("{uuid}::{CM}")),
        ]
    }

    /// Announce ourselves every minute and answer searches.
    fn ssdp(&self, sock: UdpSocket) {
        let location = format!("{}/desc.xml", self.base);
        let mut next_notify = Instant::now();
        let mut buf = [0u8; 2048];
        while !self.stopped() {
            if Instant::now() >= next_notify {
                for (nt, usn) in self.types() {
                    let msg = format!(
                        "NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: {location}\r\nNT: {nt}\r\nNTS: ssdp:alive\r\nSERVER: {SERVER}\r\nUSN: {usn}\r\n\r\n"
                    );
                    let _ = sock.send_to(msg.as_bytes(), (SSDP, 1900));
                }
                next_notify = Instant::now() + Duration::from_secs(60);
            }
            let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
            let msg = String::from_utf8_lossy(&buf[..n]);
            if !msg.starts_with("M-SEARCH") {
                continue;
            }
            let Some(st) = header(&msg, "ST") else { continue };
            for (nt, usn) in self.types() {
                if st == "ssdp:all" || st == nt {
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nEXT:\r\nLOCATION: {location}\r\nSERVER: {SERVER}\r\nST: {nt}\r\nUSN: {usn}\r\n\r\n"
                    );
                    let _ = sock.send_to(reply.as_bytes(), from);
                }
            }
        }
    }
}

// ---- HTTP ----

const SERVER: &str = "UPnP/1.0 roku-remote";
const XML: &str = "text/xml; charset=\"utf-8\"";
const DLNA_FLAGS: &str = "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000";

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: String,
}

fn read_request(r: &mut BufReader<TcpStream>) -> io::Result<Option<Request>> {
    let mut line = String::new();
    if r.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let mut headers = HashMap::new();
    loop {
        let mut l = String::new();
        if r.read_line(&mut l)? == 0 || l.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let len: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body = vec![0; len.min(1 << 20)];
    r.read_exact(&mut body)?;
    Ok(Some(Request { method, path, headers, body: String::from_utf8_lossy(&body).into_owned() }))
}

fn respond(w: &mut impl Write, status: &str, ctype: &str, extra: &str, body: &[u8]) -> io::Result<()> {
    write!(w, "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nServer: {SERVER}\r\n{extra}\r\n", body.len())?;
    w.write_all(body)?;
    w.flush()
}

impl Server {
    fn handle(&self, stream: TcpStream) -> io::Result<()> {
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        let peer = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
        let mut w = stream.try_clone()?;
        let mut r = BufReader::new(stream);
        while !self.stopped()
            && let Some(req) = read_request(&mut r)?
        {
            self.seen.lock().unwrap().insert(peer.clone(), Instant::now());
            let path = req.path.split('?').next().unwrap_or_default().to_string();
            let action = req.headers.get("soapaction").cloned().unwrap_or_default();
            match (req.method.as_str(), path.as_str()) {
                ("GET", "/desc.xml") => respond(&mut w, "200 OK", XML, "", self.description().as_bytes())?,
                ("GET", "/cd/scpd.xml") => respond(&mut w, "200 OK", XML, "", CD_SCPD.as_bytes())?,
                ("GET", "/cm/scpd.xml") => respond(&mut w, "200 OK", XML, "", CM_SCPD.as_bytes())?,
                ("POST", "/cd/control") => respond(&mut w, "200 OK", XML, "", self.content_directory(&action, &req.body).as_bytes())?,
                ("POST", "/cm/control") => respond(&mut w, "200 OK", XML, "", connection_manager(&action).as_bytes())?,
                ("SUBSCRIBE", _) => respond(&mut w, "200 OK", "text/plain", &format!("SID: uuid:{}-sub\r\nTIMEOUT: Second-1800\r\n", self.uuid), b"")?,
                ("UNSUBSCRIBE", _) => respond(&mut w, "200 OK", "text/plain", "", b"")?,
                ("GET" | "HEAD", p) if p.starts_with("/media/") => self.serve_file(&mut w, &p[7..], &req)?,
                _ => respond(&mut w, "404 Not Found", "text/plain", "", b"not found")?,
            }
            if req.headers.get("connection").is_some_and(|c| c.eq_ignore_ascii_case("close")) {
                break;
            }
        }
        Ok(())
    }

    fn description(&self) -> String {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<root xmlns="urn:schemas-upnp-org:device-1-0" xmlns:dlna="urn:schemas-dlna-org:device-1-0">
<specVersion><major>1</major><minor>0</minor></specVersion>
<device>
<deviceType>{DEVICE}</deviceType>
<friendlyName>{}</friendlyName>
<manufacturer>Roku Remote</manufacturer>
<modelName>Roku Remote</modelName>
<modelNumber>{}</modelNumber>
<UDN>uuid:{}</UDN>
<dlna:X_DLNADOC>DMS-1.50</dlna:X_DLNADOC>
<serviceList>
<service><serviceType>{CD}</serviceType><serviceId>urn:upnp-org:serviceId:ContentDirectory</serviceId><SCPDURL>/cd/scpd.xml</SCPDURL><controlURL>/cd/control</controlURL><eventSubURL>/cd/event</eventSubURL></service>
<service><serviceType>{CM}</serviceType><serviceId>urn:upnp-org:serviceId:ConnectionManager</serviceId><SCPDURL>/cm/scpd.xml</SCPDURL><controlURL>/cm/control</controlURL><eventSubURL>/cm/event</eventSubURL></service>
</serviceList>
</device>
</root>"#,
            xml_escape(&self.name),
            env!("CARGO_PKG_VERSION"),
            self.uuid
        )
    }

    /// The folder, or a file in it, for an object ID: "0" is the root, other
    /// IDs are paths relative to it. None for anything outside the folder.
    fn resolve(&self, id: &str) -> Option<PathBuf> {
        if id == "0" {
            return Some(self.root.clone());
        }
        let rel = Path::new(id);
        if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            return None;
        }
        let path = fs::canonicalize(self.root.join(rel)).ok()?;
        path.starts_with(&self.root).then_some(path)
    }

    fn id_of(&self, path: &Path) -> String {
        match path.strip_prefix(&self.root) {
            // IDs and URLs use '/' on every OS.
            Ok(rel) if !rel.as_os_str().is_empty() => {
                rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
            }
            _ => "0".into(),
        }
    }

    fn content_directory(&self, action: &str, body: &str) -> String {
        let name = action.trim_matches('"').rsplit('#').next().unwrap_or_default();
        let out = match name {
            "Browse" => {
                let id = xml_unescape(tag(body, "ObjectID").unwrap_or("0"));
                let flag = tag(body, "BrowseFlag").unwrap_or("BrowseDirectChildren");
                let start: usize = tag(body, "StartingIndex").and_then(|v| v.parse().ok()).unwrap_or(0);
                let count: usize = tag(body, "RequestedCount").and_then(|v| v.parse().ok()).unwrap_or(0);
                let (didl, returned, total) = self.browse(&id, flag, start, count);
                format!(
                    "<Result>{}</Result><NumberReturned>{returned}</NumberReturned><TotalMatches>{total}</TotalMatches><UpdateID>1</UpdateID>",
                    xml_escape(&didl)
                )
            }
            "GetSystemUpdateID" => "<Id>1</Id>".into(),
            "GetSortCapabilities" => "<SortCaps></SortCaps>".into(),
            "GetSearchCapabilities" => "<SearchCaps></SearchCaps>".into(),
            _ => String::new(),
        };
        soap(CD, name, &out)
    }

    fn browse(&self, id: &str, flag: &str, start: usize, count: usize) -> (String, usize, usize) {
        let mut didl = String::from(
            r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/">"#,
        );
        let (returned, total) = match self.resolve(id) {
            None => (0, 0),
            Some(path) if flag == "BrowseMetadata" => {
                didl.push_str(&self.object(&path));
                (1, 1)
            }
            Some(path) => {
                let entries = self.children(&path);
                let page: Vec<_> = entries.iter().skip(start).take(if count == 0 { usize::MAX } else { count }).collect();
                for p in &page {
                    didl.push_str(&self.object(p));
                }
                (page.len(), entries.len())
            }
        };
        didl.push_str("</DIDL-Lite>");
        (didl, returned, total)
    }

    /// Subfolders first, then media files, by name; hidden files skipped.
    fn children(&self, dir: &Path) -> Vec<PathBuf> {
        let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
        let mut entries: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|e| fs::canonicalize(e.path()).ok())
            .filter(|p| p.starts_with(&self.root) && (p.is_dir() || mime(p).is_some()))
            .collect();
        entries.sort_by_key(|p| (!p.is_dir(), p.file_name().map(|n| n.to_string_lossy().to_lowercase())));
        entries
    }

    fn object(&self, path: &Path) -> String {
        let id = self.id_of(path);
        let parent = if id == "0" { "-1".to_string() } else { self.id_of(path.parent().unwrap_or(&self.root)) };
        let title = if id == "0" {
            self.name.clone()
        } else {
            path.file_stem().or(path.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        };
        let (eid, parent, title) = (xml_escape(&id), xml_escape(&parent), xml_escape(&title));
        if path.is_dir() {
            let n = self.children(path).len();
            return format!(
                r#"<container id="{eid}" parentID="{parent}" restricted="1" childCount="{n}"><dc:title>{title}</dc:title><upnp:class>object.container.storageFolder</upnp:class></container>"#
            );
        }
        let mime = mime(path).unwrap_or("application/octet-stream");
        let class = match mime.split('/').next() {
            Some("video") => "object.item.videoItem",
            Some("audio") => "object.item.audioItem.musicTrack",
            _ => "object.item.imageItem.photo",
        };
        let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let url = format!("{}/media/{}", self.base, percent_encode(&id));
        format!(
            r#"<item id="{eid}" parentID="{parent}" restricted="1"><dc:title>{title}</dc:title><upnp:class>{class}</upnp:class><res protocolInfo="http-get:*:{mime}:{DLNA_FLAGS}" size="{size}">{}</res></item>"#,
            xml_escape(&url)
        )
    }

    fn serve_file(&self, w: &mut impl Write, encoded: &str, req: &Request) -> io::Result<()> {
        let Some(path) = self.resolve(&percent_decode(encoded)).filter(|p| p.is_file()) else {
            return respond(w, "404 Not Found", "text/plain", "", b"not found");
        };
        let mime = mime(&path).unwrap_or("application/octet-stream");
        let mut file = fs::File::open(&path)?;
        let size = file.metadata()?.len();
        let range = req.headers.get("range").and_then(|r| parse_range(r, size));
        let (status, from, to) = match range {
            Some((a, b)) => ("206 Partial Content", a, b),
            None if req.headers.contains_key("range") && size > 0 => {
                return respond(w, "416 Range Not Satisfiable", "text/plain", &format!("Content-Range: bytes */{size}\r\n"), b"");
            }
            None => ("200 OK", 0, size.saturating_sub(1)),
        };
        let len = if size == 0 { 0 } else { to - from + 1 };
        let mut head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {len}\r\nAccept-Ranges: bytes\r\ntransferMode.dlna.org: Streaming\r\ncontentFeatures.dlna.org: {DLNA_FLAGS}\r\nServer: {SERVER}\r\n"
        );
        if range.is_some() {
            head.push_str(&format!("Content-Range: bytes {from}-{to}/{size}\r\n"));
        }
        head.push_str("\r\n");
        w.write_all(head.as_bytes())?;
        if req.method == "GET" {
            file.seek(SeekFrom::Start(from))?;
            io::copy(&mut file.take(len), w)?;
        }
        w.flush()
    }
}

fn connection_manager(action: &str) -> String {
    let name = action.trim_matches('"').rsplit('#').next().unwrap_or_default();
    let out = match name {
        "GetProtocolInfo" => {
            let sources: Vec<String> = MIMES.iter().map(|(_, m)| format!("http-get:*:{m}:*")).collect();
            format!("<Source>{}</Source><Sink></Sink>", sources.join(","))
        }
        "GetCurrentConnectionIDs" => "<ConnectionIDs>0</ConnectionIDs>".into(),
        _ => String::new(),
    };
    soap(CM, name, &out)
}

fn soap(service: &str, action: &str, inner: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action}Response xmlns:u="{service}">{inner}</u:{action}Response></s:Body></s:Envelope>"#
    )
}

/// The text of the first `<name>` element, ignoring namespace prefixes.
fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = xml;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        let end = rest.find('>')?;
        let open = &rest[..end];
        let local = open.split_whitespace().next()?.rsplit(':').next()?;
        if local == name && !open.starts_with('/') {
            if open.ends_with('/') {
                return Some("");
            }
            let body = &rest[end + 1..];
            return Some(&body[..body.find("</")?]);
        }
        rest = &rest[end..];
    }
    None
}

/// What Roku Media Player can play, by extension.
const MIMES: &[(&str, &str)] = &[
    ("mp4", "video/mp4"),
    ("m4v", "video/mp4"),
    ("mov", "video/quicktime"),
    ("mkv", "video/x-matroska"),
    ("ts", "video/mp2t"),
    ("mp3", "audio/mpeg"),
    ("m4a", "audio/mp4"),
    ("aac", "audio/aac"),
    ("flac", "audio/flac"),
    ("wav", "audio/wav"),
    ("ogg", "audio/ogg"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("png", "image/png"),
    ("gif", "image/gif"),
];

fn mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    MIMES.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

/// `bytes=a-b`, `bytes=a-` or `bytes=-n`, clamped to the file.
fn parse_range(h: &str, size: u64) -> Option<(u64, u64)> {
    let spec = h.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;
    if size == 0 {
        return None;
    }
    let (from, to) = if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        (size.saturating_sub(n), size - 1)
    } else {
        let from: u64 = a.parse().ok()?;
        let to = if b.is_empty() { size - 1 } else { b.parse::<u64>().ok()?.min(size - 1) };
        (from, to)
    };
    (from <= to && from < size).then_some((from, to))
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const CD_SCPD: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<scpd xmlns="urn:schemas-upnp-org:service-1-0"><specVersion><major>1</major><minor>0</minor></specVersion>
<actionList>
<action><name>Browse</name><argumentList>
<argument><name>ObjectID</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_ObjectID</relatedStateVariable></argument>
<argument><name>BrowseFlag</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_BrowseFlag</relatedStateVariable></argument>
<argument><name>Filter</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Filter</relatedStateVariable></argument>
<argument><name>StartingIndex</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Index</relatedStateVariable></argument>
<argument><name>RequestedCount</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_Count</relatedStateVariable></argument>
<argument><name>SortCriteria</name><direction>in</direction><relatedStateVariable>A_ARG_TYPE_SortCriteria</relatedStateVariable></argument>
<argument><name>Result</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_Result</relatedStateVariable></argument>
<argument><name>NumberReturned</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_Count</relatedStateVariable></argument>
<argument><name>TotalMatches</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_Count</relatedStateVariable></argument>
<argument><name>UpdateID</name><direction>out</direction><relatedStateVariable>A_ARG_TYPE_UpdateID</relatedStateVariable></argument>
</argumentList></action>
<action><name>GetSystemUpdateID</name><argumentList><argument><name>Id</name><direction>out</direction><relatedStateVariable>SystemUpdateID</relatedStateVariable></argument></argumentList></action>
<action><name>GetSortCapabilities</name><argumentList><argument><name>SortCaps</name><direction>out</direction><relatedStateVariable>SortCapabilities</relatedStateVariable></argument></argumentList></action>
<action><name>GetSearchCapabilities</name><argumentList><argument><name>SearchCaps</name><direction>out</direction><relatedStateVariable>SearchCapabilities</relatedStateVariable></argument></argumentList></action>
</actionList>
<serviceStateTable>
<stateVariable sendEvents="no"><name>A_ARG_TYPE_ObjectID</name><dataType>string</dataType></stateVariable>
<stateVariable sendEvents="no"><name>A_ARG_TYPE_BrowseFlag</name><dataType>string</dataType><allowedValueList><allowedValue>BrowseMetadata</allowedValue><allowedValue>BrowseDirectChildren</allowedValue></allowedValueList></stateVariable>
<stateVariable sendEvents="no"><name>A_ARG_TYPE_Filter</name><dataType>string</dataType></stateVariable>
<stateVariable sendEvents="no"><name>A_ARG_TYPE_Index</name><dataType>ui4</dataType></stateVariable>
<stateVariable sendEvents="no"><name>A_ARG_TYPE_Count</name><dataType>ui4</dataType></stateVariable>
<stateVariable sendEvents="no"><name>A_ARG_TYPE_SortCriteria</name><dataType>string</dataType></stateVariable>
<stateVariable sendEvents="no"><name>A_ARG_TYPE_Result</name><dataType>string</dataType></stateVariable>
<stateVariable sendEvents="no"><name>A_ARG_TYPE_UpdateID</name><dataType>ui4</dataType></stateVariable>
<stateVariable sendEvents="yes"><name>SystemUpdateID</name><dataType>ui4</dataType></stateVariable>
<stateVariable sendEvents="no"><name>SortCapabilities</name><dataType>string</dataType></stateVariable>
<stateVariable sendEvents="no"><name>SearchCapabilities</name><dataType>string</dataType></stateVariable>
</serviceStateTable></scpd>"#;

const CM_SCPD: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<scpd xmlns="urn:schemas-upnp-org:service-1-0"><specVersion><major>1</major><minor>0</minor></specVersion>
<actionList>
<action><name>GetProtocolInfo</name><argumentList>
<argument><name>Source</name><direction>out</direction><relatedStateVariable>SourceProtocolInfo</relatedStateVariable></argument>
<argument><name>Sink</name><direction>out</direction><relatedStateVariable>SinkProtocolInfo</relatedStateVariable></argument>
</argumentList></action>
<action><name>GetCurrentConnectionIDs</name><argumentList><argument><name>ConnectionIDs</name><direction>out</direction><relatedStateVariable>CurrentConnectionIDs</relatedStateVariable></argument></argumentList></action>
</actionList>
<serviceStateTable>
<stateVariable sendEvents="yes"><name>SourceProtocolInfo</name><dataType>string</dataType></stateVariable>
<stateVariable sendEvents="yes"><name>SinkProtocolInfo</name><dataType>string</dataType></stateVariable>
<stateVariable sendEvents="yes"><name>CurrentConnectionIDs</name><dataType>string</dataType></stateVariable>
</serviceStateTable></scpd>"#;
