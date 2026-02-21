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
    publish_queue_dir: PathBuf,
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

#[derive(Debug, Clone, Copy)]
struct SiteCatalogUrlParams<'a> {
    search_query: &'a str,
    category: Option<&'a str>,
    platform: Option<&'a str>,
    trust_level: Option<&'a str>,
    sort_by: SortBy,
    sort_order: SortOrder,
    limit: usize,
    offset: usize,
}

#[derive(Debug, Clone, Copy)]
struct SiteReportsUrlParams<'a> {
    server: Option<&'a str>,
    reason: Option<&'a str>,
    limit: usize,
    offset: usize,
}

#[derive(Debug, Clone, Copy)]
struct SiteSubmissionsUrlParams<'a> {
    status: Option<&'a str>,
    server: Option<&'a str>,
    limit: usize,
    offset: usize,
}

#[derive(Debug, Deserialize)]
struct QueueSubmissionFile {
    submitted_at_epoch_secs: u64,
    status: String,
    manifest: QueueManifest,
    #[serde(default)]
    quality_checks: Vec<QueueQualityCheck>,
}

#[derive(Debug, Deserialize)]
struct QueueManifest {
    server: QueueServer,
}

#[derive(Debug, Deserialize)]
struct QueueServer {
    name: String,
    display_name: String,
    version: String,
    maintainer: String,
    category: String,
}

#[derive(Debug, Deserialize)]
struct QueueQualityCheck {
    passed: bool,
}

#[derive(Debug, Deserialize)]
struct PublishSubmissionStatusPayload {
    status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishSubmissionSummary {
    id: String,
    submitted_at_epoch_secs: u64,
    status: String,
    server: PublishSubmissionServerSummary,
    quality_checks_passed: usize,
    quality_checks_total: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishSubmissionServerSummary {
    name: String,
    display_name: String,
    version: String,
    maintainer: String,
    category: String,
}

impl ApiState {
    fn new(community_dir: PathBuf, publish_queue_dir: PathBuf) -> Self {
        Self {
            community_dir,
            publish_queue_dir,
        }
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

    fn publish_queue_dir(&self) -> PathBuf {
        self.publish_queue_dir.clone()
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

    fn list_all_reports(&self) -> Result<Vec<ReportEvent>, String> {
        let reports_dir = self.reports_dir();
        if !reports_dir.exists() {
            return Ok(Vec::new());
        }
        let entries = fs::read_dir(&reports_dir).map_err(|e| {
            format!(
                "failed to read reports directory {}: {e}",
                reports_dir.display()
            )
        })?;

        let mut reports = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| {
                format!(
                    "failed to enumerate reports directory {}: {e}",
                    reports_dir.display()
                )
            })?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("failed to read report file {}: {e}", path.display()))?;
            for (idx, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let event = serde_json::from_str::<ReportEvent>(trimmed).map_err(|e| {
                    format!(
                        "failed to parse report file {} at line {}: {e}",
                        path.display(),
                        idx + 1
                    )
                })?;
                reports.push(event);
            }
        }
        reports.sort_by(|left, right| {
            right
                .timestamp_epoch_secs
                .cmp(&left.timestamp_epoch_secs)
                .then_with(|| left.server.cmp(&right.server))
                .then_with(|| left.reason.cmp(&right.reason))
        });
        Ok(reports)
    }

    fn list_publish_submissions(&self) -> Result<Vec<PublishSubmissionSummary>, String> {
        let queue_dir = self.publish_queue_dir();
        if !queue_dir.exists() {
            return Ok(Vec::new());
        }
        let entries = fs::read_dir(&queue_dir).map_err(|e| {
            format!(
                "failed to read publish queue directory {}: {e}",
                queue_dir.display()
            )
        })?;

        let mut submissions = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| {
                format!(
                    "failed to enumerate publish queue directory {}: {e}",
                    queue_dir.display()
                )
            })?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("failed to read queue file {}: {e}", path.display()))?;
            let payload = serde_json::from_str::<QueueSubmissionFile>(&content)
                .map_err(|e| format!("failed to parse queue file {}: {e}", path.display()))?;

            let id = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string();
            if id.is_empty() {
                continue;
            }
            submissions.push(publish_submission_summary_from_queue_file(id, payload));
        }

        submissions.sort_by(|left, right| {
            right
                .submitted_at_epoch_secs
                .cmp(&left.submitted_at_epoch_secs)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(submissions)
    }

    fn set_publish_submission_status(
        &self,
        submission_id: &str,
        status: &str,
    ) -> Result<Option<PublishSubmissionSummary>, String> {
        if !is_safe_submission_id(submission_id) {
            return Err("invalid submission id".to_string());
        }
        let path = self.publish_queue_dir().join(submission_id);
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| format!("failed to read queue file {}: {e}", path.display()))?;
        let mut value = serde_json::from_str::<Value>(&content)
            .map_err(|e| format!("failed to parse queue file {}: {e}", path.display()))?;
        value["status"] = Value::String(status.to_string());
        value["reviewedAtEpochSecs"] = json!(now_epoch_secs());

        let payload = serde_json::to_string_pretty(&value)
            .map_err(|e| format!("failed to serialize queue file {}: {e}", path.display()))?;
        fs::write(&path, payload)
            .map_err(|e| format!("failed to write queue file {}: {e}", path.display()))?;

        let normalized = serde_json::from_value::<QueueSubmissionFile>(value)
            .map_err(|e| format!("failed to normalize queue file {}: {e}", path.display()))?;
        Ok(Some(publish_submission_summary_from_queue_file(
            submission_id.to_string(),
            normalized,
        )))
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
    let publish_queue_dir =
        paths::publish_queue_dir().unwrap_or_else(|| PathBuf::from(".berth/publish/queue"));
    let state = ApiState::new(community_dir, publish_queue_dir);
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
    if let Some((status, body)) = route_website_request(&request, registry, state) {
        return write_html_response(stream, status, &body);
    }
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

/// Routes browser-facing local website paths (`/site`, `/site/reports`, `/site/submissions`, `/site/servers/<name>`).
fn route_website_request(
    request: &HttpRequest,
    registry: &Registry,
    state: &ApiState,
) -> Option<(u16, String)> {
    let (path, query) = split_path_query(request.target.as_str());
    if path != "/site" && path != "/site/" && !path.starts_with("/site/") {
        return None;
    }

    if request.method != "GET" {
        return Some((405, render_site_not_found_page("method not allowed")));
    }

    match path {
        "/site" | "/site/" => Some((200, render_site_catalog_page(query, registry, state))),
        "/site/reports" | "/site/reports/" => Some((200, render_site_reports_page(query, state))),
        "/site/submissions" | "/site/submissions/" => {
            Some((200, render_site_submissions_page(query, state)))
        }
        _ => {
            let Some(raw_name) = path.strip_prefix("/site/servers/") else {
                return Some((404, render_site_not_found_page(path)));
            };
            if raw_name.is_empty() || raw_name.contains('/') {
                return Some((404, render_site_not_found_page(path)));
            }
            let server_name = url_decode(raw_name);
            Some(render_site_detail_page(&server_name, registry, state))
        }
    }
}

/// Renders the local catalog page backed by in-process registry data.
fn render_site_catalog_page(query: Option<&str>, registry: &Registry, state: &ApiState) -> String {
    let search_query = query_param(query, "q")
        .or_else(|| query_param(query, "query"))
        .map(url_decode)
        .unwrap_or_default();
    let search_query = search_query.trim().to_string();
    let category_filter = query_param(query, "category")
        .map(url_decode)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let platform_filter = query_param(query, "platform")
        .map(url_decode)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let trust_filter = query_param(query, "trustLevel")
        .or_else(|| query_param(query, "trust_level"))
        .map(url_decode)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let default_sort_by = if search_query.is_empty() {
        SortBy::Name
    } else {
        SortBy::Relevance
    };
    let sort_by = parse_sort_by(query, &search_query).unwrap_or(default_sort_by);
    let default_sort_order = if sort_by == SortBy::Relevance {
        SortOrder::Desc
    } else {
        SortOrder::Asc
    };
    let sort_order = parse_sort_order(query, sort_by).unwrap_or(default_sort_order);
    let limit = parse_usize_param(query, "limit").unwrap_or(24).min(100);
    let requested_offset = parse_usize_param(query, "offset").unwrap_or(0);

    let matches_filter = |server: &&ServerMetadata| {
        matches_server_filters(
            server,
            category_filter.as_deref(),
            platform_filter.as_deref(),
            trust_filter.as_deref(),
        )
    };
    let entries: Vec<(&ServerMetadata, Option<u32>)> = if search_query.is_empty() {
        registry
            .list_all()
            .iter()
            .filter(matches_filter)
            .map(|server| (server, None))
            .collect()
    } else {
        registry
            .search(&search_query)
            .into_iter()
            .map(|result| (result.server, Some(result.score)))
            .filter(|(server, _)| {
                matches_server_filters(
                    server,
                    category_filter.as_deref(),
                    platform_filter.as_deref(),
                    trust_filter.as_deref(),
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
            let quality_score = server_quality_score(server, maintainer_verified, stars, reports);
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
    let offset = if total == 0 {
        0
    } else {
        requested_offset.min(((total - 1) / limit) * limit)
    };
    let shown_servers = listed
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let shown_count = shown_servers.len();
    let showing_start = if shown_count == 0 { 0 } else { offset + 1 };
    let showing_end = offset + shown_count;
    let total_pages = if total == 0 {
        1
    } else {
        ((total - 1) / limit) + 1
    };
    let page = if total == 0 { 1 } else { (offset / limit) + 1 };
    let has_prev = offset > 0;
    let has_next = showing_end < total;

    let (_stats_status, stats_payload) = route_stats(Some("top=5"), registry, state);
    let total_servers = stats_payload["servers"]["total"].as_u64().unwrap_or(0);
    let total_downloads = stats_payload["servers"]["downloadsTotal"]
        .as_u64()
        .unwrap_or(0);
    let total_stars = stats_payload["community"]["starsTotal"]
        .as_u64()
        .unwrap_or(0);
    let total_reports = stats_payload["community"]["reportsTotal"]
        .as_u64()
        .unwrap_or(0);

    let mut trending_query_pairs = vec!["limit=5".to_string()];
    if let Some(category) = category_filter.as_deref() {
        trending_query_pairs.push(format!("category={}", url_encode(category)));
    }
    if let Some(platform) = platform_filter.as_deref() {
        trending_query_pairs.push(format!("platform={}", url_encode(platform)));
    }
    if let Some(trust_level) = trust_filter.as_deref() {
        trending_query_pairs.push(format!("trustLevel={}", url_encode(trust_level)));
    }
    let trending_query = trending_query_pairs.join("&");
    let (_trending_status, trending_payload) =
        route_servers_trending(Some(&trending_query), registry, state);
    let trending_servers = trending_payload["servers"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let prev_href = build_site_catalog_path(&SiteCatalogUrlParams {
        search_query: &search_query,
        category: category_filter.as_deref(),
        platform: platform_filter.as_deref(),
        trust_level: trust_filter.as_deref(),
        sort_by,
        sort_order,
        limit,
        offset: offset.saturating_sub(limit),
    });
    let next_href = build_site_catalog_path(&SiteCatalogUrlParams {
        search_query: &search_query,
        category: category_filter.as_deref(),
        platform: platform_filter.as_deref(),
        trust_level: trust_filter.as_deref(),
        sort_by,
        sort_order,
        limit,
        offset: offset.saturating_add(limit),
    });

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

    let mut cards = String::new();
    if shown_servers.is_empty() {
        cards.push_str("<p class=\"empty\">No servers matched your current filters.</p>");
    } else {
        for entry in shown_servers {
            let name = html_escape(&entry.server.name);
            let display_name = html_escape(&entry.server.display_name);
            let description = html_escape(&entry.server.description);
            let category = html_escape(&entry.server.category);
            let trust_level = html_escape(&entry.server.trust_level.to_string());
            let maintainer = html_escape(&entry.server.maintainer);
            let install_command = format!("berth install {}", entry.server.name);
            let install_display = html_escape(&install_command);
            let verified_badge = if entry.maintainer_verified {
                "<span class=\"badge badge-verified\">verified maintainer</span>"
            } else {
                ""
            };
            let related_link = format!("/site/servers/{name}");
            cards.push_str("<article class=\"card\">");
            cards.push_str(&format!(
                "<h3><a href=\"{related_link}\">{display_name}</a></h3>"
            ));
            cards.push_str(&format!("<p class=\"description\">{description}</p>"));
            cards.push_str("<p class=\"meta\">");
            cards.push_str(&format!(
                "<span>{category}</span><span>{trust_level}</span><span>quality {}</span>",
                entry.quality_score
            ));
            cards.push_str("</p>");
            cards.push_str("<p class=\"meta\">");
            cards.push_str(&format!(
                "<span>maintainer {maintainer}</span><span>downloads {}</span><span>stars {}</span><span>reports {}</span>{verified_badge}",
                format_number(entry.server.quality.downloads),
                entry.stars,
                entry.reports
            ));
            cards.push_str("</p>");
            cards.push_str("<div class=\"install-row\">");
            cards.push_str(&format!("<code>{install_display}</code>"));
            cards.push_str(&format!(
                "<button class=\"copy-btn\" data-copy=\"{install_display}\">Copy</button>"
            ));
            cards.push_str("</div>");
            cards.push_str("</article>");
        }
    }

    let search_input = html_escape(&search_query);
    let selected_category = category_filter.as_deref().unwrap_or_default();
    let selected_platform = platform_filter.as_deref().unwrap_or_default();
    let selected_trust = trust_filter.as_deref().unwrap_or_default();
    let mut sort_options = String::new();
    for (value, label) in [
        ("relevance", "Relevance"),
        ("name", "Name"),
        ("downloads", "Downloads"),
        ("stars", "Stars"),
        ("reports", "Reports"),
        ("qualityScore", "Quality"),
    ] {
        let selected = if sort_by.as_str() == value {
            " selected"
        } else {
            ""
        };
        sort_options.push_str(&format!(
            "<option value=\"{value}\"{selected}>{label}</option>"
        ));
    }
    let mut order_options = String::new();
    for (value, label) in [("asc", "Ascending"), ("desc", "Descending")] {
        let selected = if sort_order.as_str() == value {
            " selected"
        } else {
            ""
        };
        order_options.push_str(&format!(
            "<option value=\"{value}\"{selected}>{label}</option>"
        ));
    }

    let mut content = String::new();
    content.push_str("<header class=\"hero\">");
    content.push_str("<p class=\"kicker\">Berth Registry</p>");
    content.push_str("<h1>Server Catalog</h1>");
    content.push_str("<p>Browse MCP servers with quality, permission, and community signals from your local Berth registry API.</p>");
    content.push_str("</header>");

    content.push_str("<section class=\"panel overview-panel\">");
    content.push_str("<h2>Overview</h2>");
    content.push_str("<div class=\"overview-grid\">");
    content.push_str(&format!(
        "<div class=\"metric\"><span class=\"metric-label\">Servers</span><strong>{}</strong></div>",
        format_number(total_servers)
    ));
    content.push_str(&format!(
        "<div class=\"metric\"><span class=\"metric-label\">Downloads</span><strong>{}</strong></div>",
        format_number(total_downloads)
    ));
    content.push_str(&format!(
        "<div class=\"metric\"><span class=\"metric-label\">Stars</span><strong>{}</strong></div>",
        format_number(total_stars)
    ));
    content.push_str(&format!(
        "<div class=\"metric\"><span class=\"metric-label\">Reports</span><strong>{}</strong></div>",
        format_number(total_reports)
    ));
    content.push_str("</div>");
    if !trending_servers.is_empty() {
        content.push_str("<h3>Trending Right Now</h3>");
        content.push_str("<ul class=\"trending-list\">");
        for server in trending_servers.into_iter().take(5) {
            let name = server["name"].as_str().unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let display_name = server["displayName"].as_str().unwrap_or(name);
            let trend_score = server["trendScore"].as_u64().unwrap_or(0);
            let stars = server["stars"].as_u64().unwrap_or(0);
            content.push_str("<li>");
            content.push_str(&format!(
                "<a href=\"/site/servers/{}\">{}</a> <span class=\"meta\">trend {} · stars {}</span>",
                html_escape(name),
                html_escape(display_name),
                trend_score,
                stars
            ));
            content.push_str("</li>");
        }
        content.push_str("</ul>");
    }
    content.push_str("<p><a href=\"/site/reports\">Open moderation reports feed</a></p>");
    content.push_str("<p><a href=\"/site/submissions\">Open publish review queue</a></p>");
    content.push_str("</section>");

    content.push_str("<section class=\"panel\">");
    content.push_str("<form class=\"filters\" method=\"GET\" action=\"/site\">");
    content.push_str("<label>Query<input type=\"text\" name=\"q\" placeholder=\"github\" value=\"");
    content.push_str(&search_input);
    content.push_str("\"></label>");
    content.push_str("<label>Category<select name=\"category\">");
    content.push_str("<option value=\"\">All</option>");
    content.push_str(&render_site_filter_options(&categories, selected_category));
    content.push_str("</select></label>");
    content.push_str("<label>Platform<select name=\"platform\">");
    content.push_str("<option value=\"\">All</option>");
    content.push_str(&render_site_filter_options(&platforms, selected_platform));
    content.push_str("</select></label>");
    content.push_str("<label>Trust<select name=\"trustLevel\">");
    content.push_str("<option value=\"\">All</option>");
    content.push_str(&render_site_filter_options(&trust_levels, selected_trust));
    content.push_str("</select></label>");
    content.push_str("<label>Sort<select name=\"sortBy\">");
    content.push_str(&sort_options);
    content.push_str("</select></label>");
    content.push_str("<label>Order<select name=\"order\">");
    content.push_str(&order_options);
    content.push_str("</select></label>");
    content.push_str("<input type=\"hidden\" name=\"offset\" value=\"0\">");
    content.push_str(
        "<label>Limit<input type=\"number\" min=\"1\" max=\"100\" name=\"limit\" value=\"",
    );
    content.push_str(&limit.to_string());
    content.push_str("\"></label>");
    content.push_str("<button type=\"submit\">Apply</button>");
    content.push_str("</form>");
    content.push_str(&format!(
        "<p class=\"summary\">Showing <strong>{showing_start}-{showing_end}</strong> of <strong>{total}</strong> servers (page <strong>{page}</strong> of <strong>{total_pages}</strong>).</p>",
    ));
    content.push_str("<div class=\"pagination\">");
    if has_prev {
        content.push_str(&format!(
            "<a href=\"{}\">Previous</a>",
            html_escape(&prev_href)
        ));
    } else {
        content.push_str("<span class=\"pagination-disabled\">Previous</span>");
    }
    if has_next {
        content.push_str(&format!("<a href=\"{}\">Next</a>", html_escape(&next_href)));
    } else {
        content.push_str("<span class=\"pagination-disabled\">Next</span>");
    }
    content.push_str("</div>");
    content.push_str("</section>");
    content.push_str("<section class=\"catalog\">");
    content.push_str(&cards);
    content.push_str("</section>");

    render_site_shell("Berth Registry Catalog", &content)
}

/// Renders a global moderation reports feed at `/site/reports`.
fn render_site_reports_page(query: Option<&str>, state: &ApiState) -> String {
    let server_filter = query_param(query, "server")
        .map(url_decode)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let reason_filter = query_param(query, "reason")
        .map(url_decode)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let limit = parse_usize_param(query, "limit")
        .unwrap_or(25)
        .clamp(1, 200);
    let requested_offset = parse_usize_param(query, "offset").unwrap_or(0);

    let mut report_query_pairs = vec![
        format!("limit={limit}"),
        format!("offset={requested_offset}"),
    ];
    if let Some(server) = server_filter.as_deref() {
        report_query_pairs.push(format!("server={}", url_encode(server)));
    }
    if let Some(reason) = reason_filter.as_deref() {
        report_query_pairs.push(format!("reason={}", url_encode(reason)));
    }
    let report_query = report_query_pairs.join("&");
    let (_status, payload) = route_reports(Some(&report_query), state);

    let total = payload["total"].as_u64().unwrap_or(0) as usize;
    let offset = payload["offset"].as_u64().unwrap_or(0) as usize;
    let limit = payload["limit"].as_u64().unwrap_or(limit as u64) as usize;
    let count = payload["count"].as_u64().unwrap_or(0) as usize;
    let showing_start = if count == 0 { 0 } else { offset + 1 };
    let showing_end = offset + count;
    let has_prev = offset > 0;
    let has_next = showing_end < total;
    let reports = payload["reports"].as_array().cloned().unwrap_or_default();

    let prev_href = build_site_reports_path(&SiteReportsUrlParams {
        server: server_filter.as_deref(),
        reason: reason_filter.as_deref(),
        limit,
        offset: offset.saturating_sub(limit),
    });
    let next_href = build_site_reports_path(&SiteReportsUrlParams {
        server: server_filter.as_deref(),
        reason: reason_filter.as_deref(),
        limit,
        offset: offset.saturating_add(limit),
    });

    let mut content = String::new();
    content.push_str("<header class=\"hero\">");
    content.push_str("<p class=\"kicker\"><a href=\"/site\">Back to catalog</a></p>");
    content.push_str("<h1>Moderation Reports Feed</h1>");
    content.push_str(
        "<p>Review recent community reports across all servers from your local registry state.</p>",
    );
    content.push_str("</header>");

    content.push_str("<section class=\"panel\">");
    content.push_str("<form class=\"filters\" method=\"GET\" action=\"/site/reports\">");
    content.push_str(
        "<label>Server<input type=\"text\" name=\"server\" placeholder=\"github\" value=\"",
    );
    content.push_str(&html_escape(server_filter.as_deref().unwrap_or_default()));
    content.push_str("\"></label>");
    content.push_str(
        "<label>Reason<input type=\"text\" name=\"reason\" placeholder=\"spam\" value=\"",
    );
    content.push_str(&html_escape(reason_filter.as_deref().unwrap_or_default()));
    content.push_str("\"></label>");
    content.push_str("<input type=\"hidden\" name=\"offset\" value=\"0\">");
    content.push_str(
        "<label>Limit<input type=\"number\" min=\"1\" max=\"200\" name=\"limit\" value=\"",
    );
    content.push_str(&limit.to_string());
    content.push_str("\"></label>");
    content.push_str("<button type=\"submit\">Apply</button>");
    content.push_str("</form>");
    content.push_str(&format!(
        "<p class=\"summary\">Showing <strong>{showing_start}-{showing_end}</strong> of <strong>{total}</strong> reports.</p>",
    ));
    content.push_str("<div class=\"pagination\">");
    if has_prev {
        content.push_str(&format!(
            "<a href=\"{}\">Previous</a>",
            html_escape(&prev_href)
        ));
    } else {
        content.push_str("<span class=\"pagination-disabled\">Previous</span>");
    }
    if has_next {
        content.push_str(&format!("<a href=\"{}\">Next</a>", html_escape(&next_href)));
    } else {
        content.push_str("<span class=\"pagination-disabled\">Next</span>");
    }
    content.push_str("</div>");
    content.push_str("</section>");

    content.push_str("<section class=\"panel\">");
    content.push_str("<h2>Reports</h2>");
    if reports.is_empty() {
        content.push_str("<p class=\"empty\">No reports matched the current filters.</p>");
    } else {
        content.push_str("<ul class=\"report-list\">");
        for report in reports {
            let server = report["server"].as_str().unwrap_or_default();
            let reason = report["reason"].as_str().unwrap_or("unspecified");
            let details = report["details"]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("No details provided.");
            let epoch = report["timestampEpochSecs"].as_u64().unwrap_or(0);
            let server_href = format!("/site/servers/{}", url_encode(server));
            content.push_str("<li>");
            content.push_str(&format!(
                "<span class=\"meta\"><a href=\"{}\">{}</a> · reason {} · epoch {}</span><p>{}</p>",
                html_escape(&server_href),
                html_escape(server),
                html_escape(reason),
                epoch,
                html_escape(details)
            ));
            content.push_str("</li>");
        }
        content.push_str("</ul>");
    }
    content.push_str("</section>");

    render_site_shell("Berth Registry Reports", &content)
}

/// Renders a publish-review queue page at `/site/submissions`.
fn render_site_submissions_page(query: Option<&str>, state: &ApiState) -> String {
    let status_filter = query_param(query, "status")
        .map(url_decode)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let server_filter = query_param(query, "server")
        .map(url_decode)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let limit = parse_usize_param(query, "limit")
        .unwrap_or(25)
        .clamp(1, 200);
    let requested_offset = parse_usize_param(query, "offset").unwrap_or(0);

    let mut submission_query_pairs = vec![
        format!("limit={limit}"),
        format!("offset={requested_offset}"),
    ];
    if let Some(status) = status_filter.as_deref() {
        submission_query_pairs.push(format!("status={}", url_encode(status)));
    }
    if let Some(server) = server_filter.as_deref() {
        submission_query_pairs.push(format!("server={}", url_encode(server)));
    }
    let submission_query = submission_query_pairs.join("&");
    let (_status, payload) = route_publish_submissions(Some(&submission_query), state);

    let total = payload["total"].as_u64().unwrap_or(0) as usize;
    let offset = payload["offset"].as_u64().unwrap_or(0) as usize;
    let limit = payload["limit"].as_u64().unwrap_or(limit as u64) as usize;
    let count = payload["count"].as_u64().unwrap_or(0) as usize;
    let showing_start = if count == 0 { 0 } else { offset + 1 };
    let showing_end = offset + count;
    let has_prev = offset > 0;
    let has_next = showing_end < total;
    let submissions = payload["submissions"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let prev_href = build_site_submissions_path(&SiteSubmissionsUrlParams {
        status: status_filter.as_deref(),
        server: server_filter.as_deref(),
        limit,
        offset: offset.saturating_sub(limit),
    });
    let next_href = build_site_submissions_path(&SiteSubmissionsUrlParams {
        status: status_filter.as_deref(),
        server: server_filter.as_deref(),
        limit,
        offset: offset.saturating_add(limit),
    });

    let mut content = String::new();
    content.push_str("<header class=\"hero\">");
    content.push_str("<p class=\"kicker\"><a href=\"/site\">Back to catalog</a></p>");
    content.push_str("<h1>Publish Review Queue</h1>");
    content.push_str(
        "<p>Browse locally queued submissions produced by `berth publish` for manual review.</p>",
    );
    content.push_str("</header>");

    content.push_str("<section class=\"panel\">");
    content.push_str("<form class=\"filters\" method=\"GET\" action=\"/site/submissions\">");
    content.push_str(
        "<label>Status<input type=\"text\" name=\"status\" placeholder=\"pending-manual-review\" value=\"",
    );
    content.push_str(&html_escape(status_filter.as_deref().unwrap_or_default()));
    content.push_str("\"></label>");
    content.push_str(
        "<label>Server<input type=\"text\" name=\"server\" placeholder=\"github\" value=\"",
    );
    content.push_str(&html_escape(server_filter.as_deref().unwrap_or_default()));
    content.push_str("\"></label>");
    content.push_str("<input type=\"hidden\" name=\"offset\" value=\"0\">");
    content.push_str(
        "<label>Limit<input type=\"number\" min=\"1\" max=\"200\" name=\"limit\" value=\"",
    );
    content.push_str(&limit.to_string());
    content.push_str("\"></label>");
    content.push_str("<button type=\"submit\">Apply</button>");
    content.push_str("</form>");
    content.push_str(&format!(
        "<p class=\"summary\">Showing <strong>{showing_start}-{showing_end}</strong> of <strong>{total}</strong> submissions.</p>",
    ));
    content.push_str("<div class=\"pagination\">");
    if has_prev {
        content.push_str(&format!(
            "<a href=\"{}\">Previous</a>",
            html_escape(&prev_href)
        ));
    } else {
        content.push_str("<span class=\"pagination-disabled\">Previous</span>");
    }
    if has_next {
        content.push_str(&format!("<a href=\"{}\">Next</a>", html_escape(&next_href)));
    } else {
        content.push_str("<span class=\"pagination-disabled\">Next</span>");
    }
    content.push_str("</div>");
    content.push_str("</section>");

    content.push_str("<section class=\"panel\">");
    content.push_str("<h2>Submissions</h2>");
    if submissions.is_empty() {
        content.push_str("<p class=\"empty\">No submissions matched the current filters.</p>");
    } else {
        content.push_str("<ul class=\"related-list\">");
        for submission in submissions {
            let id = submission["id"].as_str().unwrap_or_default();
            let status = submission["status"].as_str().unwrap_or("unknown");
            let epoch = submission["submittedAtEpochSecs"].as_u64().unwrap_or(0);
            let quality_passed = submission["qualityChecksPassed"].as_u64().unwrap_or(0);
            let quality_total = submission["qualityChecksTotal"].as_u64().unwrap_or(0);

            let server_name = submission["server"]["name"].as_str().unwrap_or_default();
            let server_display_name = submission["server"]["displayName"]
                .as_str()
                .unwrap_or(server_name);
            let server_version = submission["server"]["version"].as_str().unwrap_or_default();
            let server_maintainer = submission["server"]["maintainer"]
                .as_str()
                .unwrap_or_default();
            let server_href = format!("/site/servers/{}", url_encode(server_name));

            content.push_str("<li>");
            content.push_str(&format!(
                "<a href=\"{}\">{}</a> <span class=\"meta\">status {} · submitted {} · quality {}/{} · id {}</span><p>{} v{} · maintainer {}</p>",
                html_escape(&server_href),
                html_escape(server_display_name),
                html_escape(status),
                epoch,
                quality_passed,
                quality_total,
                html_escape(id),
                html_escape(server_name),
                html_escape(server_version),
                html_escape(server_maintainer)
            ));
            content.push_str("</li>");
        }
        content.push_str("</ul>");
    }
    content.push_str("</section>");

    render_site_shell("Berth Registry Publish Queue", &content)
}

/// Renders a server detail page at `/site/servers/<name>`.
fn render_site_detail_page(
    server_name: &str,
    registry: &Registry,
    state: &ApiState,
) -> (u16, String) {
    let Some(server) = registry.get(server_name) else {
        return (404, render_site_not_found_page(server_name));
    };

    let (stars, reports) = state.community_counts(&server.name).unwrap_or((0, 0));
    let maintainer_verified = state
        .is_publisher_verified(&server.maintainer)
        .unwrap_or(false);
    let quality_score = server_quality_score(server, maintainer_verified, stars, reports);
    let install_command = format!("berth install {}", server.name);
    let readme_url = readme_url_for_repository(&server.source.repository);
    let permissions = permissions_summary(server);
    let recent_reports = state
        .list_reports(&server.name)
        .unwrap_or_default()
        .into_iter()
        .take(5)
        .collect::<Vec<_>>();
    let related = route_server_related(server, Some("limit=4"), registry, state);
    let related_servers = if related.0 == 200 {
        related.1["servers"].as_array().cloned().unwrap_or_default()
    } else {
        Vec::new()
    };

    let mut compatibility_clients = server.compatibility.clients.clone();
    compatibility_clients.sort();
    let mut compatibility_platforms = server.compatibility.platforms.clone();
    compatibility_platforms.sort();

    let mut content = String::new();
    content.push_str("<header class=\"hero\">");
    content.push_str("<p class=\"kicker\"><a href=\"/site\">Back to catalog</a></p>");
    content.push_str(&format!("<h1>{}</h1>", html_escape(&server.display_name)));
    content.push_str(&format!("<p>{}</p>", html_escape(&server.description)));
    content.push_str("<p class=\"meta\">");
    content.push_str(&format!(
        "<span>{}</span><span>{}</span><span>maintainer {}</span><span>quality {quality_score}</span>",
        html_escape(&server.category),
        html_escape(&server.trust_level.to_string()),
        html_escape(&server.maintainer)
    ));
    if maintainer_verified {
        content.push_str("<span class=\"badge badge-verified\">verified maintainer</span>");
    }
    content.push_str("</p>");
    content.push_str("</header>");

    content.push_str("<section class=\"panel\">");
    content.push_str("<h2>Install Command</h2>");
    let install_display = html_escape(&install_command);
    content.push_str("<div class=\"install-row\">");
    content.push_str(&format!("<code>{install_display}</code>"));
    content.push_str(&format!(
        "<button class=\"copy-btn\" data-copy=\"{install_display}\">Copy</button>"
    ));
    content.push_str("</div>");
    content.push_str("<p class=\"meta\">");
    content.push_str(&format!(
        "<span>downloads {}</span><span>stars {stars}</span><span>reports {reports}</span><span>version {}</span>",
        format_number(server.quality.downloads),
        html_escape(&server.version)
    ));
    content.push_str("</p>");
    if let Some(readme_url) = readme_url {
        let readme_url = html_escape(&readme_url);
        content.push_str(&format!("<p><a href=\"{readme_url}\">Open README</a></p>"));
    }
    content.push_str("</section>");

    content.push_str("<section class=\"panel detail-grid\">");
    content.push_str("<div>");
    content.push_str("<h2>Permissions</h2>");
    content.push_str(&render_site_permissions_list(
        "Network",
        &server.permissions.network,
    ));
    content.push_str(&render_site_permissions_list(
        "Env",
        &server.permissions.env,
    ));
    content.push_str(&render_site_permissions_list(
        "Filesystem",
        &server.permissions.filesystem,
    ));
    content.push_str(&render_site_permissions_list(
        "Exec",
        &server.permissions.exec,
    ));
    content.push_str("</div>");
    content.push_str("<div>");
    content.push_str("<h2>Compatibility</h2>");
    content.push_str("<p class=\"meta\"><strong>Clients:</strong> ");
    content.push_str(&html_escape(&compatibility_clients.join(", ")));
    content.push_str("</p>");
    content.push_str("<p class=\"meta\"><strong>Platforms:</strong> ");
    content.push_str(&html_escape(&compatibility_platforms.join(", ")));
    content.push_str("</p>");
    content.push_str("<h2>Security Summary</h2>");
    content.push_str("<p class=\"meta\">");
    content.push_str(&format!(
        "<span>permissions {}</span><span>network wildcard {}</span><span>filesystem write {}</span>",
        permissions["total"].as_u64().unwrap_or(0),
        permissions["network"]["wildcard"].as_bool().unwrap_or(false),
        permissions["filesystem"]["hasWriteAccess"]
            .as_bool()
            .unwrap_or(false)
    ));
    content.push_str("</p>");
    content.push_str("</div>");
    content.push_str("</section>");

    let escaped_server_name = html_escape(&server.name);
    content.push_str("<section class=\"panel\">");
    content.push_str("<h2>Community</h2>");
    content.push_str(&format!(
        "<p class=\"meta\" data-community-counts data-stars=\"{stars}\" data-reports=\"{reports}\"><span id=\"community-stars\">stars {stars}</span><span id=\"community-reports\">reports {reports}</span></p>"
    ));
    content.push_str("<div class=\"community-actions\">");
    content.push_str(&format!(
        "<button type=\"button\" class=\"star-btn\" data-star-server=\"{escaped_server_name}\">Star this server</button>"
    ));
    content.push_str("</div>");
    content.push_str(&format!(
        "<form class=\"report-form\" data-report-server=\"{escaped_server_name}\">"
    ));
    content.push_str(
        "<label>Report reason<input type=\"text\" name=\"reason\" placeholder=\"spam\"></label>",
    );
    content.push_str("<label>Details<textarea name=\"details\" rows=\"3\" placeholder=\"Describe the issue\"></textarea></label>");
    content.push_str("<button type=\"submit\">Submit report</button>");
    content.push_str("</form>");
    content.push_str("<p class=\"meta\" data-report-status></p>");
    content.push_str("<h3>Recent Reports</h3>");
    let report_empty_style = if recent_reports.is_empty() {
        ""
    } else {
        " style=\"display:none\""
    };
    content.push_str(&format!(
        "<p class=\"meta\" data-report-empty{report_empty_style}>No reports submitted.</p>"
    ));
    content.push_str("<ul class=\"report-list\" data-report-list>");
    for event in recent_reports {
        let reason = html_escape(&event.reason);
        let details = if event.details.trim().is_empty() {
            "No details provided.".to_string()
        } else {
            html_escape(&event.details)
        };
        content.push_str("<li>");
        content.push_str(&format!(
            "<span class=\"meta\">reason {reason} · epoch {}</span><p>{details}</p>",
            event.timestamp_epoch_secs
        ));
        content.push_str("</li>");
    }
    content.push_str("</ul>");
    content.push_str("</section>");

    if !related_servers.is_empty() {
        content.push_str("<section class=\"panel\">");
        content.push_str("<h2>Related Servers</h2>");
        content.push_str("<ul class=\"related-list\">");
        for server in related_servers {
            let name = server["name"].as_str().unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            let display_name = server["displayName"].as_str().unwrap_or(name);
            let description = server["description"].as_str().unwrap_or_default();
            let quality = server["qualityScore"].as_u64().unwrap_or(0);
            let stars = server["stars"].as_u64().unwrap_or(0);
            content.push_str("<li>");
            content.push_str(&format!(
                "<a href=\"/site/servers/{}\">{}</a> <span class=\"meta\">quality {} · stars {}</span><p>{}</p>",
                html_escape(name),
                html_escape(display_name),
                quality,
                stars,
                html_escape(description)
            ));
            content.push_str("</li>");
        }
        content.push_str("</ul>");
        content.push_str("</section>");
    }

    (200, render_site_shell("Berth Registry Detail", &content))
}

/// Renders a basic not-found page for website routes.
fn render_site_not_found_page(path: &str) -> String {
    let mut content = String::new();
    content.push_str("<header class=\"hero\">");
    content.push_str("<p class=\"kicker\">Berth Registry</p>");
    content.push_str("<h1>Page Not Found</h1>");
    content.push_str(&format!(
        "<p>The requested path <code>{}</code> is not available.</p>",
        html_escape(path)
    ));
    content.push_str("<p><a href=\"/site\">Open catalog</a></p>");
    content.push_str("</header>");
    render_site_shell("Not Found", &content)
}

/// Renders page shell with styles and shared copy-to-clipboard behavior.
fn render_site_shell(title: &str, content: &str) -> String {
    let mut page = String::new();
    page.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    page.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    page.push_str("<title>");
    page.push_str(&html_escape(title));
    page.push_str("</title><style>");
    page.push_str(
        r#"
:root {
  --bg: #ecf4eb;
  --surface: #ffffff;
  --ink: #11281c;
  --muted: #4a6256;
  --accent: #0f7a45;
  --accent-soft: #d6efdf;
  --line: #c3d6c9;
}
* { box-sizing: border-box; }
body {
  margin: 0;
  font-family: "Trebuchet MS", "Gill Sans", sans-serif;
  color: var(--ink);
  background: radial-gradient(circle at 20% 0%, #f9fff7 0, var(--bg) 40%, #d8e9da 100%);
}
a { color: #005f34; }
.page { max-width: 1100px; margin: 0 auto; padding: 1.5rem 1rem 2rem; }
.hero {
  background: linear-gradient(140deg, #123422 0%, #0b5d34 50%, #28955f 100%);
  color: #f3fff7;
  border-radius: 14px;
  padding: 1.2rem 1.25rem;
  box-shadow: 0 16px 40px rgba(13, 58, 37, 0.25);
}
.hero h1 { margin: 0.2rem 0 0.5rem; }
.hero p { margin: 0.2rem 0; }
.hero a { color: #d8f9e4; }
.kicker {
  letter-spacing: 0.08em;
  text-transform: uppercase;
  font-size: 0.75rem;
  opacity: 0.9;
}
.panel {
  background: var(--surface);
  border: 1px solid var(--line);
  border-radius: 12px;
  padding: 1rem;
  margin-top: 1rem;
}
.overview-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(130px, 1fr));
  gap: 0.6rem;
  margin-top: 0.55rem;
}
.metric {
  border: 1px solid var(--line);
  border-radius: 10px;
  padding: 0.55rem 0.65rem;
  background: #f8fdf7;
}
.metric-label {
  display: block;
  font-size: 0.74rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--muted);
}
.trending-list {
  margin: 0.6rem 0 0;
  padding-left: 1rem;
}
.trending-list li {
  margin: 0.45rem 0;
}
.filters {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(155px, 1fr));
  gap: 0.75rem;
  align-items: end;
}
label {
  display: flex;
  flex-direction: column;
  gap: 0.3rem;
  font-size: 0.86rem;
  color: var(--muted);
}
input, select, button {
  font: inherit;
  border-radius: 8px;
  border: 1px solid var(--line);
  padding: 0.5rem 0.6rem;
}
button {
  border: none;
  background: var(--accent);
  color: #f4fff8;
  cursor: pointer;
  font-weight: 700;
}
button:hover { filter: brightness(1.06); }
.summary { margin: 0.8rem 0 0; color: var(--muted); }
.pagination {
  margin-top: 0.75rem;
  display: flex;
  gap: 0.6rem;
  align-items: center;
}
.pagination a,
.pagination span {
  border-radius: 999px;
  padding: 0.25rem 0.6rem;
  font-size: 0.82rem;
}
.pagination a {
  text-decoration: none;
  background: #e1f3e5;
}
.pagination-disabled {
  color: #7a9185;
  background: #edf4ee;
}
.catalog {
  margin-top: 1rem;
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: 0.85rem;
}
.card {
  background: var(--surface);
  border: 1px solid var(--line);
  border-radius: 12px;
  padding: 0.9rem;
  box-shadow: 0 6px 18px rgba(20, 47, 32, 0.08);
}
.card h3 { margin: 0 0 0.4rem; }
.description { color: #1d3e2d; margin: 0 0 0.5rem; }
.meta {
  display: flex;
  gap: 0.55rem;
  flex-wrap: wrap;
  font-size: 0.82rem;
  color: var(--muted);
}
.badge {
  display: inline-flex;
  align-items: center;
  border-radius: 999px;
  padding: 0.1rem 0.45rem;
  font-size: 0.73rem;
  background: #edf2ff;
}
.badge-verified {
  background: var(--accent-soft);
  color: #0f5d34;
}
.install-row {
  margin-top: 0.65rem;
  display: flex;
  gap: 0.55rem;
  align-items: center;
}
code {
  background: #f0f7ef;
  border: 1px solid #d0e2d3;
  border-radius: 8px;
  padding: 0.32rem 0.45rem;
  font-size: 0.82rem;
  overflow-x: auto;
}
.copy-btn {
  background: #165f3a;
  white-space: nowrap;
}
.copy-btn.copied {
  background: #0f8f50;
}
.community-actions {
  margin-top: 0.35rem;
}
.star-btn {
  background: #0b7541;
}
.star-btn.starred {
  background: #09975a;
}
.report-form {
  margin-top: 0.7rem;
  display: grid;
  gap: 0.6rem;
  grid-template-columns: 1fr;
}
textarea {
  font: inherit;
  border-radius: 8px;
  border: 1px solid var(--line);
  padding: 0.5rem 0.6rem;
  resize: vertical;
}
.report-list {
  margin: 0.6rem 0 0;
  padding-left: 1rem;
}
.report-list li {
  margin: 0.6rem 0;
}
.detail-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
  gap: 0.9rem;
}
.perm-list { margin: 0 0 0.75rem; padding-left: 1rem; }
.perm-list li { margin: 0.2rem 0; }
.related-list { padding-left: 1rem; }
.related-list li { margin: 0.7rem 0; }
.empty {
  padding: 1rem;
  border-radius: 12px;
  border: 1px dashed var(--line);
  background: #f8fef6;
}
@media (max-width: 720px) {
  .install-row { flex-direction: column; align-items: stretch; }
  button { width: 100%; }
}
"#,
    );
    page.push_str("</style></head><body><main class=\"page\">");
    page.push_str(content);
    page.push_str("</main><script>");
    page.push_str(
        r#"
for (const button of document.querySelectorAll(".copy-btn")) {
  button.addEventListener("click", async () => {
    const text = button.getAttribute("data-copy") || "";
    try {
      await navigator.clipboard.writeText(text);
      const oldLabel = button.textContent;
      button.textContent = "Copied";
      button.classList.add("copied");
      setTimeout(() => {
        button.textContent = oldLabel;
        button.classList.remove("copied");
      }, 1100);
    } catch (_) {
      button.textContent = "Copy failed";
    }
  });
}

function applyCommunityCounts(payload) {
  const starsText = document.getElementById("community-stars");
  const reportsText = document.getElementById("community-reports");
  if (starsText && typeof payload.stars === "number") {
    starsText.textContent = `stars ${payload.stars}`;
  }
  if (reportsText && typeof payload.reports === "number") {
    reportsText.textContent = `reports ${payload.reports}`;
  }
}

function renderRecentReports(reports) {
  const reportList = document.querySelector("[data-report-list]");
  const reportEmpty = document.querySelector("[data-report-empty]");
  if (!reportList || !reportEmpty) return;

  reportList.innerHTML = "";
  if (!Array.isArray(reports) || reports.length === 0) {
    reportEmpty.style.display = "";
    return;
  }
  reportEmpty.style.display = "none";

  for (const report of reports) {
    const listItem = document.createElement("li");
    const meta = document.createElement("span");
    meta.className = "meta";
    const reason = typeof report?.reason === "string" && report.reason.trim()
      ? report.reason.trim()
      : "unspecified";
    const epochValue = Number(report?.timestampEpochSecs);
    const epoch = Number.isFinite(epochValue) ? Math.trunc(epochValue) : 0;
    meta.textContent = `reason ${reason} · epoch ${epoch}`;

    const detailsText = typeof report?.details === "string" && report.details.trim()
      ? report.details.trim()
      : "No details provided.";
    const details = document.createElement("p");
    details.textContent = detailsText;

    listItem.appendChild(meta);
    listItem.appendChild(details);
    reportList.appendChild(listItem);
  }
}

const starButton = document.querySelector("[data-star-server]");
if (starButton) {
  starButton.addEventListener("click", async () => {
    const server = starButton.getAttribute("data-star-server");
    if (!server) return;
    try {
      const response = await fetch(`/servers/${encodeURIComponent(server)}/star`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: "{}"
      });
      if (!response.ok) throw new Error();
      const payload = await response.json();
      applyCommunityCounts({ stars: payload.stars });
      starButton.textContent = "Star recorded";
      starButton.classList.add("starred");
      setTimeout(() => {
        starButton.textContent = "Star this server";
        starButton.classList.remove("starred");
      }, 1200);
    } catch (_) {
      starButton.textContent = "Star failed";
    }
  });
}

const reportForm = document.querySelector("form[data-report-server]");
if (reportForm) {
  const reportStatus = document.querySelector("[data-report-status]");
  reportForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const server = reportForm.getAttribute("data-report-server");
    if (!server) return;
    const reasonField = reportForm.querySelector("[name='reason']");
    const detailsField = reportForm.querySelector("[name='details']");
    const reason = reasonField ? reasonField.value : "";
    const details = detailsField ? detailsField.value : "";
    try {
      const response = await fetch(`/servers/${encodeURIComponent(server)}/report`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ reason, details })
      });
      if (!response.ok) throw new Error();
      const payload = await response.json();
      applyCommunityCounts({ reports: payload.reports });
      try {
        const historyResponse = await fetch(
          `/servers/${encodeURIComponent(server)}/reports?limit=5`
        );
        if (historyResponse.ok) {
          const history = await historyResponse.json();
          renderRecentReports(history.reports);
        }
      } catch (_) {}
      if (reportStatus) {
        reportStatus.textContent = "Report submitted";
      }
      reportForm.reset();
    } catch (_) {
      if (reportStatus) {
        reportStatus.textContent = "Report failed";
      }
    }
  });
}
"#,
    );
    page.push_str("</script></body></html>");
    page
}

/// Renders one filter select options block with selected-state support.
fn render_site_filter_options(values: &BTreeSet<String>, selected: &str) -> String {
    let mut out = String::new();
    for value in values {
        let selected_attr = if value.eq_ignore_ascii_case(selected) {
            " selected"
        } else {
            ""
        };
        let escaped = html_escape(value);
        out.push_str(&format!(
            "<option value=\"{escaped}\"{selected_attr}>{escaped}</option>"
        ));
    }
    out
}

/// Builds a stable `/site` URL with current catalog query parameters.
fn build_site_catalog_path(params: &SiteCatalogUrlParams<'_>) -> String {
    let mut pairs = Vec::new();
    if !params.search_query.trim().is_empty() {
        pairs.push(("q".to_string(), url_encode(params.search_query.trim())));
    }
    if let Some(category) = params.category.filter(|value| !value.trim().is_empty()) {
        pairs.push(("category".to_string(), url_encode(category.trim())));
    }
    if let Some(platform) = params.platform.filter(|value| !value.trim().is_empty()) {
        pairs.push(("platform".to_string(), url_encode(platform.trim())));
    }
    if let Some(trust_level) = params.trust_level.filter(|value| !value.trim().is_empty()) {
        pairs.push(("trustLevel".to_string(), url_encode(trust_level.trim())));
    }
    pairs.push(("sortBy".to_string(), params.sort_by.as_str().to_string()));
    pairs.push(("order".to_string(), params.sort_order.as_str().to_string()));
    pairs.push(("limit".to_string(), params.limit.to_string()));
    pairs.push(("offset".to_string(), params.offset.to_string()));

    let query = pairs
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    format!("/site?{query}")
}

/// Builds a stable `/site/reports` URL with current report query parameters.
fn build_site_reports_path(params: &SiteReportsUrlParams<'_>) -> String {
    let mut pairs = Vec::new();
    if let Some(server) = params.server.filter(|value| !value.trim().is_empty()) {
        pairs.push(("server".to_string(), url_encode(server.trim())));
    }
    if let Some(reason) = params.reason.filter(|value| !value.trim().is_empty()) {
        pairs.push(("reason".to_string(), url_encode(reason.trim())));
    }
    pairs.push(("limit".to_string(), params.limit.to_string()));
    pairs.push(("offset".to_string(), params.offset.to_string()));

    let query = pairs
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    format!("/site/reports?{query}")
}

/// Builds a stable `/site/submissions` URL with current submission query parameters.
fn build_site_submissions_path(params: &SiteSubmissionsUrlParams<'_>) -> String {
    let mut pairs = Vec::new();
    if let Some(status) = params.status.filter(|value| !value.trim().is_empty()) {
        pairs.push(("status".to_string(), url_encode(status.trim())));
    }
    if let Some(server) = params.server.filter(|value| !value.trim().is_empty()) {
        pairs.push(("server".to_string(), url_encode(server.trim())));
    }
    pairs.push(("limit".to_string(), params.limit.to_string()));
    pairs.push(("offset".to_string(), params.offset.to_string()));

    let query = pairs
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    format!("/site/submissions?{query}")
}

/// Renders one permissions section for detail pages.
fn render_site_permissions_list(title: &str, entries: &[String]) -> String {
    let mut html = String::new();
    html.push_str(&format!(
        "<h3>{}</h3><ul class=\"perm-list\">",
        html_escape(title)
    ));
    if entries.is_empty() {
        html.push_str("<li><span class=\"meta\">none</span></li>");
    } else {
        for entry in entries {
            html.push_str(&format!("<li><code>{}</code></li>", html_escape(entry)));
        }
    }
    html.push_str("</ul>");
    html
}

/// Formats integer counts with comma thousands separators.
fn format_number(value: u64) -> String {
    let text = value.to_string();
    let mut reversed = String::new();
    for (idx, ch) in text.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            reversed.push(',');
        }
        reversed.push(ch);
    }
    reversed.chars().rev().collect()
}

/// Escapes text for safe interpolation into HTML text/attributes.
fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Decodes basic URL-encoded query/path fragments for local website use.
fn url_decode(input: &str) -> String {
    fn hex_to_u8(ch: u8) -> Option<u8> {
        match ch {
            b'0'..=b'9' => Some(ch - b'0'),
            b'a'..=b'f' => Some(ch - b'a' + 10),
            b'A'..=b'F' => Some(ch - b'A' + 10),
            _ => None,
        }
    }

    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut idx = 0;
    while idx < bytes.len() {
        match bytes[idx] {
            b'+' => {
                out.push(' ');
                idx += 1;
            }
            b'%' if idx + 2 < bytes.len() => {
                if let (Some(hi), Some(lo)) = (hex_to_u8(bytes[idx + 1]), hex_to_u8(bytes[idx + 2]))
                {
                    out.push((hi * 16 + lo) as char);
                    idx += 3;
                } else {
                    out.push('%');
                    idx += 1;
                }
            }
            value => {
                out.push(value as char);
                idx += 1;
            }
        }
    }
    out
}

/// Encodes basic URL query values.
fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
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
    if let Some(raw_submission_id) = path
        .strip_prefix("/publish/submissions/")
        .and_then(|rest| rest.strip_suffix("/status"))
    {
        if method != "POST" {
            return (
                405,
                json!({
                    "error": "method not allowed"
                }),
            );
        }
        return route_update_publish_submission_status(
            raw_submission_id,
            request.body.trim(),
            state,
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
        "/reports" => {
            if method != "GET" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            route_reports(query, state)
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
        "/publish/submissions" => {
            if method != "GET" {
                return (
                    405,
                    json!({
                        "error": "method not allowed"
                    }),
                );
            }
            route_publish_submissions(query, state)
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

fn route_reports(query: Option<&str>, state: &ApiState) -> (u16, Value) {
    let limit = parse_usize_param(query, "limit").unwrap_or(50).min(200);
    let offset = parse_usize_param(query, "offset").unwrap_or(0);
    let server_filter = query_param(query, "server")
        .map(url_decode)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let reason_filter = query_param(query, "reason")
        .map(url_decode)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    match state.list_all_reports() {
        Ok(mut reports) => {
            if let Some(server) = &server_filter {
                reports.retain(|event| event.server.eq_ignore_ascii_case(server));
            }
            if let Some(reason) = &reason_filter {
                reports.retain(|event| event.reason.eq_ignore_ascii_case(reason));
            }
            let total = reports.len();
            let reports = reports
                .into_iter()
                .skip(offset)
                .take(limit)
                .collect::<Vec<_>>();
            let count = reports.len();
            (
                200,
                json!({
                    "total": total,
                    "count": count,
                    "offset": offset,
                    "limit": limit,
                    "filters": {
                        "server": server_filter,
                        "reason": reason_filter
                    },
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

fn route_publish_submissions(query: Option<&str>, state: &ApiState) -> (u16, Value) {
    let limit = parse_usize_param(query, "limit").unwrap_or(25).min(200);
    let offset = parse_usize_param(query, "offset").unwrap_or(0);
    let status_filter = query_param(query, "status")
        .map(url_decode)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let server_filter = query_param(query, "server")
        .map(url_decode)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    match state.list_publish_submissions() {
        Ok(mut submissions) => {
            if let Some(status) = &status_filter {
                submissions.retain(|item| item.status.eq_ignore_ascii_case(status));
            }
            if let Some(server) = &server_filter {
                submissions.retain(|item| item.server.name.eq_ignore_ascii_case(server));
            }
            let total = submissions.len();
            let submissions = submissions
                .into_iter()
                .skip(offset)
                .take(limit)
                .collect::<Vec<_>>();
            let count = submissions.len();
            (
                200,
                json!({
                    "total": total,
                    "count": count,
                    "offset": offset,
                    "limit": limit,
                    "filters": {
                        "status": status_filter,
                        "server": server_filter
                    },
                    "submissions": submissions
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

fn route_update_publish_submission_status(
    raw_submission_id: &str,
    body: &str,
    state: &ApiState,
) -> (u16, Value) {
    let submission_id = url_decode(raw_submission_id);
    if !is_safe_submission_id(&submission_id) {
        return (
            400,
            json!({
                "error": "invalid submission id"
            }),
        );
    }
    let status = match parse_publish_submission_status_body(body) {
        Ok(status) => status,
        Err(error) => return error,
    };

    match state.set_publish_submission_status(&submission_id, &status) {
        Ok(Some(submission)) => (
            200,
            json!({
                "status": "updated",
                "submission": submission
            }),
        ),
        Ok(None) => (
            404,
            json!({
                "error": "submission not found"
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

fn parse_publish_submission_status_body(body: &str) -> Result<String, (u16, Value)> {
    let payload = if body.trim().is_empty() {
        return Err((
            400,
            json!({
                "error": "missing json body"
            }),
        ));
    } else {
        match serde_json::from_str::<PublishSubmissionStatusPayload>(body) {
            Ok(payload) => payload,
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
    };

    let normalized = payload.status.trim().to_ascii_lowercase();
    if !is_valid_submission_status(&normalized) {
        return Err((
            400,
            json!({
                "error": "invalid status"
            }),
        ));
    }
    Ok(normalized)
}

fn is_valid_submission_status(status: &str) -> bool {
    matches!(
        status,
        "pending-manual-review" | "approved" | "rejected" | "needs-changes"
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
    let offset = parse_usize_param(query, "offset").unwrap_or(0);
    match state.list_reports(&server.name) {
        Ok(events) => {
            let total = events.len();
            let reports = events
                .into_iter()
                .skip(offset)
                .take(limit)
                .collect::<Vec<_>>();
            let count = reports.len();
            (
                200,
                json!({
                    "server": server.name,
                    "total": total,
                    "count": count,
                    "offset": offset,
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

fn is_safe_submission_id(value: &str) -> bool {
    !value.is_empty()
        && value.ends_with(".json")
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains("..")
}

fn publish_submission_summary_from_queue_file(
    id: String,
    payload: QueueSubmissionFile,
) -> PublishSubmissionSummary {
    let quality_checks_passed = payload.quality_checks.iter().filter(|c| c.passed).count();
    let quality_checks_total = payload.quality_checks.len();
    PublishSubmissionSummary {
        id,
        submitted_at_epoch_secs: payload.submitted_at_epoch_secs,
        status: payload.status,
        server: PublishSubmissionServerSummary {
            name: payload.manifest.server.name,
            display_name: payload.manifest.server.display_name,
            version: payload.manifest.server.version,
            maintainer: payload.manifest.server.maintainer,
            category: payload.manifest.server.category,
        },
        quality_checks_passed,
        quality_checks_total,
    }
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
    let payload = serde_json::to_string(body)
        .unwrap_or_else(|_| "{\"error\":\"serialization failed\"}".to_string());
    write_http_response(
        stream,
        status,
        "application/json",
        &payload,
        &[
            ("Access-Control-Allow-Origin", "*"),
            ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
            ("Access-Control-Allow-Headers", "Content-Type"),
            ("Access-Control-Max-Age", "86400"),
        ],
    )
}

/// Writes an HTML HTTP response to a stream.
fn write_html_response(stream: &mut TcpStream, status: u16, body: &str) -> io::Result<()> {
    write_http_response(stream, status, "text/html; charset=utf-8", body, &[])
}

/// Writes an HTTP response body with explicit content type and optional extra headers.
fn write_http_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
    extra_headers: &[(&str, &str)],
) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    };

    let mut response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in extra_headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    response.push_str(body);
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
        let root = std::env::temp_dir().join(unique);
        let community_path = root.join("community");
        let queue_path = root.join("publish").join("queue");
        std::fs::create_dir_all(&community_path).unwrap();
        std::fs::create_dir_all(&queue_path).unwrap();
        ApiState::new(community_path, queue_path)
    }

    fn seed_publish_submission(
        state: &ApiState,
        file_name: &str,
        submitted_at_epoch_secs: u64,
        status: &str,
        server_name: &str,
    ) {
        let path = state.publish_queue_dir.join(file_name);
        let payload = json!({
            "submitted_at_epoch_secs": submitted_at_epoch_secs,
            "status": status,
            "manifest": {
                "server": {
                    "name": server_name,
                    "display_name": format!("{server_name} display"),
                    "version": "1.0.0",
                    "maintainer": "Acme",
                    "category": "developer-tools"
                }
            },
            "quality_checks": [
                {"name": "schema", "passed": true, "detail": "ok"},
                {"name": "security", "passed": false, "detail": "failed"}
            ]
        });
        std::fs::write(path, serde_json::to_string_pretty(&payload).unwrap()).unwrap();
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
    fn route_website_request_renders_catalog_and_detail() {
        let registry = Registry::from_seed();
        let state = test_state();

        let (catalog_status, catalog) = route_website_request(
            &req(
                "GET",
                "/site?q=github&category=developer-tools&trustLevel=official",
            ),
            &registry,
            &state,
        )
        .unwrap();
        assert_eq!(catalog_status, 200);
        assert!(catalog.contains("Server Catalog"));
        assert!(catalog.contains("/site/servers/github"));
        assert!(catalog.contains("copy-btn"));
        assert!(catalog.contains("page <strong>1</strong>"));
        assert!(catalog.contains("Overview"));
        assert!(catalog.contains("Trending Right Now"));
        assert!(catalog.contains("/site/reports"));
        assert!(catalog.contains("/site/submissions"));

        let (detail_status, detail) =
            route_website_request(&req("GET", "/site/servers/github"), &registry, &state).unwrap();
        assert_eq!(detail_status, 200);
        assert!(detail.contains("GitHub MCP Server"));
        assert!(detail.contains("berth install github"));
        assert!(detail.contains("Permissions"));
        assert!(detail.contains("Star this server"));
        assert!(detail.contains("Recent Reports"));
        assert!(detail.contains("data-report-list"));

        let report_req = HttpRequest {
            method: "POST".to_string(),
            target: "/servers/github/report".to_string(),
            body: "{\"reason\":\"spam\",\"details\":\"broken output\"}".to_string(),
        };
        let (report_status, _) = route_request(&report_req, &registry, &state);
        assert_eq!(report_status, 200);
        let (detail_after_status, detail_after) =
            route_website_request(&req("GET", "/site/servers/github"), &registry, &state).unwrap();
        assert_eq!(detail_after_status, 200);
        assert!(detail_after.contains("reason spam"));

        let (reports_status, reports_body) = route_website_request(
            &req("GET", "/site/reports?server=github"),
            &registry,
            &state,
        )
        .unwrap();
        assert_eq!(reports_status, 200);
        assert!(reports_body.contains("Moderation Reports Feed"));
        assert!(reports_body.contains("/site/servers/github"));
        assert!(reports_body.contains("reason spam"));

        seed_publish_submission(
            &state,
            "github-400.json",
            400,
            "pending-manual-review",
            "github",
        );
        let (submissions_status, submissions_body) = route_website_request(
            &req("GET", "/site/submissions?status=pending-manual-review"),
            &registry,
            &state,
        )
        .unwrap();
        assert_eq!(submissions_status, 200);
        assert!(submissions_body.contains("Publish Review Queue"));
        assert!(submissions_body.contains("/site/servers/github"));
        assert!(submissions_body.contains("pending-manual-review"));

        let (page_status, page_body) =
            route_website_request(&req("GET", "/site?limit=1&offset=1"), &registry, &state)
                .unwrap();
        assert_eq!(page_status, 200);
        assert!(page_body.contains("page <strong>2</strong>"));
        assert!(page_body.contains("Previous"));
        assert!(page_body.contains("Next"));
    }

    #[test]
    fn route_website_request_handles_missing_and_invalid_routes() {
        let registry = Registry::from_seed();
        let state = test_state();

        let (not_found_status, not_found_body) =
            route_website_request(&req("GET", "/site/servers/nope"), &registry, &state).unwrap();
        assert_eq!(not_found_status, 404);
        assert!(not_found_body.contains("Page Not Found"));

        let (method_status, method_body) =
            route_website_request(&req("POST", "/site"), &registry, &state).unwrap();
        assert_eq!(method_status, 405);
        assert!(method_body.contains("method not allowed"));

        assert!(route_website_request(&req("GET", "/servers"), &registry, &state).is_none());
    }

    #[test]
    fn url_decode_translates_plus_and_percent_sequences() {
        assert_eq!(url_decode("google+drive"), "google drive");
        assert_eq!(url_decode("mcp%2Fgithub"), "mcp/github");
    }

    #[test]
    fn build_site_catalog_path_preserves_query_state() {
        let path = build_site_catalog_path(&SiteCatalogUrlParams {
            search_query: "git hub",
            category: Some("developer-tools"),
            platform: Some("macos"),
            trust_level: Some("official"),
            sort_by: SortBy::QualityScore,
            sort_order: SortOrder::Desc,
            limit: 20,
            offset: 40,
        });
        assert!(path.starts_with("/site?"));
        assert!(path.contains("q=git%20hub"));
        assert!(path.contains("category=developer-tools"));
        assert!(path.contains("platform=macos"));
        assert!(path.contains("trustLevel=official"));
        assert!(path.contains("sortBy=qualityScore"));
        assert!(path.contains("order=desc"));
        assert!(path.contains("limit=20"));
        assert!(path.contains("offset=40"));
    }

    #[test]
    fn build_site_reports_path_preserves_query_state() {
        let path = build_site_reports_path(&SiteReportsUrlParams {
            server: Some("google drive"),
            reason: Some("unsafe output"),
            limit: 15,
            offset: 30,
        });
        assert!(path.starts_with("/site/reports?"));
        assert!(path.contains("server=google%20drive"));
        assert!(path.contains("reason=unsafe%20output"));
        assert!(path.contains("limit=15"));
        assert!(path.contains("offset=30"));
    }

    #[test]
    fn build_site_submissions_path_preserves_query_state() {
        let path = build_site_submissions_path(&SiteSubmissionsUrlParams {
            status: Some("pending manual review"),
            server: Some("github"),
            limit: 12,
            offset: 24,
        });
        assert!(path.starts_with("/site/submissions?"));
        assert!(path.contains("status=pending%20manual%20review"));
        assert!(path.contains("server=github"));
        assert!(path.contains("limit=12"));
        assert!(path.contains("offset=24"));
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
    fn route_request_supports_reports_feed_endpoint() {
        let registry = Registry::from_seed();
        let state = test_state();

        let report_one = HttpRequest {
            method: "POST".to_string(),
            target: "/servers/github/report".to_string(),
            body: "{\"reason\":\"spam\",\"details\":\"bad output\"}".to_string(),
        };
        let report_two = HttpRequest {
            method: "POST".to_string(),
            target: "/servers/filesystem/report".to_string(),
            body: "{\"reason\":\"abuse\",\"details\":\"unsafe behavior\"}".to_string(),
        };
        assert_eq!(route_request(&report_one, &registry, &state).0, 200);
        assert_eq!(route_request(&report_two, &registry, &state).0, 200);

        let (status, body) = route_request(&req("GET", "/reports?limit=10"), &registry, &state);
        assert_eq!(status, 200);
        assert_eq!(body["total"].as_u64(), Some(2));
        assert_eq!(body["count"].as_u64(), Some(2));

        let (github_status, github_body) =
            route_request(&req("GET", "/reports?server=github"), &registry, &state);
        assert_eq!(github_status, 200);
        assert_eq!(github_body["total"].as_u64(), Some(1));
        assert_eq!(github_body["reports"][0]["server"].as_str(), Some("github"));

        let (abuse_status, abuse_body) =
            route_request(&req("GET", "/reports?reason=abuse"), &registry, &state);
        assert_eq!(abuse_status, 200);
        assert_eq!(abuse_body["total"].as_u64(), Some(1));
        assert_eq!(abuse_body["reports"][0]["reason"].as_str(), Some("abuse"));
    }

    #[test]
    fn route_request_supports_publish_submissions_endpoint() {
        let registry = Registry::from_seed();
        let state = test_state();

        seed_publish_submission(
            &state,
            "github-200.json",
            200,
            "pending-manual-review",
            "github",
        );
        seed_publish_submission(&state, "filesystem-100.json", 100, "approved", "filesystem");

        let (status, body) = route_request(
            &req("GET", "/publish/submissions?limit=1&offset=0"),
            &registry,
            &state,
        );
        assert_eq!(status, 200);
        assert_eq!(body["total"].as_u64(), Some(2));
        assert_eq!(body["count"].as_u64(), Some(1));
        assert_eq!(
            body["submissions"][0]["server"]["name"].as_str(),
            Some("github")
        );
        assert_eq!(
            body["submissions"][0]["qualityChecksPassed"].as_u64(),
            Some(1)
        );
        assert_eq!(
            body["submissions"][0]["qualityChecksTotal"].as_u64(),
            Some(2)
        );

        let (pending_status, pending_body) = route_request(
            &req(
                "GET",
                "/publish/submissions?status=pending-manual-review&server=github",
            ),
            &registry,
            &state,
        );
        assert_eq!(pending_status, 200);
        assert_eq!(pending_body["total"].as_u64(), Some(1));
        assert_eq!(
            pending_body["submissions"][0]["status"].as_str(),
            Some("pending-manual-review")
        );
        assert_eq!(
            pending_body["submissions"][0]["server"]["name"].as_str(),
            Some("github")
        );
    }

    #[test]
    fn route_request_supports_publish_submission_status_updates() {
        let registry = Registry::from_seed();
        let state = test_state();
        seed_publish_submission(
            &state,
            "github-500.json",
            500,
            "pending-manual-review",
            "github",
        );

        let update_request = HttpRequest {
            method: "POST".to_string(),
            target: "/publish/submissions/github-500.json/status".to_string(),
            body: "{\"status\":\"approved\"}".to_string(),
        };
        let (update_status, update_body) = route_request(&update_request, &registry, &state);
        assert_eq!(update_status, 200);
        assert_eq!(update_body["status"].as_str(), Some("updated"));
        assert_eq!(
            update_body["submission"]["status"].as_str(),
            Some("approved")
        );
        assert_eq!(
            update_body["submission"]["server"]["name"].as_str(),
            Some("github")
        );

        let (approved_status, approved_body) = route_request(
            &req("GET", "/publish/submissions?status=approved&server=github"),
            &registry,
            &state,
        );
        assert_eq!(approved_status, 200);
        assert_eq!(approved_body["total"].as_u64(), Some(1));

        let invalid_request = HttpRequest {
            method: "POST".to_string(),
            target: "/publish/submissions/github-500.json/status".to_string(),
            body: "{\"status\":\"unknown\"}".to_string(),
        };
        let (invalid_status, invalid_body) = route_request(&invalid_request, &registry, &state);
        assert_eq!(invalid_status, 400);
        assert_eq!(invalid_body["error"].as_str(), Some("invalid status"));

        let missing_request = HttpRequest {
            method: "POST".to_string(),
            target: "/publish/submissions/nope.json/status".to_string(),
            body: "{\"status\":\"approved\"}".to_string(),
        };
        let (missing_status, missing_body) = route_request(&missing_request, &registry, &state);
        assert_eq!(missing_status, 404);
        assert_eq!(missing_body["error"].as_str(), Some("submission not found"));
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

        let second_report_req = HttpRequest {
            method: "POST".to_string(),
            target: "/servers/github/report".to_string(),
            body: "{\"reason\":\"abuse\",\"details\":\"unsafe behavior\"}".to_string(),
        };
        let (second_report_status, second_report_body) =
            route_request(&second_report_req, &registry, &state);
        assert_eq!(second_report_status, 200);
        assert_eq!(second_report_body["reports"].as_u64(), Some(2));

        let (community_status, community_body) =
            route_request(&req("GET", "/servers/github/community"), &registry, &state);
        assert_eq!(community_status, 200);
        assert_eq!(community_body["stars"].as_u64(), Some(1));
        assert_eq!(community_body["reports"].as_u64(), Some(2));

        let (reports_status, reports_body) = route_request(
            &req("GET", "/servers/github/reports?limit=1&offset=1"),
            &registry,
            &state,
        );
        assert_eq!(reports_status, 200);
        assert_eq!(reports_body["server"].as_str(), Some("github"));
        assert_eq!(reports_body["count"].as_u64(), Some(1));
        assert_eq!(reports_body["offset"].as_u64(), Some(1));
        assert!(reports_body["total"].as_u64().unwrap_or(0) >= 2);
        assert!(reports_body["reports"][0]["reason"].as_str().is_some());
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
