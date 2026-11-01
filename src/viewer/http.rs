use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

const MAX_REQUEST: usize = 64 * 1024;

/// A loopback listener with bounded requests and explicit host/origin checks.
/// `serve_one` lets an embedding host own cancellation and process lifecycle.
pub struct ViewerServer {
    listener: TcpListener,
    address: SocketAddr,
    read_only: bool,
}

impl ViewerServer {
    pub fn bind(port: u16, read_only: bool) -> Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
        let address = listener.local_addr()?;
        Ok(Self {
            listener,
            address,
            read_only,
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Set nonblocking mode for a host-managed event loop. `serve_one` returns
    /// false when no client is waiting; no background runtime is required.
    pub fn set_nonblocking(&self, nonblocking: bool) -> Result<()> {
        self.listener.set_nonblocking(nonblocking)?;
        Ok(())
    }

    pub fn serve_one(&self, engine: &mut Engine) -> Result<bool> {
        let (mut stream, peer) = match self.listener.accept() {
            Ok(client) => client,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.set_write_timeout(Some(Duration::from_secs(3)))?;
        if !peer.ip().is_loopback() {
            return Ok(true);
        }
        match read_request(&stream) {
            Ok(request) => {
                let head = request.method == "HEAD";
                match self.dispatch(engine, &request) {
                    Ok((mime, body)) => respond(&mut stream, 200, mime, &body, head)?,
                    Err(error) => {
                        let message = error.to_string();
                        let status = if message.starts_with("refused:") {
                            403
                        } else if message.starts_with("not found:") {
                            404
                        } else {
                            400
                        };
                        respond(
                            &mut stream,
                            status,
                            "application/json",
                            &serde_json::to_vec(&json!({"error":message}))?,
                            head,
                        )?;
                    }
                }
            }
            Err(error) => respond(
                &mut stream,
                400,
                "application/json",
                &serde_json::to_vec(&json!({"error":error.to_string()}))?,
                false,
            )?,
        }
        Ok(true)
    }

    fn dispatch(&self, engine: &mut Engine, request: &Request) -> Result<(&'static str, Vec<u8>)> {
        let expected = self.address.to_string();
        let local = format!("localhost:{}", self.address.port());
        let host = request
            .headers
            .get("host")
            .context("refused: Host is required")?;
        ensure!(
            host == &expected || host == &local,
            "refused: Host must name this loopback listener"
        );
        if let Some(origin) = request.headers.get("origin") {
            ensure!(
                origin == &format!("http://{host}"),
                "refused: cross-origin request"
            );
        }
        if let Some(site) = request.headers.get("sec-fetch-site") {
            ensure!(
                matches!(site.as_str(), "same-origin" | "none"),
                "refused: cross-origin request"
            );
        }
        ensure!(
            matches!(request.method.as_str(), "GET" | "HEAD" | "POST" | "DELETE"),
            "unsupported HTTP method"
        );
        let (path, _) = parse_target(&request.target)?;
        if matches!(request.method.as_str(), "POST" | "DELETE") {
            ensure!(!self.read_only, "refused: viewer is read-only");
            ensure!(
                request.headers.get("x-viewer-write").map(String::as_str) == Some("1"),
                "refused: X-Viewer-Write is required"
            );
            ensure!(
                request
                    .headers
                    .get("content-type")
                    .is_some_and(|value| value.split(';').next() == Some("application/json")),
                "refused: Content-Type must be application/json"
            );
            ensure!(
                path == "/api/trails" || path.starts_with("/api/trails/"),
                "refused: only trails may be changed"
            );
            let value = trail_write(engine, &request.method, &path, &request.body)?;
            return Ok(("application/json", serde_json::to_vec(&value)?));
        }
        let asset = match path.as_str() {
            "/" | "/index.html" => Some((
                "text/html; charset=utf-8",
                include_bytes!("index.html").as_slice(),
            )),
            "/viewer.js" => Some((
                "text/javascript; charset=utf-8",
                include_bytes!("viewer.js").as_slice(),
            )),
            "/viewer.css" => Some((
                "text/css; charset=utf-8",
                include_bytes!("viewer.css").as_slice(),
            )),
            _ => None,
        };
        if let Some((mime, bytes)) = asset {
            return Ok((mime, bytes.to_vec()));
        }
        ensure!(path.starts_with("/api"), "not found: viewer asset");
        if path == "/api/events" {
            let generation = engine.status()?.generation;
            return Ok((
                "text/event-stream",
                format!("retry: 2000\nevent: index\ndata: {{\"generation\":{generation}}}\n\n")
                    .into_bytes(),
            ));
        }
        engine.refresh()?;
        let value = if path == "/api/trails" {
            trail_list(engine, self.read_only)?
        } else {
            engine.viewer_query(&request.target)?
        };
        let body = serde_json::to_vec(&value)?;
        Ok(("application/json", body))
    }
}

/// Run the viewer until the host terminates the process. The library's
/// `ViewerServer` exposes a cancellable, host-driven alternative.
pub fn serve(engine: &mut Engine, port: u16) -> Result<()> {
    serve_with_options(engine, port, false)
}

pub fn serve_with_options(engine: &mut Engine, port: u16, read_only: bool) -> Result<()> {
    let server = ViewerServer::bind(port, read_only)?;
    engine.refresh()?;
    let _watch = engine
        .watch()
        .inspect_err(|error| {
            eprintln!("Viewer watcher unavailable; requests still refresh the index: {error}");
        })
        .ok();
    eprintln!("Viewer: http://{}", server.address());
    loop {
        if let Err(error) = server.serve_one(engine) {
            eprintln!("Viewer connection: {error}");
        }
    }
}

struct Request {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}
fn read_request(stream: &TcpStream) -> Result<Request> {
    let mut reader = BufReader::new(stream);
    let mut header = vec![];
    loop {
        let mut line = vec![];
        let bytes = (&mut reader)
            .take((MAX_REQUEST - header.len() + 1) as u64)
            .read_until(b'\n', &mut line)?;
        ensure!(
            bytes > 0 && header.len() + bytes <= MAX_REQUEST,
            "request headers exceed 64 KiB or are incomplete"
        );
        let done = line == b"\r\n";
        header.extend(line);
        if done {
            break;
        }
    }
    let header = String::from_utf8(header)?;
    let mut lines = header.split("\r\n");
    let words = lines
        .next()
        .context("missing request line")?
        .split_whitespace()
        .collect::<Vec<_>>();
    ensure!(
        words.len() == 3 && matches!(words[2], "HTTP/1.1" | "HTTP/1.0"),
        "invalid HTTP request line"
    );
    let mut headers = BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (key, value) = line.split_once(':').context("invalid HTTP header")?;
        ensure!(
            key.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
            "invalid HTTP header name"
        );
        ensure!(
            headers
                .insert(key.to_ascii_lowercase(), value.trim().to_owned())
                .is_none(),
            "duplicate HTTP header"
        );
    }
    ensure!(
        !headers.contains_key("transfer-encoding"),
        "transfer encoding is unsupported"
    );
    let length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(0);
    ensure!(length <= MAX_REQUEST, "request body exceeds 64 KiB");
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Request {
        method: words[0].into(),
        target: words[1].into(),
        headers,
        body,
    })
}
fn respond(stream: &mut TcpStream, status: u16, mime: &str, body: &[u8], head: bool) -> Result<()> {
    let reason = match status {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Bad Request",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'\r\n\r\n",
        body.len()
    )?;
    if !head {
        stream.write_all(body)?;
    }
    stream.flush()?;
    Ok(())
}

fn trail_database(engine: &Engine, create: bool) -> Result<Option<rusqlite::Connection>> {
    let path = engine.database.with_extension("viewer.sqlite");
    if !path.exists() && !create {
        return Ok(None);
    }
    ensure!(!path.is_symlink(), "refused: trail database is a symlink");
    if !create {
        let connection = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let owner: String =
            connection.query_row("SELECT root FROM ownership", [], |row| row.get(0))?;
        ensure!(
            owner == engine.root.to_string_lossy(),
            "refused: trail database belongs to another project"
        );
        return Ok(Some(connection));
    }

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let connection = rusqlite::Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(3))?;
    connection.execute_batch("CREATE TABLE IF NOT EXISTS ownership(root TEXT NOT NULL); CREATE TABLE IF NOT EXISTS trails(id TEXT PRIMARY KEY, body TEXT NOT NULL)")?;
    let root = engine.root.to_string_lossy();
    connection.execute(
        "INSERT INTO ownership(root) SELECT ?1 WHERE NOT EXISTS(SELECT 1 FROM ownership)",
        [root.as_ref()],
    )?;
    let owner: String = connection.query_row("SELECT root FROM ownership", [], |row| row.get(0))?;
    ensure!(
        owner == root,
        "refused: trail database belongs to another project"
    );
    Ok(Some(connection))
}
fn trail_list(engine: &Engine, read_only: bool) -> Result<Value> {
    let mut stored = vec![];
    if let Some(connection) = trail_database(engine, false)? {
        let mut statement = connection
            .prepare("SELECT body FROM trails WHERE length(body)<=65536 ORDER BY id LIMIT 200")?;
        for row in statement.query_map([], |row| row.get::<_, String>(0))? {
            stored.push(serde_json::from_str::<Value>(&row?)?);
        }
    }
    with_graph(engine, |graph, _| {
        let mut identities = std::collections::HashMap::<(&str, &str, &str), Vec<usize>>::new();
        let mut ids = std::collections::HashMap::new();
        for (index, node) in graph.nodes.iter().enumerate() {
            identities
                .entry((&node.path, &node.symbol.qualified_name, &node.symbol.kind))
                .or_default()
                .push(index);
            ids.insert(node_id(graph, index), index);
        }
        let mut trails = vec![];
        for mut trail in stored {
            if let Some(hops) = trail["hops"].as_array_mut() {
                for hop in hops {
                    let key = (
                        hop["file"].as_str().unwrap_or(""),
                        hop["qualifiedName"].as_str().unwrap_or(""),
                        hop["kind"].as_str().unwrap_or(""),
                    );
                    let candidates = identities.get(&key).cloned().unwrap_or_default();
                    let exact = hop["id"]
                        .as_str()
                        .and_then(|id| ids.get(id))
                        .copied()
                        .filter(|index| candidates.contains(index));
                    let resolved = exact.or_else(|| (candidates.len() == 1).then(|| candidates[0]));
                    hop["status"] = json!(if resolved.is_some() {
                        "resolved"
                    } else if candidates.is_empty() {
                        "missing"
                    } else {
                        "ambiguous"
                    });
                    hop["node"] = resolved
                        .map(|index| node_ref(graph, index))
                        .unwrap_or(Value::Null);
                }
            }
            trails.push(trail);
        }
        Ok(
            json!({"trails":trails,"readOnly":read_only,"limit":200,"truncated":false,"generation":graph.generation}),
        )
    })
}

fn trail_write(engine: &Engine, method: &str, path: &str, body: &[u8]) -> Result<Value> {
    let mut connection = trail_database(engine, true)?.context("trail database unavailable")?;
    if method == "DELETE" {
        let id = path
            .strip_prefix("/api/trails/")
            .context("trail ID is required")?;
        ensure!(
            id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid trail ID"
        );
        let removed = connection.execute("DELETE FROM trails WHERE id=?1", [id])?;
        return Ok(json!({"removed":removed>0}));
    }
    ensure!(path == "/api/trails", "invalid trail endpoint");
    let request: Value = serde_json::from_slice(body)?;
    let name = request["name"]
        .as_str()
        .filter(|name| !name.trim().is_empty() && name.chars().count() <= 120)
        .context("name must contain 1 to 120 characters")?;
    let note = request["note"].as_str().unwrap_or("");
    ensure!(note.chars().count() <= 600, "note exceeds 600 characters");
    let ids = request["hops"]
        .as_array()
        .filter(|hops| !hops.is_empty() && hops.len() <= 64)
        .context("hops must contain 1 to 64 node IDs")?;
    let hops = with_graph(engine, |graph, _| {
        ids.iter().map(|id| {let id=id.as_str().context("hop must be a node ID")?;let index=lookup(graph,id)?;let node=&graph.nodes[index];Ok(json!({"file":node.path,"qualifiedName":node.symbol.qualified_name,"kind":node.symbol.kind,"line":node.symbol.start_line,"id":node_id(graph,index)}))}).collect::<Result<Vec<_>>>()
    })?;
    let id = blake3::hash(name.as_bytes()).to_hex().to_string();
    let trail = json!({"id":id,"name":name,"note":note,"hops":hops});
    let encoded = serde_json::to_string(&trail)?;
    ensure!(
        encoded.len() <= MAX_REQUEST,
        "resolved trail exceeds 64 KiB"
    );
    let transaction = connection.transaction()?;
    let total: i64 =
        transaction.query_row("SELECT count(*) FROM trails WHERE id!=?1", [&id], |row| {
            row.get(0)
        })?;
    ensure!(total < 200, "trail limit reached");
    transaction.execute("INSERT INTO trails(id,body) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body",rusqlite::params![id,serde_json::to_string(&trail)?])?;
    transaction.commit()?;
    Ok(trail)
}
