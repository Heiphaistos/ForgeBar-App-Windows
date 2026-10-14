//! Mini serveur HTTP local : analyse des requêtes, contrôles de sécurité et routage.
//! `route` ne touche à aucune socket : il prend une `Request` et rend une `Response`.

use crate::api::{self, Ctx};
use crate::previews;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

pub const MAX_BODY: usize = 256 * 1024;
const IDLE_LIMIT_SECS: u64 = 30 * 60;

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    /// Noms en minuscules.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

pub struct Response {
    pub status: u16,
    pub ctype: &'static str,
    pub body: Vec<u8>,
    pub csp: Option<String>,
}

impl Response {
    pub fn json(status: u16, v: &serde_json::Value) -> Self {
        Self { status, ctype: "application/json; charset=utf-8", body: v.to_string().into_bytes(), csp: None }
    }
    pub fn err(status: u16, msg: &str) -> Self {
        Self::json(status, &serde_json::json!({ "ok": false, "error": msg }))
    }
    pub fn ok() -> Self {
        Self::json(200, &serde_json::json!({ "ok": true }))
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect()
}

fn read_line(r: &mut impl BufRead) -> Result<String, Response> {
    let mut s = String::new();
    let n = r.by_ref().take(8192).read_line(&mut s).map_err(|_| Response::err(408, "lecture impossible"))?;
    if n == 0 || !s.ends_with('\n') {
        return Err(Response::err(400, "requête incomplète ou ligne trop longue"));
    }
    Ok(s.trim_end().to_string())
}

pub fn parse_request(r: &mut impl BufRead) -> Result<Request, Response> {
    let line = read_line(r)?;
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(ver), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return Err(Response::err(400, "ligne de requête invalide"));
    };
    if !ver.starts_with("HTTP/1.") || !target.starts_with('/') {
        return Err(Response::err(400, "requête invalide"));
    }
    let (path, q) = target.split_once('?').unwrap_or((target, ""));
    let mut headers = HashMap::new();
    loop {
        let h = read_line(r)?;
        if h.is_empty() {
            break;
        }
        if headers.len() >= 64 {
            return Err(Response::err(431, "trop d'en-têtes"));
        }
        let Some((k, v)) = h.split_once(':') else { return Err(Response::err(400, "en-tête invalide")) };
        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
    }
    if headers.contains_key("transfer-encoding") {
        return Err(Response::err(501, "Transfer-Encoding non pris en charge"));
    }
    let len: usize = match headers.get("content-length") {
        None => 0,
        Some(v) => v.parse().map_err(|_| Response::err(400, "Content-Length invalide"))?,
    };
    if len > MAX_BODY {
        return Err(Response::err(413, "corps trop volumineux (256 Ko maximum)"));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).map_err(|_| Response::err(400, "corps incomplet"))?;
    Ok(Request { method: method.to_string(), path: path.to_string(), query: parse_query(q), headers, body })
}

/// Comparaison en temps constant (le jeton ne fuit pas par le temps de réponse).
pub fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn page(ctx: &Ctx) -> Response {
    let html = include_str!("../ui/index.html")
        .replace("<!--CSS-->", include_str!("../ui/app.css"))
        .replace(
            "<!--JS-->",
            concat!(
                include_str!("../ui/app.js"),
                "
",
                include_str!("../ui/pages.js")
            ),
        )
        .replace("{{NONCE}}", &ctx.nonce);
    let csp = format!(
        "default-src 'none'; script-src 'nonce-{n}'; style-src 'nonce-{n}'; img-src 'self' blob: data:; \
         connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
        n = ctx.nonce
    );
    Response { status: 200, ctype: "text/html; charset=utf-8", body: html.into_bytes(), csp: Some(csp) }
}

pub fn route(ctx: &Ctx, req: &Request) -> Response {
    // Anti DNS rebinding : seul l'hôte exact est servi.
    if req.headers.get("host").map(String::as_str) != Some(format!("127.0.0.1:{}", ctx.port).as_str()) {
        return Response::err(403, "hôte refusé");
    }
    ctx.touch();
    let given = if req.path == "/" { req.query.get("t") } else { req.headers.get("x-forge-token") };
    if !given.is_some_and(|t| ct_eq(t, &ctx.token)) {
        return Response::err(401, "jeton manquant ou invalide");
    }
    if req.method == "POST" {
        let json = req
            .headers
            .get("content-type")
            .is_some_and(|c| c.split(';').next().is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json")));
        if !json {
            return Response::err(415, "Content-Type application/json requis");
        }
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => page(ctx),
        ("GET", "/api/state") => api::state(ctx),
        ("POST", "/api/config") => api::post_config(ctx, &req.body),
        ("POST", "/api/condition") => api::post_condition(&req.body),
        ("POST", "/api/theme") => api::post_theme(ctx, &req.body),
        ("POST", "/api/theme/delete") => api::post_theme_delete(ctx, &req.body),
        ("POST", "/api/admin-key") => api::post_admin_key(&req.body),
        ("POST", "/api/admin-key/clear") => api::post_admin_key_clear(),
        ("POST", "/api/preview") => previews::draft(ctx, &req.body),
        ("GET", "/preview.png") => previews::get(ctx, &req.query),
        ("POST", "/api/quit") => {
            ctx.quit.store(true, Ordering::SeqCst);
            Response::ok()
        }
        _ => Response::err(404, "route inconnue"),
    }
}

fn reason(s: u16) -> &'static str {
    match s {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        _ => "Error",
    }
}

fn write_response(s: &mut TcpStream, r: &Response) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\
         X-Frame-Options: DENY\r\nCross-Origin-Resource-Policy: same-origin\r\n",
        r.status,
        reason(r.status),
        r.ctype,
        r.body.len()
    );
    if let Some(c) = &r.csp {
        head.push_str(&format!("Content-Security-Policy: {c}\r\n"));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes())?;
    s.write_all(&r.body)?;
    s.flush()
}

fn handle(ctx: &Ctx, mut stream: TcpStream) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let Ok(rd) = stream.try_clone() else { return };
    let (resp, rejected) = match parse_request(&mut BufReader::new(rd)) {
        Ok(req) => (route(ctx, &req), false),
        Err(r) => (r, true),
    };
    let _ = write_response(&mut stream, &resp);
    if rejected {
        // Évacue un corps refusé (413) : fermer avec des données non lues enverrait un RST qui efface la réponse.
        let _ = stream.shutdown(std::net::Shutdown::Write);
        let _ = std::io::copy(&mut (&stream).take(2_000_000), &mut std::io::sink());
    }
}

/// Boucle d'acceptation, un thread par connexion. S'arrête sur `/api/quit` ou après 30 min d'inactivité.
pub fn serve(listener: TcpListener, ctx: Arc<Ctx>) -> std::io::Result<()> {
    listener.set_nonblocking(true)?;
    while !ctx.quit.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let c = Arc::clone(&ctx);
                std::thread::spawn(move || handle(&c, stream));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if ctx.idle_secs() > IDLE_LIMIT_SECS {
                    break;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(40)),
        }
    }
    // Laisse partir la réponse de /api/quit.
    std::thread::sleep(Duration::from_millis(150));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test_ctx;

    fn req(method: &str, path: &str, hdrs: &[(&str, &str)], body: &str) -> Request {
        let (p, q) = path.split_once('?').unwrap_or((path, ""));
        Request {
            method: method.into(),
            path: p.into(),
            query: parse_query(q),
            headers: hdrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            body: body.as_bytes().to_vec(),
        }
    }

    fn good(ctx: &Ctx) -> Vec<(&'static str, String)> {
        vec![("host", format!("127.0.0.1:{}", ctx.port)), ("x-forge-token", ctx.token.clone())]
    }

    fn call(ctx: &Ctx, m: &str, p: &str, extra: &[(&str, &str)], body: &str) -> Response {
        let mut h: Vec<(String, String)> = good(ctx).into_iter().map(|(k, v)| (k.into(), v)).collect();
        h.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        let hs: Vec<(&str, &str)> = h.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        route(ctx, &req(m, p, &hs, body))
    }

    const JSON: (&str, &str) = ("content-type", "application/json");

    #[test]
    fn parses_http() {
        let raw = "POST /api/x?a=1&b=%C3%A9 HTTP/1.1\r\nHost: h\r\nContent-Length: 2\r\n\r\n{}";
        let r = parse_request(&mut raw.as_bytes()).ok().unwrap();
        assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/api/x"));
        assert_eq!(r.query["b"], "é");
        assert_eq!(r.body, b"{}");
        assert!(parse_request(&mut "GARBAGE\r\n\r\n".as_bytes()).is_err());
        let big = format!("POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1);
        assert_eq!(parse_request(&mut big.as_bytes()).err().unwrap().status, 413);
        let chunked = "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(parse_request(&mut chunked.as_bytes()).err().unwrap().status, 501);
    }

    #[test]
    fn rejects_bad_host_and_token() {
        let ctx = test_ctx();
        let bad_host = req("GET", "/api/state", &[("host", "evil.com"), ("x-forge-token", &ctx.token)], "");
        assert_eq!(route(&ctx, &bad_host).status, 403);
        let rebind = format!("localhost:{}", ctx.port);
        let r = req("GET", "/api/state", &[("host", &rebind), ("x-forge-token", &ctx.token)], "");
        assert_eq!(route(&ctx, &r).status, 403);
        let host = format!("127.0.0.1:{}", ctx.port);
        assert_eq!(route(&ctx, &req("GET", "/api/state", &[("host", &host)], "")).status, 401);
        assert_eq!(
            route(&ctx, &req("GET", "/api/state", &[("host", &host), ("x-forge-token", "nope")], "")).status,
            401
        );
        assert_eq!(route(&ctx, &req("GET", "/", &[("host", &host)], "")).status, 401);
        let page = format!("/?t={}", ctx.token);
        assert_eq!(route(&ctx, &req("GET", &page, &[("host", &host)], "")).status, 200);
        // Le paramètre t ne vaut que pour la page.
        let api = format!("/api/state?t={}", ctx.token);
        assert_eq!(route(&ctx, &req("GET", &api, &[("host", &host)], "")).status, 401);
    }

    #[test]
    fn post_needs_json_content_type() {
        let ctx = test_ctx();
        assert_eq!(call(&ctx, "POST", "/api/condition", &[], r#"{"text":"cpu>5"}"#).status, 415);
        assert_eq!(call(&ctx, "POST", "/api/condition", &[("content-type", "text/plain")], "{}").status, 415);
        assert_eq!(call(&ctx, "POST", "/api/condition", &[JSON], r#"{"text":"cpu>5"}"#).status, 200);
    }

    #[test]
    fn condition_validation_via_api() {
        let ctx = test_ctx();
        let ok = call(&ctx, "POST", "/api/condition", &[JSON], r#"{"text":"battery<20"}"#);
        assert_eq!(ok.status, 200);
        let bad = call(&ctx, "POST", "/api/condition", &[JSON], r#"{"text":"foo<3"}"#);
        assert_eq!(bad.status, 400);
        assert!(String::from_utf8_lossy(&bad.body).contains("métrique inconnue"));
    }

    #[test]
    fn path_traversal_refused() {
        let ctx = test_ctx();
        for name in ["../evil", "..\\evil", "a/b", "UPPER", "", "x.toml", &"a".repeat(41)] {
            let body = serde_json::json!({ "id": name, "extends": "nord", "overrides": {} }).to_string();
            assert_eq!(call(&ctx, "POST", "/api/theme", &[JSON], &body).status, 400, "{name}");
            let del = serde_json::json!({ "id": name }).to_string();
            assert_eq!(call(&ctx, "POST", "/api/theme/delete", &[JSON], &del).status, 400, "{name}");
        }
        assert_eq!(call(&ctx, "GET", "/preview.png?theme=..%2F..%2Fx", &[], "").status, 400);
        assert!(!ctx.dir.join("evil.toml").exists());
    }

    #[test]
    fn quit_route_sets_flag() {
        let ctx = test_ctx();
        assert_eq!(call(&ctx, "POST", "/api/quit", &[JSON], "{}").status, 200);
        assert!(ctx.quit.load(Ordering::SeqCst));
    }
}
