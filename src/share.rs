use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use uuid::Uuid;

const MAX_BYTES: usize = 512 * 1024 * 1024;

pub struct Host {
    stop: Arc<AtomicBool>,
    pub url: String,
}

impl Host {
    pub fn start(bind: &str, token: &str, dir: &Path) -> Result<Self, String> {
        Self::start_public(bind, token, dir, None)
    }

    pub fn start_public(bind: &str, token: &str, dir: &Path, public: Option<&str>) -> Result<Self, String> {
        if token.trim().is_empty() {
            return Err("missing".into());
        }
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let listener = TcpListener::bind(bind).map_err(|e| e.to_string())?;
        listener.set_nonblocking(false).ok();
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let url = public_base(port, public);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let token = token.to_string();
        let dir = dir.to_path_buf();
        let public_base = url.clone();
        thread::spawn(move || serve_loop(listener, stop_thread, token, dir, public_base));
        Ok(Self { stop, url })
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub fn upload(base: &str, token: &str, file: &Path) -> Result<String, String> {
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() || token.trim().is_empty() {
        return Err("missing".into());
    }
    let base = if base.starts_with("http://") || base.starts_with("https://") {
        base.to_string()
    } else {
        format!("http://{base}")
    };
    let bytes = fs::read(file).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("too large".into());
    }
    let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("capture.bin");
    let (host, port) = host_port(&base)?;
    let mut stream = TcpStream::connect((host.as_str(), port)).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(30))).ok();
    let request = format!(
        "POST /v1/captures HTTP/1.1\r\nHost: {host}:{port}\r\nAuthorization: Bearer {token}\r\nX-Kapture-Filename: {name}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    stream.write_all(request.as_bytes()).map_err(|e| e.to_string())?;
    stream.write_all(&bytes).map_err(|e| e.to_string())?;
    let response = read_response(&mut stream)?;
    let (status, body) = split_response(&response);
    if status == 401 {
        return Err("denied".into());
    }
    if status != 201 {
        return Err(format!("status {status}"));
    }
    let url = body.lines().next().unwrap_or("").trim().to_string();
    if url.is_empty() {
        Err("empty".into())
    } else {
        Ok(url)
    }
}

pub fn serve_cli(args: &[String]) -> i32 {
    let mut bind = std::env::var("KAPTURE_BIND").unwrap_or_else(|_| "0.0.0.0:8741".into());
    let mut token = std::env::var("KAPTURE_TOKEN").unwrap_or_default();
    let mut dir = std::env::var("KAPTURE_DIR").map(PathBuf::from).unwrap_or_else(|_| default_inbox());
    let mut public = std::env::var("KAPTURE_PUBLIC_URL").ok();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--bind" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    bind = value.clone();
                }
            }
            "--token" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    token = value.clone();
                }
            }
            "--dir" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    dir = PathBuf::from(value);
                }
            }
            "--public" => {
                index += 1;
                if let Some(value) = args.get(index) {
                    public = Some(value.clone());
                }
            }
            _ => {}
        }
        index += 1;
    }
    if token.trim().is_empty() {
        eprintln!("kapture serve a besoin de --token");
        return 2;
    }
    match Host::start_public(&bind, &token, &dir, public.as_deref()) {
        Ok(host) => {
            println!("Kapture reçoit sur {}", host.url);
            while !host.stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_secs(3600));
            }
            0
        }
        Err(err) => {
            eprintln!("kapture serve : {err}");
            1
        }
    }
}

pub fn default_inbox() -> PathBuf {
    directories::ProjectDirs::from("studio", "Karom Studio", "kapture")
        .map(|dirs| dirs.data_dir().join("inbox"))
        .unwrap_or_else(|| std::env::temp_dir().join("kapture-inbox"))
}

fn serve_loop(listener: TcpListener, stop: Arc<AtomicBool>, token: String, dir: PathBuf, public_base: String) {
    listener.set_nonblocking(true).ok();
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let token = token.clone();
                let dir = dir.clone();
                let public_base = public_base.clone();
                thread::spawn(move || {
                    let _ = handle(stream, &token, &dir, &public_base);
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(40));
            }
            Err(_) => thread::sleep(Duration::from_millis(40)),
        }
    }
}

fn handle(mut stream: TcpStream, token: &str, dir: &Path, public_base: &str) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(20)))?;
    let (method, path, headers, body) = read_request(&mut stream)?;
    if method == "POST" && path == "/v1/captures" {
        if !authorized(&headers, token) {
            return write_text(&mut stream, 401, "denied");
        }
        let name = safe_name(headers.get("x-kapture-filename").map(String::as_str).unwrap_or("capture.bin"));
        let id = Uuid::new_v4().to_string();
        let folder = dir.join(&id);
        fs::create_dir_all(&folder)?;
        fs::write(folder.join(&name), &body)?;
        let url = format!("{public_base}/c/{id}/{name}");
        return write_text(&mut stream, 201, &url);
    }
    if method == "GET" && path == "/" {
        return write_html(&mut stream, &index_page(dir, public_base));
    }
    if method == "GET" {
        if let Some((id, name)) = capture_path(&path) {
            let file = dir.join(&id).join(&name);
            if file.is_file() {
                let bytes = fs::read(&file)?;
                return write_file(&mut stream, &name, &bytes);
            }
        }
    }
    write_text(&mut stream, 404, "introuvable")
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<(String, String, HashMap<String, String>, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let header_end = loop {
        if buf.len() > 1024 * 1024 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "header"));
        }
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof"));
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
    };
    let header = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut body = buf[header_end + 4..].to_vec();
    let mut lines = header.split("\r\n");
    let request = lines.next().unwrap_or("");
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((key, value)) = line.split_once(':') {
            headers.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let len = headers.get("content-length").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
    if len > MAX_BYTES {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "too large"));
    }
    while body.len() < len {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(len);
    Ok((method, path, headers, body))
}

fn read_response(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock || err.kind() == std::io::ErrorKind::TimedOut => {
                break;
            }
            Err(err) => return Err(err.to_string()),
        }
        if buf.len() > 1024 * 1024 {
            break;
        }
    }
    Ok(buf)
}

fn split_response(bytes: &[u8]) -> (u16, String) {
    let text = String::from_utf8_lossy(bytes);
    let status = text.split_whitespace().nth(1).and_then(|n| n.parse().ok()).unwrap_or(0);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("").trim().to_string();
    (status, body)
}

fn authorized(headers: &HashMap<String, String>, token: &str) -> bool {
    let Some(value) = headers.get("authorization") else { return false };
    let presented = value.strip_prefix("Bearer ").unwrap_or(value).trim();
    tokens_match(presented, token)
}

fn tokens_match(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() || left.is_empty() {
        return false;
    }
    left.iter().zip(right).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

fn safe_name(name: &str) -> String {
    if name.contains("..") {
        return "capture.bin".into();
    }
    let name = name.rsplit(['/', '\\']).next().unwrap_or("capture.bin");
    let cleaned: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() || cleaned.starts_with('.') {
        "capture.bin".into()
    } else {
        cleaned
    }
}

fn capture_path(path: &str) -> Option<(String, String)> {
    let rest = path.strip_prefix("/c/")?;
    let (id, name) = rest.split_once('/')?;
    if id.len() < 8 || id.contains('.') || id.contains("..") {
        return None;
    }
    let name = safe_name(name);
    Some((id.to_string(), name))
}

fn index_page(dir: &Path, public_base: &str) -> String {
    let mut items = String::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().to_string();
            let Ok(files) = fs::read_dir(entry.path()) else { continue };
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().to_string();
                items.push_str(&format!(
                    "<li><a href=\"/c/{id}/{name}\">{name}</a></li>\n"
                ));
                let _ = public_base;
            }
        }
    }
    format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Kapture</title>\
         <body style=\"font-family:sans-serif;background:#f5f4f1;color:#1c1b18;margin:40px\">\
         <h1>Kapture</h1><p>Captures reçues sur ce serveur.</p><ul>{items}</ul></body>"
    )
}

fn write_text(stream: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    let reason = reason(status);
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}\n",
        body.len() + 1
    )
}

fn write_html(stream: &mut TcpStream, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn write_file(stream: &mut TcpStream, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let kind = if name.ends_with(".png") {
        "image/png"
    } else if name.ends_with(".jpg") || name.ends_with(".jpeg") {
        "image/jpeg"
    } else if name.ends_with(".webp") {
        "image/webp"
    } else if name.ends_with(".mp4") {
        "video/mp4"
    } else {
        "application/octet-stream"
    };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    )?;
    stream.write_all(bytes)
}

fn reason(status: u16) -> &'static str {
    match status {
        201 => "Created",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Payload Too Large",
        _ => "OK",
    }
}

fn host_port(base: &str) -> Result<(String, u16), String> {
    let without = base.trim_start_matches("http://").trim_start_matches("https://");
    let hostport = without.split('/').next().unwrap_or(without);
    if let Some((host, port)) = hostport.rsplit_once(':') {
        let port = port.parse::<u16>().map_err(|_| "port".to_string())?;
        Ok((host.to_string(), port))
    } else {
        Ok((hostport.to_string(), 80))
    }
}

fn public_base(port: u16, public: Option<&str>) -> String {
    match public.map(str::trim).filter(|value| !value.is_empty()) {
        Some(base) => base.trim_end_matches('/').to_string(),
        None => format!("http://{}:{port}", lan_ip()),
    }
}

fn lan_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .ok()
        .and_then(|socket| socket.connect("8.8.8.8:80").ok().map(|_| socket))
        .and_then(|socket| socket.local_addr().ok())
        .map(|addr| addr.ip().to_string())
        .filter(|ip| ip != "0.0.0.0")
        .unwrap_or_else(|| "127.0.0.1".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_url_replaces_the_lan_address() {
        assert_eq!(
            public_base(8741, Some("https://captures.example/")),
            "https://captures.example"
        );
        assert!(public_base(8741, None).ends_with(":8741"));
    }

    #[test]
    fn upload_and_fetch_roundtrip() {
        let dir = std::env::temp_dir().join(format!("kapture-share-{}", Uuid::new_v4()));
        let host = Host::start("127.0.0.1:0", "secret", &dir).unwrap();
        let port = host.url.rsplit(':').next().unwrap().parse::<u16>().unwrap();
        let source = dir.join("out.png");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&source, b"png-bytes").unwrap();
        let base = format!("http://127.0.0.1:{port}");
        let url = upload(&base, "secret", &source).unwrap();
        assert!(url.contains("/c/"));
        assert!(upload(&base, "wrong", &source).unwrap_err().contains("denied"));
        let path = url.splitn(4, '/').nth(3).unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "GET /{path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").unwrap();
        let response = read_response(&mut stream).unwrap();
        let text = String::from_utf8_lossy(&response);
        assert!(text.contains("png-bytes"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_stay_inside_the_inbox() {
        assert_eq!(safe_name("../secret.png"), "capture.bin");
        assert_eq!(safe_name("a b.png"), "a_b.png");
        assert!(capture_path("/c/abcdef12/a.png").is_some());
        assert!(capture_path("/c/../a.png").is_none());
    }
}
