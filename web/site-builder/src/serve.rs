//! Minimal local static server for inspecting the browser engine.
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::Path,
};
pub fn serve(directory: &Path) -> std::io::Result<()> {
    let root = directory.canonicalize()?;
    let listener = TcpListener::bind("127.0.0.1:3030")?;
    println!("Serving {} at http://127.0.0.1:3030", root.display());
    for stream in listener.incoming() {
        let mut stream = stream?;
        let mut request = [0; 8192];
        let n = stream.read(&mut request)?;
        let path = std::str::from_utf8(&request[..n])
            .unwrap_or("")
            .split_whitespace()
            .nth(1)
            .unwrap_or("/")
            .split('?')
            .next()
            .unwrap_or("/");
        let path = if path == "/" {
            "index.html"
        } else {
            path.trim_start_matches('/')
        };
        let file = root.join(path).canonicalize();
        let (status, body, mime) = match file {
            Ok(file) if file.starts_with(&root) && file.is_file() => {
                let mime = match file.extension().and_then(|v| v.to_str()) {
                    Some("html") => "text/html",
                    Some("js") => "text/javascript",
                    Some("css") => "text/css",
                    Some("wasm") => "application/wasm",
                    Some("webp") => "image/webp",
                    Some("png") => "image/png",
                    _ => "application/octet-stream",
                };
                ("200 OK", std::fs::read(file)?, mime)
            }
            _ => ("404 Not Found", b"Not found".to_vec(), "text/plain"),
        };
        write!(stream,"HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",body.len())?;
        let _ = stream.write_all(&body);
    }
    Ok(())
}
