//! Command handler for `berth registry-api`.

use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use berth_registry::types::{ServerMetadata, TrustLevel};
use berth_registry::Registry;

use crate::paths;

const MAX_REQUEST_BYTES: usize = 16 * 1024;

#[derive(Debug)]
struct ApiState {
    community_dir: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CommunitySnapshot {
    #[serde(default)]
    stars: std::collections::BTreeMap<String, u64>,
    #[serde(default)]
    reports: std::collections::BTreeMap<String, u64>,
    #[serde(default)]
    verified_publishers: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportEvent {
    timestamp_epoch_secs: u64,
    server: String,
    reason: String,
    details: String,
}

#[derive(Debug, Deserialize)]
struct ReportPayload {
    #[serde(default)]
    reason: String,
    #[serde(default)]
    details: String,
}

#[derive(Debug, Deserialize)]
struct PublisherPayload {
    #[serde(default)]
    maintainer: String,
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    target: String,
    body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortBy {
    Relevance,
    Name,
    Downloads,
    Stars,
    Reports,
    QualityScore,
}

impl SortBy {
    fn as_str(self) -> &'static str {
        match self {
            SortBy::Relevance => "relevance",
            SortBy::Name => "name",
            SortBy::Downloads => "downloads",
            SortBy::Stars => "stars",
            SortBy::Reports => "reports",
            SortBy::QualityScore => "qualityScore",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    fn as_str(self) -> &'static str {
        match self {
            SortOrder::Asc => "asc",
            SortOrder::Desc => "desc",
        }
    }
}

#[derive(Debug)]
struct ListedServer<'a> {
    server: &'a ServerMetadata,
    search_score: Option<u32>,
    maintainer_verified: bool,
    stars: u64,
    reports: u64,
    quality_score: u32,
}

impl ApiState {
    fn new(community_dir: PathBuf) -> Self {
        Self { community_dir }
    }

    fn snapshot_path(&self) -> PathBuf {
        self.community_dir.join("snapshot.json")
    }

    fn reports_dir(&self) -> PathBuf {
        self.community_dir.join("reports")
    }

    fn report_path(&self, server: &str) -> PathBuf {
        self.reports_dir().join(format!("{server}.jsonl"))
    }

    fn load_snapshot(&self) -> Result<CommunitySnapshot, String> {
        let path = self.snapshot_path();
        if !path.exists() {
            return Ok(CommunitySnapshot::default());
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read community snapshot {}: {e}", path.display()))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("failed to parse community snapshot {}: {e}", path.display()))
    }

    fn save_snapshot(&self, snapshot: &CommunitySnapshot) -> Result<(), String> {
        fs::create_dir_all(&self.community_dir).map_err(|e| {
            format!(
                "failed to create community directory {}: {e}",
                self.community_dir.display()
            )
        })?;
        let path = self.snapshot_path();
        let payload = serde_json::to_string_pretty(snapshot)
            .map_err(|e| format!("failed to serialize community snapshot: {e}"))?;
        fs::write(&path, payload)
            .map_err(|e| format!("failed to write community snapshot {}: {e}", path.display()))
    }

    fn increment_star(&self, server: &str) -> Result<u64, String> {
        let mut snapshot = self.load_snapshot()?;
        let value = snapshot.stars.entry(server.to_string()).or_insert(0);
        *value += 1;
        let stars = *value;
        self.save_snapshot(&snapshot)?;
        Ok(stars)
    }

    fn community_counts(&self, server: &str) -> Result<(u64, u64), String> {
        let snapshot = self.load_snapshot()?;
        let stars = snapshot.stars.get(server).copied().unwrap_or(0);
        let reports = snapshot.reports.get(server).copied().unwrap_or(0);
        Ok((stars, reports))
    }

    fn record_report(&self, server: &str, reason: &str, details: &str) -> Result<u64, String> {
        let mut snapshot = self.load_snapshot()?;
        let reports = snapshot.reports.entry(server.to_string()).or_insert(0);
        *reports += 1;
        let report_count = *reports;
        self.save_snapshot(&snapshot)?;

        let reports_dir = self.reports_dir();
        fs::create_dir_all(&reports_dir).map_err(|e| {
            format!(
                "failed to create reports directory {}: {e}",
                reports_dir.display()
            )
        })?;
        let report_path = self.report_path(server);
        let event = ReportEvent {
            timestamp_epoch_secs: now_epoch_secs(),
            server: server.to_string(),
            reason: reason.to_string(),
            details: details.to_string(),
        };
        let line = serde_json::to_string(&event)
            .map_err(|e| format!("failed to serialize report: {e}"))?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&report_path)
            .map_err(|e| format!("failed to open report file {}: {e}", report_path.display()))?;
        writeln!(file, "{line}")
            .map_err(|e| format!("failed to append report {}: {e}", report_path.display()))?;
        Ok(report_count)
    }

    fn list_reports(&self, server: &str) -> Result<Vec<ReportEvent>, String> {
        let report_path = self.report_path(server);
        if !report_path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(&report_path)
            .map_err(|e| format!("failed to read report file {}: {e}", report_path.display()))?;
        let mut reports = Vec::new();
        for (idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let event = serde_json::from_str::<ReportEvent>(trimmed).map_err(|e| {
                format!(
                    "failed to parse report file {} at line {}: {e}",
                    report_path.display(),
                    idx + 1
                )
            })?;
            reports.push(event);
        }
        reports.sort_by(|left, right| {
            right
                .timestamp_epoch_secs
                .cmp(&left.timestamp_epoch_secs)
                .then_with(|| left.reason.cmp(&right.reason))
        });
        Ok(reports)
    }

    fn list_verified_publishers(&self) -> Result<Vec<String>, String> {
        let snapshot = self.load_snapshot()?;
        let mut normalized: Vec<String> = snapshot
            .verified_publishers
            .iter()
            .map(|name| normalize_maintainer(name))
            .filter(|name| !name.is_empty())
            .collect();
        normalized.sort();
        normalized.dedup();
        Ok(normalized)
    }

    fn verify_publisher(&self, maintainer: &str) -> Result<Vec<String>, String> {
        let normalized = normalize_maintainer(maintainer);
        if normalized.is_empty() {
            return Err("maintainer is required".to_string());
        }
        let mut snapshot = self.load_snapshot()?;
        snapshot.verified_publishers.push(normalized);
        snapshot.verified_publishers.sort();
        snapshot.verified_publishers.dedup();
        self.save_snapshot(&snapshot)?;
        self.list_verified_publishers()
    }

    fn unverify_publisher(&self, maintainer: &str) -> Result<Vec<String>, String> {
        let normalized = normalize_maintainer(maintainer);
        if normalized.is_empty() {
            return Err("maintainer is required".to_string());
        }
        let mut snapshot = self.load_snapshot()?;
        snapshot
            .verified_publishers
            .retain(|name| normalize_maintainer(name) != normalized);
        self.save_snapshot(&snapshot)?;
        self.list_verified_publishers()
    }

    fn is_publisher_verified(&self, maintainer: &str) -> Result<bool, String> {
        let normalized = normalize_maintainer(maintainer);
        if normalized.is_empty() {
            return Ok(false);
        }
        let publishers = self.list_verified_publishers()?;
        Ok(publishers.iter().any(|name| name == &normalized))
    }
}

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
    let community_dir = paths::berth_home()
        .map(|home| home.join("registry").join("community"))
        .unwrap_or_else(|| PathBuf::from(".berth/registry/community"));
    let state = ApiState::new(community_dir);
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
        if let Err(e) = handle_connection(&mut stream, &registry, &state) {
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
fn handle_connection(
    stream: &mut TcpStream,
    registry: &Registry,
    state: &ApiState,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let request = read_http_request(stream)?;
    let (status, body) = route_request(&request, registry, state);
    write_json_response(stream, status, &body)
}

/// Reads an HTTP request (request line + headers + optional body) from a client stream.
fn read_http_request(stream: &mut TcpStream) -> io::Result<HttpRequest> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 512];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if find_header_end(&buf).is_some() {
            break;
        }
        if buf.len() >= MAX_REQUEST_BYTES {
            break;
        }
    }

    let Some(header_end) = find_header_end(&buf) else {
        return Ok(HttpRequest {
            method: String::new(),
            target: String::new(),
            body: String::new(),
        });
    };

    let headers_str = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let content_length = parse_content_length(&headers_str).unwrap_or(0);
    let body_start = header_end + 4;
    while buf.len().saturating_sub(body_start) < content_length && buf.len() < MAX_REQUEST_BYTES {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }

    let request_line = headers_str.lines().next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();

    let body_bytes = if body_start <= buf.len() {
        &buf[body_start..]
    } else {
        &[]
    };
    let body = String::from_utf8_lossy(body_bytes)
        .chars()
        .take(content_length)
        .collect::<String>();

    Ok(HttpRequest {
        method,
        target,
        body,
    })
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                return value.trim().parse::<usize>().ok();
            }
        }
    }
    None
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Routes a request to a status code and JSON response body.
fn route_request(request: &HttpRequest, registry: &Registry, state: &ApiState) -> (u16, Value) {
    let method = request.method.as_str();
    let target = request.target.as_str();
    if method.is_empty() || target.is_empty() {
        return (
            400,
            json!({
                "error": "malformed request line"
            }),
        );
    }
    if !matches!(method, "GET" | "POST" | "OPTIONS") {
        return (
            405,
            json!({
                "error": "method not allowed"
            }),
        );
    }

    let (path, query) = split_path_query(target);
    if method == "OPTIONS" {
        return (
            200,
            json!({
                "status": "ok",
                "path": path,
                "methods": ["GET", "POST", "OPTIONS"]
            }),
        );
    }
    match path {
        "/" | "/health" => {
            if method != "GET" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            (
                200,
                json!({
                    "status": "ok"
                }),
            )
        }
        "/servers/filters" => {
            if method != "GET" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            route_server_filters(registry)
        }
        "/servers/facets" => {
            if method != "GET" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            route_servers_facets(query, registry)
        }
        "/servers/suggest" => {
            if method != "GET" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            route_servers_suggest(query, registry, state)
        }
        "/stats" => {
            if method != "GET" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            route_stats(query, registry, state)
        }
        "/servers/trending" => {
            if method != "GET" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            route_servers_trending(query, registry, state)
        }
        "/publishers/verified" => {
            if method != "GET" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            route_verified_publishers(state)
        }
        "/publishers/verify" => {
            if method != "POST" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            route_verify_publisher(request.body.trim(), state)
        }
        "/publishers/unverify" => {
            if method != "POST" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            route_unverify_publisher(request.body.trim(), state)
        }
        "/servers" => {
            if method != "GET" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
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
            let sort_by = match parse_sort_by(query, query_value) {
                Ok(sort_by) => sort_by,
                Err(detail) => {
                    return (
                        400,
                        json!({
                            "error": "invalid sortBy",
                            "detail": detail
                        }),
                    );
                }
            };
            let sort_order = match parse_sort_order(query, sort_by) {
                Ok(sort_order) => sort_order,
                Err(detail) => {
                    return (
                        400,
                        json!({
                            "error": "invalid order",
                            "detail": detail
                        }),
                    );
                }
            };

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
            let verified_publishers = state.list_verified_publishers().unwrap_or_default();
            let mut listed = entries
                .into_iter()
                .map(|(server, score)| {
                    let (stars, reports) = state.community_counts(&server.name).unwrap_or((0, 0));
                    let maintainer_verified =
                        is_maintainer_verified(&server.maintainer, &verified_publishers);
                    let quality_score =
                        server_quality_score(server, maintainer_verified, stars, reports);
                    ListedServer {
                        server,
                        search_score: score,
                        maintainer_verified,
                        stars,
                        reports,
                        quality_score,
                    }
                })
                .collect::<Vec<_>>();
            listed.sort_by(|left, right| compare_listed_servers(left, right, sort_by, sort_order));
            let total = listed.len();
            let sliced = if let Some(limit) = limit {
                listed
                    .into_iter()
                    .skip(offset)
                    .take(limit)
                    .collect::<Vec<_>>()
            } else {
                listed.into_iter().skip(offset).collect::<Vec<_>>()
            };
            let servers = sliced
                .into_iter()
                .map(|entry| {
                    let mut summary = server_summary(
                        entry.server,
                        entry.maintainer_verified,
                        entry.quality_score,
                    );
                    if let Some(score) = entry.search_score {
                        if let Some(obj) = summary.as_object_mut() {
                            obj.insert("score".to_string(), json!(score));
                        }
                    }
                    if let Some(obj) = summary.as_object_mut() {
                        obj.insert("stars".to_string(), json!(entry.stars));
                        obj.insert("reports".to_string(), json!(entry.reports));
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
                    "sort": {
                        "by": sort_by.as_str(),
                        "order": sort_order.as_str()
                    },
                    "total": total,
                    "count": count,
                    "offset": offset,
                    "limit": limit,
                    "servers": servers
                }),
            )
        }
        _ => route_server_detail(method, path, query, request.body.trim(), registry, state),
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

fn route_servers_facets(query: Option<&str>, registry: &Registry) -> (u16, Value) {
    let query_value = query_param(query, "q")
        .or_else(|| query_param(query, "query"))
        .unwrap_or_default();
    let category_filter = query_param(query, "category").filter(|v| !v.trim().is_empty());
    let platform_filter = query_param(query, "platform").filter(|v| !v.trim().is_empty());
    let trust_filter = query_param(query, "trustLevel")
        .or_else(|| query_param(query, "trust_level"))
        .filter(|v| !v.trim().is_empty());

    let entries: Vec<&ServerMetadata> = if query_value.trim().is_empty() {
        registry.list_all().iter().collect()
    } else {
        registry
            .search(query_value)
            .into_iter()
            .map(|result| result.server)
            .collect()
    };

    let total = entries
        .iter()
        .filter(|server| {
            matches_server_filters(server, category_filter, platform_filter, trust_filter)
        })
        .count();

    let mut category_counts = std::collections::BTreeMap::<String, u64>::new();
    for server in entries
        .iter()
        .filter(|server| matches_server_filters(server, None, platform_filter, trust_filter))
    {
        *category_counts.entry(server.category.clone()).or_insert(0) += 1;
    }

    let mut platform_counts = std::collections::BTreeMap::<String, u64>::new();
    for server in entries
        .iter()
        .filter(|server| matches_server_filters(server, category_filter, None, trust_filter))
    {
        for platform in &server.compatibility.platforms {
            *platform_counts.entry(platform.clone()).or_insert(0) += 1;
        }
    }

    let mut trust_counts = std::collections::BTreeMap::<String, u64>::new();
    for server in entries
        .iter()
        .filter(|server| matches_server_filters(server, category_filter, platform_filter, None))
    {
        *trust_counts
            .entry(server.trust_level.to_string())
            .or_insert(0) += 1;
    }

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
            "facets": {
                "categories": sorted_facet_items(category_counts),
                "platforms": sorted_facet_items(platform_counts),
                "trustLevels": sorted_facet_items(trust_counts)
            }
        }),
    )
}

fn sorted_facet_items(counts: std::collections::BTreeMap<String, u64>) -> Vec<Value> {
    let mut items = counts.into_iter().collect::<Vec<_>>();
    items.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    items
        .into_iter()
        .map(|(value, count)| {
            json!({
                "value": value,
                "count": count
            })
        })
        .collect()
}

fn route_servers_suggest(
    query: Option<&str>,
    registry: &Registry,
    state: &ApiState,
) -> (u16, Value) {
    let raw_query = query_param(query, "q")
        .or_else(|| query_param(query, "query"))
        .unwrap_or_default();
    let normalized_query = raw_query.trim().to_lowercase();
    let category_filter = query_param(query, "category")
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let limit = parse_usize_param(query, "limit").unwrap_or(8).min(25);
    let verified_publishers = state.list_verified_publishers().unwrap_or_default();

    let mut suggestions = registry
        .list_all()
        .iter()
        .filter(|server| {
            category_filter.is_none_or(|category| server.category.eq_ignore_ascii_case(category))
        })
        .filter_map(|server| {
            let name = server.name.to_lowercase();
            let display_name = server.display_name.to_lowercase();
            let maintainer = server.maintainer.to_lowercase();
            let tags = server
                .tags
                .iter()
                .map(|tag| tag.to_lowercase())
                .collect::<Vec<_>>();
            let category = server.category.to_lowercase();
            let mut score: i32 = 0;

            if normalized_query.is_empty() {
                score += 10;
            } else {
                if name == normalized_query {
                    score += 220;
                } else if name.starts_with(&normalized_query) {
                    score += 140;
                } else if name.contains(&normalized_query) {
                    score += 90;
                }

                if display_name.starts_with(&normalized_query) {
                    score += 80;
                } else if display_name.contains(&normalized_query) {
                    score += 55;
                }

                if maintainer.contains(&normalized_query) {
                    score += 35;
                }
                if category.contains(&normalized_query) {
                    score += 20;
                }

                for tag in tags {
                    if tag.starts_with(&normalized_query) {
                        score += 40;
                    } else if tag.contains(&normalized_query) {
                        score += 20;
                    }
                }
            }

            if score == 0 {
                return None;
            }

            let (stars, reports) = state.community_counts(&server.name).unwrap_or((0, 0));
            let maintainer_verified =
                is_maintainer_verified(&server.maintainer, &verified_publishers);
            let quality_score = server_quality_score(server, maintainer_verified, stars, reports);

            score += (quality_score / 8) as i32;
            score += match server.quality.downloads {
                0..=999 => 2,
                1000..=9999 => 4,
                _ => 6,
            };

            Some((server, score, quality_score, maintainer_verified))
        })
        .collect::<Vec<_>>();

    suggestions.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| right.0.quality.downloads.cmp(&left.0.quality.downloads))
            .then_with(|| left.0.name.cmp(&right.0.name))
    });

    let servers = suggestions
        .into_iter()
        .take(limit)
        .map(|(server, score, quality_score, maintainer_verified)| {
            json!({
                "name": server.name,
                "displayName": server.display_name,
                "category": server.category,
                "maintainer": server.maintainer,
                "maintainerVerified": maintainer_verified,
                "trustLevel": server.trust_level.to_string(),
                "qualityScore": quality_score,
                "installCommand": format!("berth install {}", server.name),
                "score": score
            })
        })
        .collect::<Vec<_>>();

    (
        200,
        json!({
            "query": raw_query,
            "filters": {
                "category": category_filter
            },
            "count": servers.len(),
            "limit": limit,
            "servers": servers
        }),
    )
}

fn route_stats(query: Option<&str>, registry: &Registry, state: &ApiState) -> (u16, Value) {
    let top_limit = parse_usize_param(query, "top").unwrap_or(5).min(20);
    let verified_publishers = state.list_verified_publishers().unwrap_or_default();

    let mut categories = std::collections::BTreeMap::<String, u64>::new();
    let mut trust_levels = std::collections::BTreeMap::<String, u64>::new();
    let mut platforms = std::collections::BTreeMap::<String, u64>::new();
    let mut maintainers = std::collections::BTreeMap::<String, u64>::new();
    let mut stars_total = 0_u64;
    let mut reports_total = 0_u64;
    let mut downloads_total = 0_u64;
    let mut top_downloaded = Vec::new();
    let mut top_trending = Vec::new();

    for server in registry.list_all() {
        *categories.entry(server.category.clone()).or_insert(0) += 1;
        *trust_levels
            .entry(server.trust_level.to_string())
            .or_insert(0) += 1;
        for platform in &server.compatibility.platforms {
            *platforms.entry(platform.clone()).or_insert(0) += 1;
        }
        *maintainers.entry(server.maintainer.clone()).or_insert(0) += 1;

        let (stars, reports) = state.community_counts(&server.name).unwrap_or((0, 0));
        stars_total += stars;
        reports_total += reports;
        downloads_total += server.quality.downloads;

        let maintainer_verified = is_maintainer_verified(&server.maintainer, &verified_publishers);
        let quality_score = server_quality_score(server, maintainer_verified, stars, reports);
        let trend_score =
            server_trending_score(server, quality_score, stars, reports, maintainer_verified);

        top_downloaded.push((
            server,
            maintainer_verified,
            quality_score,
            server.quality.downloads,
        ));
        top_trending.push((
            server,
            maintainer_verified,
            quality_score,
            trend_score,
            stars,
            reports,
        ));
    }

    top_downloaded.sort_by(|left, right| {
        right
            .3
            .cmp(&left.3)
            .then_with(|| left.0.name.cmp(&right.0.name))
    });
    top_trending.sort_by(|left, right| {
        right
            .3
            .cmp(&left.3)
            .then_with(|| left.0.name.cmp(&right.0.name))
    });

    let downloaded_servers = top_downloaded
        .into_iter()
        .take(top_limit)
        .map(|(server, maintainer_verified, quality_score, downloads)| {
            let mut summary = server_summary(server, maintainer_verified, quality_score);
            if let Some(obj) = summary.as_object_mut() {
                obj.insert("downloads".to_string(), json!(downloads));
            }
            summary
        })
        .collect::<Vec<_>>();

    let trending_servers = top_trending
        .into_iter()
        .take(top_limit)
        .map(
            |(server, maintainer_verified, quality_score, trend_score, stars, reports)| {
                let mut summary = server_summary(server, maintainer_verified, quality_score);
                if let Some(obj) = summary.as_object_mut() {
                    obj.insert("trendScore".to_string(), json!(trend_score));
                    obj.insert("stars".to_string(), json!(stars));
                    obj.insert("reports".to_string(), json!(reports));
                }
                summary
            },
        )
        .collect::<Vec<_>>();

    let mut maintainer_entries = maintainers
        .into_iter()
        .map(|(maintainer, servers)| {
            let verified = is_maintainer_verified(&maintainer, &verified_publishers);
            (maintainer, servers, verified)
        })
        .collect::<Vec<_>>();
    maintainer_entries
        .sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    let top_maintainers = maintainer_entries
        .into_iter()
        .take(top_limit)
        .map(|(maintainer, servers, verified)| {
            json!({
                "maintainer": maintainer,
                "servers": servers,
                "verified": verified
            })
        })
        .collect::<Vec<_>>();

    (
        200,
        json!({
            "servers": {
                "total": registry.list_all().len(),
                "downloadsTotal": downloads_total
            },
            "community": {
                "starsTotal": stars_total,
                "reportsTotal": reports_total,
                "verifiedPublishers": verified_publishers.len()
            },
            "breakdown": {
                "categories": categories,
                "trustLevels": trust_levels,
                "platforms": platforms
            },
            "top": {
                "downloaded": downloaded_servers,
                "trending": trending_servers,
                "maintainers": top_maintainers
            }
        }),
    )
}

fn route_servers_trending(
    query: Option<&str>,
    registry: &Registry,
    state: &ApiState,
) -> (u16, Value) {
    let category_filter = query_param(query, "category").filter(|v| !v.trim().is_empty());
    let platform_filter = query_param(query, "platform").filter(|v| !v.trim().is_empty());
    let trust_filter = query_param(query, "trustLevel")
        .or_else(|| query_param(query, "trust_level"))
        .filter(|v| !v.trim().is_empty());
    let offset = parse_usize_param(query, "offset").unwrap_or(0);
    let limit = parse_usize_param(query, "limit").unwrap_or(10).min(100);

    let verified_publishers = state.list_verified_publishers().unwrap_or_default();
    let mut entries = registry
        .list_all()
        .iter()
        .filter(|server| {
            matches_server_filters(server, category_filter, platform_filter, trust_filter)
        })
        .map(|server| {
            let (stars, reports) = state.community_counts(&server.name).unwrap_or((0, 0));
            let maintainer_verified =
                is_maintainer_verified(&server.maintainer, &verified_publishers);
            let quality_score = server_quality_score(server, maintainer_verified, stars, reports);
            let trend_score =
                server_trending_score(server, quality_score, stars, reports, maintainer_verified);
            (
                server,
                maintainer_verified,
                stars,
                reports,
                quality_score,
                trend_score,
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .5
            .cmp(&left.5)
            .then_with(|| left.0.name.cmp(&right.0.name))
    });
    let total = entries.len();
    let servers = entries
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(
            |(server, maintainer_verified, stars, reports, quality_score, trend_score)| {
                let mut summary = server_summary(server, maintainer_verified, quality_score);
                if let Some(obj) = summary.as_object_mut() {
                    obj.insert("stars".to_string(), json!(stars));
                    obj.insert("reports".to_string(), json!(reports));
                    obj.insert("trendScore".to_string(), json!(trend_score));
                }
                summary
            },
        )
        .collect::<Vec<_>>();
    let count = servers.len();
    (
        200,
        json!({
            "filters": {
                "category": category_filter,
                "platform": platform_filter,
                "trustLevel": trust_filter
            },
            "sort": {
                "by": "trendScore",
                "order": "desc"
            },
            "total": total,
            "count": count,
            "offset": offset,
            "limit": limit,
            "servers": servers
        }),
    )
}

fn route_verified_publishers(state: &ApiState) -> (u16, Value) {
    match state.list_verified_publishers() {
        Ok(verified_publishers) => (
            200,
            json!({
                "count": verified_publishers.len(),
                "verifiedPublishers": verified_publishers
            }),
        ),
        Err(e) => (
            500,
            json!({
                "error": "internal error",
                "detail": e
            }),
        ),
    }
}

fn route_verify_publisher(body: &str, state: &ApiState) -> (u16, Value) {
    let maintainer = match parse_publisher_body(body) {
        Ok(maintainer) => maintainer,
        Err(err) => return err,
    };
    match state.verify_publisher(&maintainer) {
        Ok(verified_publishers) => (
            200,
            json!({
                "status": "verified",
                "maintainer": maintainer,
                "count": verified_publishers.len(),
                "verifiedPublishers": verified_publishers
            }),
        ),
        Err(e) => (
            500,
            json!({
                "error": "internal error",
                "detail": e
            }),
        ),
    }
}

fn route_unverify_publisher(body: &str, state: &ApiState) -> (u16, Value) {
    let maintainer = match parse_publisher_body(body) {
        Ok(maintainer) => maintainer,
        Err(err) => return err,
    };
    match state.unverify_publisher(&maintainer) {
        Ok(verified_publishers) => (
            200,
            json!({
                "status": "unverified",
                "maintainer": maintainer,
                "count": verified_publishers.len(),
                "verifiedPublishers": verified_publishers
            }),
        ),
        Err(e) => (
            500,
            json!({
                "error": "internal error",
                "detail": e
            }),
        ),
    }
}

fn parse_publisher_body(body: &str) -> Result<String, (u16, Value)> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err((
            400,
            json!({
                "error": "maintainer is required"
            }),
        ));
    }

    let maintainer = if trimmed.starts_with('{') {
        match serde_json::from_str::<PublisherPayload>(trimmed) {
            Ok(payload) => payload.maintainer,
            Err(e) => {
                return Err((
                    400,
                    json!({
                        "error": "invalid json body",
                        "detail": e.to_string()
                    }),
                ));
            }
        }
    } else if trimmed.starts_with('"') {
        match serde_json::from_str::<String>(trimmed) {
            Ok(value) => value,
            Err(e) => {
                return Err((
                    400,
                    json!({
                        "error": "invalid json body",
                        "detail": e.to_string()
                    }),
                ));
            }
        }
    } else {
        trimmed.to_string()
    };
    let normalized = normalize_maintainer(&maintainer);
    if normalized.is_empty() {
        return Err((
            400,
            json!({
                "error": "maintainer is required"
            }),
        ));
    }
    Ok(normalized)
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

/// Parses sort-by query parameter for `/servers`.
fn parse_sort_by(query: Option<&str>, query_value: &str) -> Result<SortBy, String> {
    let default = if query_value.trim().is_empty() {
        SortBy::Name
    } else {
        SortBy::Relevance
    };
    let Some(raw) = query_param(query, "sortBy")
        .or_else(|| query_param(query, "sort"))
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return Ok(default);
    };

    if raw.eq_ignore_ascii_case("relevance") {
        Ok(SortBy::Relevance)
    } else if raw.eq_ignore_ascii_case("name") {
        Ok(SortBy::Name)
    } else if raw.eq_ignore_ascii_case("downloads") {
        Ok(SortBy::Downloads)
    } else if raw.eq_ignore_ascii_case("stars") {
        Ok(SortBy::Stars)
    } else if raw.eq_ignore_ascii_case("reports") {
        Ok(SortBy::Reports)
    } else if raw.eq_ignore_ascii_case("qualityScore")
        || raw.eq_ignore_ascii_case("quality_score")
        || raw.eq_ignore_ascii_case("quality")
    {
        Ok(SortBy::QualityScore)
    } else {
        Err(format!(
            "{raw}; supported values: relevance, name, downloads, stars, reports, qualityScore"
        ))
    }
}

/// Parses sort order query parameter for `/servers`.
fn parse_sort_order(query: Option<&str>, sort_by: SortBy) -> Result<SortOrder, String> {
    let default = if sort_by == SortBy::Relevance {
        SortOrder::Desc
    } else {
        SortOrder::Asc
    };
    let Some(raw) = query_param(query, "order")
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return Ok(default);
    };

    if raw.eq_ignore_ascii_case("asc") {
        Ok(SortOrder::Asc)
    } else if raw.eq_ignore_ascii_case("desc") {
        Ok(SortOrder::Desc)
    } else {
        Err(format!("{raw}; supported values: asc, desc"))
    }
}

/// Compares list entries according to sort mode and order.
fn compare_listed_servers(
    left: &ListedServer<'_>,
    right: &ListedServer<'_>,
    sort_by: SortBy,
    order: SortOrder,
) -> std::cmp::Ordering {
    let asc = match sort_by {
        SortBy::Relevance => left
            .search_score
            .unwrap_or(0)
            .cmp(&right.search_score.unwrap_or(0))
            .then_with(|| left.server.name.cmp(&right.server.name)),
        SortBy::Name => left.server.name.cmp(&right.server.name),
        SortBy::Downloads => left
            .server
            .quality
            .downloads
            .cmp(&right.server.quality.downloads)
            .then_with(|| left.server.name.cmp(&right.server.name)),
        SortBy::Stars => left
            .stars
            .cmp(&right.stars)
            .then_with(|| left.server.name.cmp(&right.server.name)),
        SortBy::Reports => left
            .reports
            .cmp(&right.reports)
            .then_with(|| left.server.name.cmp(&right.server.name)),
        SortBy::QualityScore => left
            .quality_score
            .cmp(&right.quality_score)
            .then_with(|| left.server.name.cmp(&right.server.name)),
    };
    if order == SortOrder::Asc {
        asc
    } else {
        asc.reverse()
    }
}

/// Routes `/servers/<name>` detail/community/star/report/reports/related paths.
fn route_server_detail(
    method: &str,
    path: &str,
    query: Option<&str>,
    body: &str,
    registry: &Registry,
    state: &ApiState,
) -> (u16, Value) {
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
        None => {
            if method != "GET" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            let (stars, report_count) = state.community_counts(server_name).unwrap_or((0, 0));
            let maintainer_verified = state
                .is_publisher_verified(&server.maintainer)
                .unwrap_or(false);
            let badges = publisher_badges(maintainer_verified);
            let quality_score =
                server_quality_score(server, maintainer_verified, stars, report_count);
            (
                200,
                json!({
                    "server": server,
                    "installCommand": format!("berth install {}", server.name),
                    "installCommandCopy": format!("berth install {}", server.name),
                    "community": {
                        "stars": stars,
                        "reports": report_count
                    },
                    "permissionsSummary": permissions_summary(server),
                    "maintainerVerified": maintainer_verified,
                    "badges": badges,
                    "qualityScore": quality_score,
                    "readmeUrl": readme_url_for_repository(&server.source.repository)
                }),
            )
        }
        Some("downloads") => {
            if method != "GET" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            (
                200,
                json!({
                    "server": server.name,
                    "downloads": server.quality.downloads,
                    "installCommand": format!("berth install {}", server.name)
                }),
            )
        }
        Some("community") => {
            if method != "GET" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            route_server_community(server_name, state)
        }
        Some("related") => {
            if method != "GET" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            route_server_related(server, query, registry, state)
        }
        Some("star") => {
            if method != "POST" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            route_server_star(server, state)
        }
        Some("report") => {
            if method != "POST" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            route_server_report(server, body, state)
        }
        Some("reports") => {
            if method != "GET" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            route_server_reports(server, query, state)
        }
        _ => (
            404,
            json!({
                "error": "not found"
            }),
        ),
    }
}

fn route_server_community(server_name: &str, state: &ApiState) -> (u16, Value) {
    match state.community_counts(server_name) {
        Ok((stars, reports)) => (
            200,
            json!({
                "server": server_name,
                "stars": stars,
                "reports": reports
            }),
        ),
        Err(e) => (
            500,
            json!({
                "error": "internal error",
                "detail": e
            }),
        ),
    }
}

fn route_server_related(
    server: &ServerMetadata,
    query: Option<&str>,
    registry: &Registry,
    state: &ApiState,
) -> (u16, Value) {
    let limit = parse_usize_param(query, "limit").unwrap_or(6).min(25);
    let offset = parse_usize_param(query, "offset").unwrap_or(0);
    let verified_publishers = state.list_verified_publishers().unwrap_or_default();
    let server_maintainer = normalize_maintainer(&server.maintainer);

    let mut related = registry
        .list_all()
        .iter()
        .filter(|candidate| candidate.name != server.name)
        .map(|candidate| {
            let shared_tags = shared_values(&server.tags, &candidate.tags);
            let shared_platforms = shared_values(
                &server.compatibility.platforms,
                &candidate.compatibility.platforms,
            );
            let same_category = candidate.category.eq_ignore_ascii_case(&server.category);
            let candidate_maintainer = normalize_maintainer(&candidate.maintainer);
            let same_maintainer =
                !server_maintainer.is_empty() && candidate_maintainer == server_maintainer;
            let same_trust = candidate.trust_level.to_string() == server.trust_level.to_string();
            let related_score = related_server_score(
                same_category,
                same_maintainer,
                same_trust,
                shared_tags.len(),
                shared_platforms.len(),
                candidate.quality.downloads,
            );
            let (stars, reports) = state.community_counts(&candidate.name).unwrap_or((0, 0));
            let maintainer_verified =
                is_maintainer_verified(&candidate.maintainer, &verified_publishers);
            let quality_score =
                server_quality_score(candidate, maintainer_verified, stars, reports);
            (
                candidate,
                related_score,
                shared_tags,
                shared_platforms,
                same_category,
                same_maintainer,
                stars,
                reports,
                maintainer_verified,
                quality_score,
            )
        })
        .collect::<Vec<_>>();

    related.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| left.0.name.cmp(&right.0.name))
    });
    let total = related.len();
    let servers = related
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(
            |(
                candidate,
                related_score,
                shared_tags,
                shared_platforms,
                same_category,
                same_maintainer,
                stars,
                reports,
                maintainer_verified,
                quality_score,
            )| {
                let mut summary = server_summary(candidate, maintainer_verified, quality_score);
                if let Some(obj) = summary.as_object_mut() {
                    obj.insert("relatedScore".to_string(), json!(related_score));
                    obj.insert("stars".to_string(), json!(stars));
                    obj.insert("reports".to_string(), json!(reports));
                    obj.insert(
                        "match".to_string(),
                        json!({
                            "sameCategory": same_category,
                            "sameMaintainer": same_maintainer,
                            "sharedTags": shared_tags,
                            "sharedPlatforms": shared_platforms
                        }),
                    );
                }
                summary
            },
        )
        .collect::<Vec<_>>();
    let count = servers.len();

    (
        200,
        json!({
            "server": server.name,
            "total": total,
            "count": count,
            "offset": offset,
            "limit": limit,
            "servers": servers
        }),
    )
}

fn route_server_star(server: &ServerMetadata, state: &ApiState) -> (u16, Value) {
    match state.increment_star(&server.name) {
        Ok(stars) => (
            200,
            json!({
                "server": server.name,
                "stars": stars
            }),
        ),
        Err(e) => (
            500,
            json!({
                "error": "internal error",
                "detail": e
            }),
        ),
    }
}

fn route_server_report(server: &ServerMetadata, body: &str, state: &ApiState) -> (u16, Value) {
    let payload = if body.trim().is_empty() {
        ReportPayload {
            reason: String::new(),
            details: String::new(),
        }
    } else {
        match serde_json::from_str::<ReportPayload>(body) {
            Ok(payload) => payload,
            Err(e) => {
                return (
                    400,
                    json!({
                        "error": "invalid json body",
                        "detail": e.to_string()
                    }),
                );
            }
        }
    };
    let reason = if payload.reason.trim().is_empty() {
        "unspecified".to_string()
    } else {
        payload.reason.trim().to_string()
    };
    let details = payload.details.trim().to_string();

    match state.record_report(&server.name, &reason, &details) {
        Ok(reports) => (
            200,
            json!({
                "server": server.name,
                "status": "received",
                "reports": reports
            }),
        ),
        Err(e) => (
            500,
            json!({
                "error": "internal error",
                "detail": e
            }),
        ),
    }
}

fn route_server_reports(
    server: &ServerMetadata,
    query: Option<&str>,
    state: &ApiState,
) -> (u16, Value) {
    let limit = parse_usize_param(query, "limit").unwrap_or(20).min(100);
    match state.list_reports(&server.name) {
        Ok(events) => {
            let total = events.len();
            let reports = events.into_iter().take(limit).collect::<Vec<_>>();
            let count = reports.len();
            (
                200,
                json!({
                    "server": server.name,
                    "total": total,
                    "count": count,
                    "limit": limit,
                    "reports": reports
                }),
            )
        }
        Err(e) => (
            500,
            json!({
                "error": "internal error",
                "detail": e
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

/// Produces a normalized maintainer identifier for reliable comparisons.
fn normalize_maintainer(maintainer: &str) -> String {
    maintainer
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Returns `true` when a maintainer is in the verified list.
fn is_maintainer_verified(maintainer: &str, verified_publishers: &[String]) -> bool {
    let normalized = normalize_maintainer(maintainer);
    !normalized.is_empty() && verified_publishers.iter().any(|name| name == &normalized)
}

/// Builds badges for API responses.
fn publisher_badges(maintainer_verified: bool) -> Vec<String> {
    if maintainer_verified {
        vec!["verified-publisher".to_string()]
    } else {
        Vec::new()
    }
}

/// Computes shared values from two case-insensitive string lists.
fn shared_values(left: &[String], right: &[String]) -> Vec<String> {
    let mut left_set = std::collections::BTreeSet::new();
    for value in left {
        left_set.insert(value.to_lowercase());
    }

    let mut seen = std::collections::BTreeSet::new();
    let mut shared = Vec::new();
    for value in right {
        let normalized = value.to_lowercase();
        if left_set.contains(&normalized) && seen.insert(normalized.clone()) {
            shared.push(normalized);
        }
    }
    shared
}

/// Produces a deterministic related-server score for detail pages.
fn related_server_score(
    same_category: bool,
    same_maintainer: bool,
    same_trust: bool,
    shared_tags: usize,
    shared_platforms: usize,
    downloads: u64,
) -> u32 {
    let mut score: i32 = 0;
    if same_category {
        score += 35;
    }
    if same_maintainer {
        score += 30;
    }
    if same_trust {
        score += 10;
    }
    score += (shared_tags.min(4) as i32) * 12;
    score += (shared_platforms.min(3) as i32) * 8;
    score += match downloads {
        0..=999 => 2,
        1000..=9999 => 5,
        _ => 8,
    };
    score.clamp(0, 100) as u32
}

/// Builds a normalized permission summary for website rendering.
fn permissions_summary(server: &ServerMetadata) -> Value {
    let has_wildcard = |entries: &[String]| entries.iter().any(|entry| entry.trim() == "*");
    let has_filesystem_write = server
        .permissions
        .filesystem
        .iter()
        .any(|entry| entry.trim() == "*" || entry.trim_start().starts_with("write:"));

    json!({
        "network": {
            "count": server.permissions.network.len(),
            "wildcard": has_wildcard(&server.permissions.network)
        },
        "env": {
            "count": server.permissions.env.len(),
            "wildcard": has_wildcard(&server.permissions.env)
        },
        "filesystem": {
            "count": server.permissions.filesystem.len(),
            "wildcard": has_wildcard(&server.permissions.filesystem),
            "hasWriteAccess": has_filesystem_write
        },
        "exec": {
            "count": server.permissions.exec.len(),
            "wildcard": has_wildcard(&server.permissions.exec)
        },
        "total": server.permissions.network.len()
            + server.permissions.env.len()
            + server.permissions.filesystem.len()
            + server.permissions.exec.len()
    })
}

/// Returns a best-effort README URL for a repository.
fn readme_url_for_repository(repository: &str) -> Option<String> {
    let trimmed = repository.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }

    let github_rest = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"));
    if let Some(rest) = github_rest {
        let mut parts = rest.split('/');
        let owner = parts.next().unwrap_or_default().trim();
        let repo = parts
            .next()
            .unwrap_or_default()
            .trim()
            .trim_end_matches(".git");
        if !owner.is_empty() && !repo.is_empty() {
            return Some(format!(
                "https://github.com/{owner}/{repo}/blob/main/README.md"
            ));
        }
    }

    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        Some(format!("{trimmed}/README.md"))
    } else {
        None
    }
}

/// Produces a coarse, deterministic quality score for website ranking.
fn server_quality_score(
    server: &ServerMetadata,
    maintainer_verified: bool,
    stars: u64,
    reports: u64,
) -> u32 {
    let mut score: i32 = 0;

    score += match server.trust_level {
        TrustLevel::Official => 35,
        TrustLevel::Verified => 28,
        TrustLevel::Community => 20,
        TrustLevel::Untrusted => 8,
    };

    if server.quality.security_scan.eq_ignore_ascii_case("passed") {
        score += 20;
    } else if server.quality.security_scan.eq_ignore_ascii_case("unknown") {
        score += 8;
    }

    if server.quality.health_check {
        score += 15;
    }
    if maintainer_verified {
        score += 10;
    }

    score += match server.quality.downloads {
        0 => 0,
        1..=99 => 4,
        100..=999 => 8,
        1000..=9999 => 12,
        _ => 15,
    };

    score += stars.min(10) as i32;
    score -= reports.min(10) as i32;

    score.clamp(0, 100) as u32
}

/// Produces a deterministic trending score for homepage catalog ranking.
fn server_trending_score(
    server: &ServerMetadata,
    quality_score: u32,
    stars: u64,
    reports: u64,
    maintainer_verified: bool,
) -> u32 {
    let mut score: i64 = (quality_score as i64) * 2;
    score += (server.quality.downloads.min(50_000) / 500) as i64;
    score += (stars.min(50) * 6) as i64;
    score -= (reports.min(50) * 8) as i64;
    if maintainer_verified {
        score += 20;
    }
    score.clamp(0, 1000) as u32
}

/// Builds a compact API summary view for one registry server.
fn server_summary(
    server: &berth_registry::types::ServerMetadata,
    maintainer_verified: bool,
    quality_score: u32,
) -> Value {
    let badges = publisher_badges(maintainer_verified);
    json!({
        "name": server.name,
        "displayName": server.display_name,
        "description": server.description,
        "version": server.version,
        "category": server.category,
        "maintainer": server.maintainer,
        "permissionsSummary": permissions_summary(server),
        "maintainerVerified": maintainer_verified,
        "badges": badges,
        "qualityScore": quality_score,
        "readmeUrl": readme_url_for_repository(&server.source.repository),
        "trustLevel": server.trust_level.to_string(),
        "downloads": server.quality.downloads,
        "installCommand": format!("berth install {}", server.name),
        "installCommandCopy": format!("berth install {}", server.name)
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
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nAccess-Control-Max-Age: 86400\r\nConnection: close\r\n\r\n{}",
        payload.len(),
        payload
    );
    stream.write_all(response.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> ApiState {
        let unique = format!(
            "berth-registry-api-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique).join("community");
        std::fs::create_dir_all(&path).unwrap();
        ApiState::new(path)
    }

    fn req(method: &str, target: &str) -> HttpRequest {
        HttpRequest {
            method: method.to_string(),
            target: target.to_string(),
            body: String::new(),
        }
    }

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
        let state = test_state();
        let (status, search) = route_request(&req("GET", "/servers?q=github"), &registry, &state);
        assert_eq!(status, 200);
        assert!(search["count"].as_u64().unwrap_or(0) >= 1);
        let github = search["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|server| server["name"].as_str() == Some("github"))
            .unwrap();
        assert_eq!(
            github["installCommand"].as_str(),
            Some("berth install github")
        );
        assert_eq!(
            github["installCommandCopy"].as_str(),
            Some("berth install github")
        );
        assert!(github["permissionsSummary"]["total"].as_u64().unwrap_or(0) >= 1);
        assert!(github["qualityScore"].as_u64().unwrap_or(0) > 0);
        assert!(github["readmeUrl"]
            .as_str()
            .unwrap_or_default()
            .contains("github.com"));

        let (status_downloads, downloads) =
            route_request(&req("GET", "/servers/github/downloads"), &registry, &state);
        assert_eq!(status_downloads, 200);
        assert_eq!(downloads["server"].as_str(), Some("github"));
    }

    #[test]
    fn route_request_returns_not_found_for_unknown_server() {
        let registry = Registry::from_seed();
        let state = test_state();
        let (status, body) = route_request(&req("GET", "/servers/nope"), &registry, &state);
        assert_eq!(status, 404);
        assert_eq!(body["error"].as_str(), Some("server not found"));
    }

    #[test]
    fn route_request_supports_filters_and_pagination() {
        let registry = Registry::from_seed();
        let state = test_state();
        let (status, body) = route_request(
            &req(
                "GET",
                "/servers?category=developer-tools&platform=macos&trustLevel=official&limit=1",
            ),
            &registry,
            &state,
        );
        assert_eq!(status, 200);
        assert_eq!(body["count"].as_u64(), Some(1));
        assert!(body["total"].as_u64().unwrap_or(0) >= 1);
        let first = &body["servers"][0];
        assert_eq!(first["category"].as_str(), Some("developer-tools"));
        assert_eq!(first["trustLevel"].as_str(), Some("official"));
    }

    #[test]
    fn route_request_supports_sorting_with_metadata() {
        let registry = Registry::from_seed();
        let state = test_state();
        let (status, body) = route_request(
            &req("GET", "/servers?sortBy=name&order=asc&limit=5"),
            &registry,
            &state,
        );
        assert_eq!(status, 200);
        assert_eq!(body["sort"]["by"].as_str(), Some("name"));
        assert_eq!(body["sort"]["order"].as_str(), Some("asc"));

        let names = body["servers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        let mut expected = names.clone();
        expected.sort();
        assert_eq!(names, expected);
    }

    #[test]
    fn route_request_rejects_invalid_sort_inputs() {
        let registry = Registry::from_seed();
        let state = test_state();

        let (bad_sort_status, bad_sort_body) =
            route_request(&req("GET", "/servers?sortBy=banana"), &registry, &state);
        assert_eq!(bad_sort_status, 400);
        assert_eq!(bad_sort_body["error"].as_str(), Some("invalid sortBy"));

        let (bad_order_status, bad_order_body) =
            route_request(&req("GET", "/servers?order=sideways"), &registry, &state);
        assert_eq!(bad_order_status, 400);
        assert_eq!(bad_order_body["error"].as_str(), Some("invalid order"));
    }

    #[test]
    fn route_request_lists_available_filter_values() {
        let registry = Registry::from_seed();
        let state = test_state();
        let (status, body) = route_request(&req("GET", "/servers/filters"), &registry, &state);
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

    #[test]
    fn route_request_supports_trending_endpoint() {
        let registry = Registry::from_seed();
        let state = test_state();
        let _ = route_request(&req("POST", "/servers/github/star"), &registry, &state);
        let _ = route_request(&req("POST", "/servers/github/star"), &registry, &state);

        let (status, body) = route_request(
            &req("GET", "/servers/trending?limit=5&platform=macos"),
            &registry,
            &state,
        );
        assert_eq!(status, 200);
        assert_eq!(body["sort"]["by"].as_str(), Some("trendScore"));
        assert_eq!(body["sort"]["order"].as_str(), Some("desc"));
        assert!(body["count"].as_u64().unwrap_or(0) >= 1);
        let servers = body["servers"].as_array().unwrap();
        assert!(servers.iter().all(|s| s["trendScore"].as_u64().is_some()));
        let github = servers
            .iter()
            .find(|s| s["name"].as_str() == Some("github"))
            .unwrap();
        assert!(github["stars"].as_u64().unwrap_or(0) >= 2);
    }

    #[test]
    fn route_request_supports_related_endpoint() {
        let registry = Registry::from_seed();
        let state = test_state();

        let (status, body) = route_request(
            &req("GET", "/servers/github/related?limit=3"),
            &registry,
            &state,
        );
        assert_eq!(status, 200);
        assert_eq!(body["server"].as_str(), Some("github"));
        assert!(body["count"].as_u64().unwrap_or(0) <= 3);
        let servers = body["servers"].as_array().unwrap();
        assert!(!servers.is_empty());
        assert!(servers
            .iter()
            .all(|candidate| candidate["name"].as_str() != Some("github")));
        assert!(servers
            .iter()
            .all(|candidate| candidate["relatedScore"].as_u64().is_some()));
        assert!(servers.iter().all(|candidate| {
            candidate["match"]["sharedTags"].is_array()
                && candidate["match"]["sharedPlatforms"].is_array()
        }));
    }

    #[test]
    fn route_request_supports_stats_endpoint() {
        let registry = Registry::from_seed();
        let state = test_state();

        let _ = route_request(&req("POST", "/servers/github/star"), &registry, &state);
        let _ = route_request(&req("POST", "/servers/github/report"), &registry, &state);

        let (status, body) = route_request(&req("GET", "/stats?top=3"), &registry, &state);
        assert_eq!(status, 200);
        assert!(body["servers"]["total"].as_u64().unwrap_or(0) >= 1);
        assert!(body["servers"]["downloadsTotal"].as_u64().unwrap_or(0) > 0);
        assert!(body["community"]["starsTotal"].as_u64().unwrap_or(0) >= 1);
        assert!(body["community"]["reportsTotal"].as_u64().unwrap_or(0) >= 1);

        let downloaded = body["top"]["downloaded"].as_array().unwrap();
        assert!(!downloaded.is_empty());
        assert!(downloaded.len() <= 3);
        if downloaded.len() >= 2 {
            assert!(
                downloaded[0]["downloads"].as_u64().unwrap_or(0)
                    >= downloaded[1]["downloads"].as_u64().unwrap_or(0)
            );
        }

        let maintainers = body["top"]["maintainers"].as_array().unwrap();
        assert!(!maintainers.is_empty());
    }

    #[test]
    fn route_request_supports_suggest_endpoint() {
        let registry = Registry::from_seed();
        let state = test_state();

        let (status, body) = route_request(
            &req("GET", "/servers/suggest?q=git&limit=5"),
            &registry,
            &state,
        );
        assert_eq!(status, 200);
        assert_eq!(body["query"].as_str(), Some("git"));
        assert!(body["count"].as_u64().unwrap_or(0) >= 1);
        let servers = body["servers"].as_array().unwrap();
        assert!(servers
            .iter()
            .any(|server| server["name"].as_str() == Some("github")));
        assert!(servers
            .iter()
            .all(|server| server["installCommand"].as_str().is_some()));

        let (empty_status, empty_body) =
            route_request(&req("GET", "/servers/suggest?limit=3"), &registry, &state);
        assert_eq!(empty_status, 200);
        assert_eq!(empty_body["count"].as_u64(), Some(3));
    }

    #[test]
    fn route_request_supports_facets_endpoint() {
        let registry = Registry::from_seed();
        let state = test_state();

        let (status, body) = route_request(
            &req("GET", "/servers/facets?q=git&platform=macos"),
            &registry,
            &state,
        );
        assert_eq!(status, 200);
        assert_eq!(body["query"].as_str(), Some("git"));
        assert!(body["total"].as_u64().unwrap_or(0) >= 1);

        let categories = body["facets"]["categories"].as_array().unwrap();
        assert!(categories
            .iter()
            .any(|item| item["value"].as_str() == Some("developer-tools")));
        let platforms = body["facets"]["platforms"].as_array().unwrap();
        assert!(platforms
            .iter()
            .any(|item| item["value"].as_str() == Some("macos")));
        let trust_levels = body["facets"]["trustLevels"].as_array().unwrap();
        assert!(trust_levels
            .iter()
            .any(|item| item["count"].as_u64().unwrap_or(0) >= 1));
    }

    #[test]
    fn route_request_supports_options_preflight() {
        let registry = Registry::from_seed();
        let state = test_state();
        let (status, body) = route_request(&req("OPTIONS", "/servers"), &registry, &state);
        assert_eq!(status, 200);
        assert_eq!(body["status"].as_str(), Some("ok"));
        assert_eq!(body["path"].as_str(), Some("/servers"));
        assert!(body["methods"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m.as_str() == Some("OPTIONS")));
    }

    #[test]
    fn route_request_handles_star_and_report_endpoints() {
        let registry = Registry::from_seed();
        let state = test_state();

        let (star_status, star_body) =
            route_request(&req("POST", "/servers/github/star"), &registry, &state);
        assert_eq!(star_status, 200);
        assert_eq!(star_body["stars"].as_u64(), Some(1));

        let report_req = HttpRequest {
            method: "POST".to_string(),
            target: "/servers/github/report".to_string(),
            body: "{\"reason\":\"spam\",\"details\":\"bad output\"}".to_string(),
        };
        let (report_status, report_body) = route_request(&report_req, &registry, &state);
        assert_eq!(report_status, 200);
        assert_eq!(report_body["status"].as_str(), Some("received"));
        assert_eq!(report_body["reports"].as_u64(), Some(1));

        let (community_status, community_body) =
            route_request(&req("GET", "/servers/github/community"), &registry, &state);
        assert_eq!(community_status, 200);
        assert_eq!(community_body["stars"].as_u64(), Some(1));
        assert_eq!(community_body["reports"].as_u64(), Some(1));

        let (reports_status, reports_body) = route_request(
            &req("GET", "/servers/github/reports?limit=1"),
            &registry,
            &state,
        );
        assert_eq!(reports_status, 200);
        assert_eq!(reports_body["server"].as_str(), Some("github"));
        assert_eq!(reports_body["count"].as_u64(), Some(1));
        assert!(reports_body["total"].as_u64().unwrap_or(0) >= 1);
        assert_eq!(reports_body["reports"][0]["reason"].as_str(), Some("spam"));
    }

    #[test]
    fn route_request_handles_verified_publishers_endpoints() {
        let registry = Registry::from_seed();
        let state = test_state();

        let (initial_status, initial_body) =
            route_request(&req("GET", "/publishers/verified"), &registry, &state);
        assert_eq!(initial_status, 200);
        assert_eq!(initial_body["count"].as_u64(), Some(0));

        let verify_request = HttpRequest {
            method: "POST".to_string(),
            target: "/publishers/verify".to_string(),
            body: "{\"maintainer\":\"Anthropic\"}".to_string(),
        };
        let (verify_status, verify_body) = route_request(&verify_request, &registry, &state);
        assert_eq!(verify_status, 200);
        assert_eq!(verify_body["status"].as_str(), Some("verified"));
        assert_eq!(verify_body["maintainer"].as_str(), Some("anthropic"));
        assert!(verify_body["verifiedPublishers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some("anthropic")));

        let (search_status, search_body) =
            route_request(&req("GET", "/servers?q=github"), &registry, &state);
        assert_eq!(search_status, 200);
        let github = search_body["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|server| server["name"].as_str() == Some("github"))
            .unwrap();
        assert_eq!(github["maintainerVerified"].as_bool(), Some(true));
        assert!(github["badges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|badge| badge.as_str() == Some("verified-publisher")));

        let (detail_status, detail_body) =
            route_request(&req("GET", "/servers/github"), &registry, &state);
        assert_eq!(detail_status, 200);
        assert_eq!(
            detail_body["installCommandCopy"].as_str(),
            Some("berth install github")
        );
        assert!(
            detail_body["permissionsSummary"]["total"]
                .as_u64()
                .unwrap_or(0)
                >= 1
        );
        assert_eq!(detail_body["maintainerVerified"].as_bool(), Some(true));
        assert!(detail_body["qualityScore"].as_u64().unwrap_or(0) > 0);
        assert!(detail_body["readmeUrl"]
            .as_str()
            .unwrap_or_default()
            .contains("github.com"));

        let unverify_request = HttpRequest {
            method: "POST".to_string(),
            target: "/publishers/unverify".to_string(),
            body: "{\"maintainer\":\"Anthropic\"}".to_string(),
        };
        let (unverify_status, unverify_body) = route_request(&unverify_request, &registry, &state);
        assert_eq!(unverify_status, 200);
        assert_eq!(unverify_body["status"].as_str(), Some("unverified"));
        assert_eq!(unverify_body["count"].as_u64(), Some(0));
    }

    #[test]
    fn readme_url_for_repository_handles_github_and_generic_urls() {
        assert_eq!(
            readme_url_for_repository("https://github.com/acme/mcp-github.git"),
            Some("https://github.com/acme/mcp-github/blob/main/README.md".to_string())
        );
        assert_eq!(
            readme_url_for_repository("https://example.com/repo"),
            Some("https://example.com/repo/README.md".to_string())
        );
        assert_eq!(readme_url_for_repository(""), None);
    }
}
