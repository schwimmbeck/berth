//! Command handler for `berth registry-api`.

use colored::Colorize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process;
use std::time::Duration;

use berth_registry::Registry;

const MAX_REQUEST_BYTES: usize = 16 * 1024;

/// Executes the `berth registry-api` command.
pub fn execute(bind: &str, max_requests: Option<u32>) {
    let listener = match TcpListener::bind(bind) {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!(
                "{} Failed to bind registry API at {}: {}",
                "✗".red().bold(),
                bind.cyan(),
                e
            );
            process::exit(1);
        }
    };
    let local_addr = match listener.local_addr() {
        Ok(addr) => addr,
        Err(e) => {
            eprintln!("{} Failed to read bound address: {}", "✗".red().bold(), e);
            process::exit(1);
        }
    };

    println!("Registry API listening on http://{local_addr}");
    let _ = io::stdout().flush();

    let registry = Registry::from_seed();
    let mut handled: u32 = 0;
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!(
                    "{} Failed to accept API connection: {}",
                    "!".yellow().bold(),
                    e
                );
                continue;
            }
        };
        if let Err(e) = handle_connection(&mut stream, &registry) {
            eprintln!(
                "{} Failed handling API connection: {}",
                "!".yellow().bold(),
                e
            );
        }

        handled = handled.saturating_add(1);
        if max_requests.is_some_and(|limit| handled >= limit) {
            break;
        }
    }
}

/// Handles one HTTP connection and writes a JSON response.
fn handle_connection(stream: &mut TcpStream, registry: &Registry) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let request = read_request(stream)?;
    let request_line = request.lines().next().unwrap_or_default().trim();
    let (status, body) = route_request(request_line, registry);
    write_json_response(stream, status, &body)
}

/// Reads an HTTP request header block from a client stream.
fn read_request(stream: &mut TcpStream) -> io::Result<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 512];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() >= 4 && buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() >= MAX_REQUEST_BYTES {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).to_string())
}

/// Routes a request line to a status code and JSON response body.
fn route_request(request_line: &str, registry: &Registry) -> (u16, Value) {
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if method.is_empty() || target.is_empty() {
        return (
            400,
            json!({
                "error": "malformed request line"
            }),
        );
    }
    if method != "GET" {
        return (
            405,
            json!({
                "error": "method not allowed"
            }),
        );
    }

    let (path, query) = split_path_query(target);
    match path {
        "/" | "/health" => (
            200,
            json!({
                "status": "ok"
            }),
        ),
        "/servers/filters" => route_server_filters(registry),
        "/servers" => {
            let query_value = query_param(query, "q")
                .or_else(|| query_param(query, "query"))
                .unwrap_or_default();
            let category_filter = query_param(query, "category").filter(|v| !v.trim().is_empty());
            let platform_filter = query_param(query, "platform").filter(|v| !v.trim().is_empty());
            let trust_filter = query_param(query, "trustLevel")
                .or_else(|| query_param(query, "trust_level"))
                .filter(|v| !v.trim().is_empty());
            let offset = parse_usize_param(query, "offset").unwrap_or(0);
            let limit = parse_usize_param(query, "limit");

            let matches_filter = |server: &&berth_registry::types::ServerMetadata| {
                matches_server_filters(server, category_filter, platform_filter, trust_filter)
            };

            let entries: Vec<(&berth_registry::types::ServerMetadata, Option<u32>)> =
                if query_value.trim().is_empty() {
                    registry
                        .list_all()
                        .iter()
                        .filter(matches_filter)
                        .map(|server| (server, None))
                        .collect()
                } else {
                    registry
                        .search(query_value)
                        .into_iter()
                        .map(|result| (result.server, Some(result.score)))
                        .filter(|(server, _)| {
                            matches_server_filters(
                                server,
                                category_filter,
                                platform_filter,
                                trust_filter,
                            )
                        })
                        .collect()
                };
            let total = entries.len();
            let sliced = if let Some(limit) = limit {
                entries
                    .into_iter()
                    .skip(offset)
                    .take(limit)
                    .collect::<Vec<_>>()
            } else {
                entries.into_iter().skip(offset).collect::<Vec<_>>()
            };
            let servers = sliced
                .into_iter()
                .map(|(server, score)| {
                    let mut summary = server_summary(server);
                    if let Some(score) = score {
                        if let Some(obj) = summary.as_object_mut() {
                            obj.insert("score".to_string(), json!(score));
                        }
                    }
                    summary
                })
                .collect::<Vec<Value>>();
            let count = servers.len();
            (
                200,
                json!({
                    "query": query_value,
                    "filters": {
                        "category": category_filter,
                        "platform": platform_filter,
                        "trustLevel": trust_filter
                    },
                    "total": total,
                    "count": count,
                    "offset": offset,
                    "limit": limit,
                    "servers": servers
                }),
            )
        }
        _ => route_server_detail(path, registry),
    }
}

/// Routes `/servers/filters` path.
fn route_server_filters(registry: &Registry) -> (u16, Value) {
    let mut categories = BTreeSet::new();
    let mut platforms = BTreeSet::new();
    let mut trust_levels = BTreeSet::new();

    for server in registry.list_all() {
        categories.insert(server.category.clone());
        for platform in &server.compatibility.platforms {
            platforms.insert(platform.clone());
        }
        trust_levels.insert(server.trust_level.to_string());
    }

    (
        200,
        json!({
            "categories": categories.into_iter().collect::<Vec<_>>(),
            "platforms": platforms.into_iter().collect::<Vec<_>>(),
            "trustLevels": trust_levels.into_iter().collect::<Vec<_>>()
        }),
    )
}

/// Returns whether a server matches all optional search filters.
fn matches_server_filters(
    server: &berth_registry::types::ServerMetadata,
    category: Option<&str>,
    platform: Option<&str>,
    trust_level: Option<&str>,
) -> bool {
    if let Some(category) = category {
        if !server.category.eq_ignore_ascii_case(category) {
            return false;
        }
    }

    if let Some(platform) = platform {
        if !server
            .compatibility
            .platforms
            .iter()
            .any(|p| p.eq_ignore_ascii_case(platform))
        {
            return false;
        }
    }

    if let Some(trust_level) = trust_level {
        if !server
            .trust_level
            .to_string()
            .eq_ignore_ascii_case(trust_level)
        {
            return false;
        }
    }

    true
}

/// Parses a positive integer query parameter.
fn parse_usize_param(query: Option<&str>, key: &str) -> Option<usize> {
    query_param(query, key)
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
}

/// Routes `/servers/<name>` and `/servers/<name>/downloads` paths.
fn route_server_detail(path: &str, registry: &Registry) -> (u16, Value) {
    let trimmed = path.trim_start_matches('/');
    let mut parts = trimmed.split('/');
    let first = parts.next().unwrap_or_default();
    if first != "servers" {
        return (
            404,
            json!({
                "error": "not found"
            }),
        );
    }

    let server_name = parts.next().unwrap_or_default();
    if server_name.is_empty() {
        return (
            404,
            json!({
                "error": "not found"
            }),
        );
    }

    let trailing = parts.next();
    if parts.next().is_some() {
        return (
            404,
            json!({
                "error": "not found"
            }),
        );
    }

    let Some(server) = registry.get(server_name) else {
        return (
            404,
            json!({
                "error": "server not found",
                "server": server_name
            }),
        );
    };

    match trailing {
        None => (
            200,
            json!({
                "server": server,
                "installCommand": format!("berth install {}", server.name)
            }),
        ),
        Some("downloads") => (
            200,
            json!({
                "server": server.name,
                "downloads": server.quality.downloads,
                "installCommand": format!("berth install {}", server.name)
            }),
        ),
        _ => (
            404,
            json!({
                "error": "not found"
            }),
        ),
    }
}

/// Splits request target into path and optional query string.
fn split_path_query(target: &str) -> (&str, Option<&str>) {
    if let Some((path, query)) = target.split_once('?') {
        (path, Some(query))
    } else {
        (target, None)
    }
}

/// Returns one query parameter value when present.
fn query_param<'a>(query: Option<&'a str>, key: &str) -> Option<&'a str> {
    let query = query?;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == key {
            return Some(v);
        }
    }
    None
}

/// Builds a compact API summary view for one registry server.
fn server_summary(server: &berth_registry::types::ServerMetadata) -> Value {
    json!({
        "name": server.name,
        "displayName": server.display_name,
        "description": server.description,
        "version": server.version,
        "category": server.category,
        "trustLevel": server.trust_level.to_string(),
        "downloads": server.quality.downloads,
        "installCommand": format!("berth install {}", server.name)
    })
}

/// Writes a JSON HTTP response to a stream.
fn write_json_response(stream: &mut TcpStream, status: u16, body: &Value) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    };
    let payload = serde_json::to_string(body)
        .unwrap_or_else(|_| "{\"error\":\"serialization failed\"}".to_string());
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload.len(),
        payload
    );
    stream.write_all(response.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_path_query_parses_query() {
        let (path, query) = split_path_query("/servers?q=github");
        assert_eq!(path, "/servers");
        assert_eq!(query, Some("q=github"));
    }

    #[test]
    fn query_param_extracts_value() {
        let v = query_param(Some("q=github&limit=20"), "q");
        assert_eq!(v, Some("github"));
    }

    #[test]
    fn route_request_handles_search_and_download_routes() {
        let registry = Registry::from_seed();
        let (status, search) = route_request("GET /servers?q=github HTTP/1.1", &registry);
        assert_eq!(status, 200);
        assert!(search["count"].as_u64().unwrap_or(0) >= 1);
        assert_eq!(
            search["servers"][0]["installCommand"].as_str(),
            Some("berth install github")
        );

        let (status_downloads, downloads) =
            route_request("GET /servers/github/downloads HTTP/1.1", &registry);
        assert_eq!(status_downloads, 200);
        assert_eq!(downloads["server"].as_str(), Some("github"));
    }

    #[test]
    fn route_request_returns_not_found_for_unknown_server() {
        let registry = Registry::from_seed();
        let (status, body) = route_request("GET /servers/nope HTTP/1.1", &registry);
        assert_eq!(status, 404);
        assert_eq!(body["error"].as_str(), Some("server not found"));
    }

    #[test]
    fn route_request_supports_filters_and_pagination() {
        let registry = Registry::from_seed();
        let (status, body) = route_request(
            "GET /servers?category=developer-tools&platform=macos&trustLevel=official&limit=1 HTTP/1.1",
            &registry,
        );
        assert_eq!(status, 200);
        assert_eq!(body["count"].as_u64(), Some(1));
        assert!(body["total"].as_u64().unwrap_or(0) >= 1);
        let first = &body["servers"][0];
        assert_eq!(first["category"].as_str(), Some("developer-tools"));
        assert_eq!(first["trustLevel"].as_str(), Some("official"));
    }

    #[test]
    fn route_request_lists_available_filter_values() {
        let registry = Registry::from_seed();
        let (status, body) = route_request("GET /servers/filters HTTP/1.1", &registry);
        assert_eq!(status, 200);
        let categories = body["categories"].as_array().unwrap();
        assert!(categories
            .iter()
            .any(|v| v.as_str() == Some("developer-tools")));
        let platforms = body["platforms"].as_array().unwrap();
        assert!(platforms.iter().any(|v| v.as_str() == Some("macos")));
        let trust_levels = body["trustLevels"].as_array().unwrap();
        assert!(trust_levels.iter().any(|v| v.as_str() == Some("official")));
    }
}
