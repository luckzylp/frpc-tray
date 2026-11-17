//! STUN server discovery, quality probing and result cache.
//!
//! frp uses a STUN server to discover the public address of the machine before
//! attempting NAT hole punching (`natHoleStunServer` in `frpc.toml`). Public
//! servers differ wildly in reachability and latency from a given network, so
//! this module probes them for real and lets the user pick the fastest one.
//!
//! Candidate lists come from user editable sources (see [`sources_path`]). The
//! default is the always-online-stun project, which publishes a list of STUN
//! servers that were verified to answer recently:
//! <https://github.com/pradt2/always-online-stun>.
//!
//! Quality is measured with an actual RFC 5389 binding request, so the numbers
//! reflect what frpc will experience on the same network path. Besides latency
//! the response carries the server-reflexive address, which is shown as the
//! detected public (exit) IP.

use std::fs;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config;
use crate::logger;

/// STUN server frp falls back to when nothing is configured.
pub const FRP_DEFAULT_SERVER: &str = "stun.easyvoip.com:3478";

/// always-online-stun publishes the currently working servers here (hourly).
pub const DEFAULT_SOURCE: &str =
    "https://raw.githubusercontent.com/pradt2/always-online-stun/master/valid_hosts.txt";

const MAGIC_COOKIE: u32 = 0x2112_A442;
const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS: u16 = 0x0101;
const ATTR_MAPPED_ADDRESS: u16 = 0x0001;
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;

const PROBE_ATTEMPTS: usize = 3;
/// Two answers are enough to report a latency, the third packet only exists to
/// detect packet loss on a flaky server.
const PROBE_ENOUGH: usize = 2;
const PROBE_TIMEOUT: Duration = Duration::from_millis(700);
const PROBE_WORKERS: usize = 32;
const SOURCE_TIMEOUT: Duration = Duration::from_secs(20);
/// Probe on startup only when the cache is older than this.
const CACHE_TTL_SECS: u64 = 6 * 3600;

/// Used when no source can be reached and there is no cache yet.
const BUILTIN_SERVERS: &[&str] = &[
    FRP_DEFAULT_SERVER,
    "stun.l.google.com:19302",
    "stun1.l.google.com:19302",
    "stun.cloudflare.com:3478",
    "stun.nextcloud.com:443",
    "stun.miwifi.com:3478",
    "stun.chat.bilibili.com:3478",
    "stun.hitv.com:3478",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub addr: String,
    /// Average round trip of the successful probes, in milliseconds.
    pub latency_ms: Option<u64>,
    /// Packets sent that never produced a valid response.
    pub loss: u32,
    /// Packets sent in total.
    pub attempts: u32,
    /// Address the STUN server saw us coming from (the NAT exit address).
    pub public_ip: Option<String>,
    pub error: Option<String>,
    /// Comes from the user's own `servers.txt`, so it is always listed.
    #[serde(default)]
    pub custom: bool,
}

impl ProbeResult {
    fn failed(addr: &str, error: String) -> Self {
        Self {
            addr: addr.to_string(),
            latency_ms: None,
            loss: PROBE_ATTEMPTS as u32,
            attempts: PROBE_ATTEMPTS as u32,
            public_ip: None,
            error: Some(error),
            custom: false,
        }
    }

    pub fn is_usable(&self) -> bool {
        self.latency_ms.is_some()
    }

    /// Coarse quality label shown next to the latency in the menu.
    pub fn grade(&self) -> &'static str {
        match self.latency_ms {
            None => "不可用",
            Some(ms) if ms <= 100 => "优",
            Some(ms) if ms <= 250 => "良",
            Some(ms) if ms <= 500 => "一般",
            Some(_) => "差",
        }
    }
}

/// Runtime view of the STUN feature, owned by the tray state.
#[derive(Default)]
pub struct StunState {
    /// Sorted by latency; unusable servers last.
    pub results: Vec<ProbeResult>,
    pub probing: bool,
    /// Non fatal problems of the last refresh (bad source, fallback used, ...).
    pub notes: Vec<String>,
    pub updated_at: Option<u64>,
}

impl StunState {
    /// Fill the state from the on-disk cache. Returns `true` when the cache is
    /// missing or stale, i.e. a probe should be started.
    pub fn restore_from_cache(&mut self) -> bool {
        match load_cache() {
            Some((results, updated_at)) => {
                self.results = results;
                self.updated_at = Some(updated_at);
                cache_is_stale(updated_at)
            }
            None => true,
        }
    }

    pub fn apply(&mut self, results: Vec<ProbeResult>, notes: Vec<String>) {
        self.results = results;
        self.notes = notes;
        self.probing = false;
        self.updated_at = Some(now_secs());
    }

    /// Insert (or refresh) a single node and keep the list ordered.
    pub fn upsert(&mut self, result: ProbeResult) {
        self.results.retain(|r| r.addr != result.addr);
        self.results.push(result);
        sort_results(&mut self.results);
        self.updated_at = Some(now_secs());
        save_cache(&self.results);
    }
}

pub enum StunEvent {
    Started,
    Done {
        results: Vec<ProbeResult>,
        notes: Vec<String>,
    },
    /// Result of probing one freshly added node.
    Single(ProbeResult),
}

pub fn stun_dir() -> PathBuf {
    let dir = config::app_dir().join("stun");
    fs::create_dir_all(&dir).ok();
    dir
}

/// User editable list of STUN sources (remote lists, local files, addresses).
pub fn sources_path() -> PathBuf {
    stun_dir().join("sources.txt")
}

/// User maintained list of STUN nodes. These are always probed and always
/// listed in the tray menu, even when they are slower than the public ones.
pub fn custom_servers_path() -> PathBuf {
    stun_dir().join("servers.txt")
}

fn cache_path() -> PathBuf {
    stun_dir().join("cache.json")
}

/// Create `stun/sources.txt` with a commented template on first run.
pub fn ensure_sources_file() {
    let path = sources_path();
    if path.exists() {
        return;
    }
    let template = format!(
        "# STUN 服务器源。每行一条，支持三种写法：\n\
         #   1) 远程列表   https://example.com/stun.txt    （每行 host:port）\n\
         #   2) 本地文件   /data/stun.txt 或 C:\\stun.txt\n\
         #   3) 直接地址   stun.example.com:3478\n\
         #\n\
         # 一个源里可以混写以上三种内容。# 之后为注释。\n\
         # 只想加一两个自己的服务器时，建议写进 servers.txt（菜单里始终显示）。\n\
         # 修改后请在托盘菜单「STUN 服务器 → 测速并刷新」中重新测速。\n\
         {}\n",
        DEFAULT_SOURCE
    );
    let _ = fs::write(&path, template);
}

/// Create `stun/servers.txt` with a commented template on first run.
pub fn ensure_servers_file() {
    let path = custom_servers_path();
    if path.exists() {
        return;
    }
    let template = "# 自定义 STUN 节点，每行一个 host:port，行尾可用 # 写备注。\n\
         # 这里的节点会最先测速，并且始终出现在托盘菜单的 STUN 列表里（标 ★）。\n\
         #\n\
         # 示例：\n\
         # stun.myserver.com:3478\n\
         # 192.168.1.10:3478   # 内网测试服务器\n\
         #\n\
         # 也可以直接用菜单「STUN 服务器 → 添加自定义节点…」录入。\n";
    let _ = fs::write(&path, template);
}

/// Addresses listed in `servers.txt`, in file order.
pub fn load_custom_servers() -> Vec<String> {
    ensure_servers_file();
    let Ok(text) = fs::read_to_string(custom_servers_path()) else {
        return Vec::new();
    };
    let mut addrs = Vec::new();
    for line in text.lines() {
        let body = line.split('#').next().unwrap_or("").trim();
        if body.is_empty() {
            continue;
        }
        let Some(token) = body.split_whitespace().next() else {
            continue;
        };
        if is_server_addr(token) && !addrs.iter().any(|a: &String| a.eq_ignore_ascii_case(token)) {
            addrs.push(token.to_string());
        }
    }
    addrs
}

/// Fill in the standard STUN port when the user only typed a host name.
fn normalize_node(input: &str) -> String {
    let trimmed = input.trim();
    if !trimmed.is_empty() && !trimmed.contains(':') && !trimmed.contains('/') {
        format!("{}:3478", trimmed)
    } else {
        trimmed.to_string()
    }
}

fn append_line(path: &Path, line: &str) -> Result<(), String> {
    let mut text = fs::read_to_string(path).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(line);
    text.push('\n');
    fs::write(path, text).map_err(|e| format!("写入 {} 失败: {}", path.display(), e))
}

/// Append a node to `servers.txt`. Returns the normalised address, or `None`
/// when it was already present.
pub fn add_custom_server(input: &str) -> Result<Option<String>, String> {
    let addr = normalize_node(input);
    if !is_server_addr(&addr) {
        return Err("格式应为 host:port，例如 stun.example.com:3478".to_string());
    }
    ensure_servers_file();
    if load_custom_servers()
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&addr))
    {
        return Ok(None);
    }
    append_line(&custom_servers_path(), &addr)?;
    Ok(Some(addr))
}

/// Append a source line to `sources.txt`. Returns `false` when it was already
/// there.
pub fn add_custom_source(input: &str) -> Result<bool, String> {
    let entry = input.trim();
    if entry.is_empty() || entry.contains(['\n', '\r']) {
        return Err("源不能为空，也不能包含换行".to_string());
    }
    let valid = is_url(entry) || is_server_addr(entry) || Path::new(entry).exists();
    if !valid {
        return Err("请填写 http(s) 链接、本地列表文件路径或 host:port".to_string());
    }
    ensure_sources_file();
    let existing = fs::read_to_string(sources_path()).unwrap_or_default();
    if existing
        .lines()
        .any(|line| line.split('#').next().unwrap_or("").trim() == entry)
    {
        return Ok(false);
    }
    append_line(&sources_path(), entry)?;
    Ok(true)
}

enum Source {
    Url(String),
    File(PathBuf),
    Server(String),
}

fn is_server_addr(value: &str) -> bool {
    match value.rsplit_once(':') {
        Some((host, port)) => {
            !host.trim().is_empty()
                && !host.contains(char::is_whitespace)
                && port.parse::<u16>().is_ok()
        }
        None => false,
    }
}

fn is_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Split a source line into `host:port` candidates. Public lists separate
/// entries with spaces, commas or semicolons, and some append comments.
fn candidates_of(line: &str) -> Vec<String> {
    let line = line.split('#').next().unwrap_or("");
    line.split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .filter(|token| is_server_addr(token))
        .map(|token| token.to_string())
        .collect()
}

fn add_candidate(list: &mut Vec<String>, seen: &mut std::collections::HashSet<String>, addr: &str) {
    let addr = addr.trim();
    if !is_server_addr(addr) {
        return;
    }
    if seen.insert(addr.to_ascii_lowercase()) {
        list.push(addr.to_string());
    }
}

fn load_sources() -> (Vec<Source>, Vec<String>) {
    ensure_sources_file();
    let mut sources = Vec::new();
    let mut notes = Vec::new();
    let text = match fs::read_to_string(sources_path()) {
        Ok(t) => t,
        Err(e) => {
            notes.push(format!("读取 sources.txt 失败: {}", e));
            return (sources, notes);
        }
    };
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if is_url(line) {
            sources.push(Source::Url(line.to_string()));
            continue;
        }
        let inline = candidates_of(line);
        if !inline.is_empty() {
            for addr in inline {
                sources.push(Source::Server(addr));
            }
            continue;
        }
        sources.push(Source::File(PathBuf::from(line)));
    }
    if sources.is_empty() {
        notes.push("sources.txt 中没有可用的源".to_string());
    }
    (sources, notes)
}

fn collect_servers(notes: &mut Vec<String>) -> Vec<String> {
    let (sources, source_notes) = load_sources();
    notes.extend(source_notes);

    let client = reqwest::blocking::Client::builder()
        .timeout(SOURCE_TIMEOUT)
        .connect_timeout(Duration::from_secs(10))
        .user_agent("frpc-tray")
        .build()
        .ok();

    let mut list: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Nodes the user added by hand are probed first and never dropped, even if
    // every online source is unreachable.
    let custom = load_custom_servers();
    for addr in &custom {
        add_candidate(&mut list, &mut seen, addr);
    }

    for source in &sources {
        match source {
            Source::Server(addr) => add_candidate(&mut list, &mut seen, addr),
            Source::File(path) => match fs::read_to_string(path) {
                Ok(text) => {
                    for addr in text.lines().flat_map(candidates_of) {
                        add_candidate(&mut list, &mut seen, &addr);
                    }
                }
                Err(e) => notes.push(format!("读取 {} 失败: {}", path.display(), e)),
            },
            Source::Url(url) => {
                let Some(client) = client.as_ref() else {
                    notes.push("HTTP 客户端初始化失败，跳过远程源".to_string());
                    continue;
                };
                match client.get(url).send().and_then(|r| r.error_for_status()) {
                    Ok(resp) => match resp.text() {
                        Ok(text) => {
                            for addr in text.lines().flat_map(candidates_of) {
                                add_candidate(&mut list, &mut seen, &addr);
                            }
                        }
                        Err(e) => notes.push(format!("读取源内容失败: {}", e)),
                    },
                    Err(e) => notes.push(format!("获取源失败: {}", e)),
                }
            }
        }
    }

    if list.is_empty()
        && let Some((cached, _)) = load_cache()
    {
        for item in &cached {
            add_candidate(&mut list, &mut seen, &item.addr);
        }
    }
    if list.is_empty() {
        for addr in BUILTIN_SERVERS {
            add_candidate(&mut list, &mut seen, addr);
        }
        notes.push("在线源不可用，已回退到内置服务器列表".to_string());
    }
    list
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Cached results plus the unix time they were produced at.
pub fn load_cache() -> Option<(Vec<ProbeResult>, u64)> {
    let text = fs::read_to_string(cache_path()).ok()?;
    let cache: CacheFile = serde_json::from_str(&text).ok()?;
    Some((cache.results, cache.updated_at))
}

pub fn cache_is_stale(updated_at: u64) -> bool {
    now_secs().saturating_sub(updated_at) > CACHE_TTL_SECS
}

fn save_cache(results: &[ProbeResult]) {
    let cache = CacheFile {
        updated_at: now_secs(),
        results: results.to_vec(),
    };
    if let Ok(text) = serde_json::to_string(&cache) {
        let _ = fs::write(cache_path(), text);
    }
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    updated_at: u64,
    results: Vec<ProbeResult>,
}

/// Fetch every source and probe all candidates on a background thread, then
/// report the sorted results through `tx`.
pub fn refresh(tx: Sender<StunEvent>) {
    thread::spawn(move || {
        let _ = tx.send(StunEvent::Started);
        let mut notes = Vec::new();
        let addrs = collect_servers(&mut notes);
        logger::log(&format!("STUN: 候选服务器 {} 个，开始测速", addrs.len()));

        let mut results = probe_all(&addrs);
        let custom: std::collections::HashSet<String> = load_custom_servers()
            .iter()
            .map(|a| a.to_ascii_lowercase())
            .collect();
        for result in &mut results {
            result.custom = custom.contains(&result.addr.to_ascii_lowercase());
        }
        sort_results(&mut results);
        if !custom.is_empty() {
            logger::log(&format!("STUN: 其中 {} 个自定义节点", custom.len()));
        }

        let usable = results.iter().filter(|r| r.is_usable()).count();
        logger::log(&format!(
            "STUN: 测速完成，{}/{} 个可用",
            usable,
            results.len()
        ));
        if let Some(best) = results.iter().find(|r| r.is_usable()) {
            logger::log(&format!(
                "STUN: 最快 {} ({}ms)",
                best.addr,
                best.latency_ms.unwrap_or(0)
            ));
        }
        save_cache(&results);
        let _ = tx.send(StunEvent::Done { results, notes });
    });
}

/// Probe one user added node without re-running the whole list.
pub fn probe_custom_async(addr: String, tx: Sender<StunEvent>) {
    thread::spawn(move || {
        let mut result = probe(&addr);
        result.custom = true;
        match result.latency_ms {
            Some(ms) => logger::log(&format!("STUN: 自定义节点 {} 可用 ({}ms)", addr, ms)),
            None => logger::log(&format!(
                "STUN: 自定义节点 {} 无响应 ({})",
                addr,
                result.error.as_deref().unwrap_or("未知原因")
            )),
        }
        let _ = tx.send(StunEvent::Single(result));
    });
}

fn sort_results(results: &mut [ProbeResult]) {
    results.sort_by(|a, b| match (a.latency_ms, b.latency_ms) {
        (Some(x), Some(y)) => x.cmp(&y).then_with(|| a.addr.cmp(&b.addr)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.addr.cmp(&b.addr),
    });
}

/// Probe all addresses with a bounded worker pool.
pub fn probe_all(addrs: &[String]) -> Vec<ProbeResult> {
    if addrs.is_empty() {
        return Vec::new();
    }
    let addrs = Arc::new(addrs.to_vec());
    let next = Arc::new(AtomicUsize::new(0));
    let collected: Arc<Mutex<Vec<ProbeResult>>> = Arc::new(Mutex::new(Vec::new()));

    let workers = PROBE_WORKERS.min(addrs.len());
    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let addrs = Arc::clone(&addrs);
        let next = Arc::clone(&next);
        let collected = Arc::clone(&collected);
        handles.push(thread::spawn(move || {
            loop {
                let idx = next.fetch_add(1, Ordering::Relaxed);
                let Some(addr) = addrs.get(idx) else {
                    break;
                };
                let result = probe(addr);
                if let Ok(mut guard) = collected.lock() {
                    guard.push(result);
                }
            }
        }));
    }
    for handle in handles {
        let _ = handle.join();
    }

    Arc::try_unwrap(collected)
        .map(|m| m.into_inner().unwrap_or_default())
        .unwrap_or_default()
}

/// Measure one server with up to three binding requests.
pub fn probe(addr: &str) -> ProbeResult {
    let target = match resolve(addr) {
        Ok(t) => t,
        Err(e) => return ProbeResult::failed(addr, e),
    };

    let bind = if target.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let socket = match UdpSocket::bind(bind) {
        Ok(s) => s,
        Err(e) => return ProbeResult::failed(addr, format!("UDP 绑定失败: {}", e)),
    };
    if let Err(e) = socket.connect(target) {
        return ProbeResult::failed(addr, format!("UDP 连接失败: {}", e));
    }
    let _ = socket.set_read_timeout(Some(PROBE_TIMEOUT));
    let _ = socket.set_write_timeout(Some(PROBE_TIMEOUT));

    let mut successes = 0usize;
    // Only timeouts that happen *after* a first answer count as loss: the very
    // first packet of a cold path routinely needs longer than the timeout.
    let mut loss = 0u32;
    let mut sent = 0u32;
    let mut total_ms = 0u128;
    let mut public_ip: Option<String> = None;

    for _ in 0..PROBE_ATTEMPTS {
        let txid = next_txid();
        let request = build_request(txid);
        let started = Instant::now();
        sent += 1;
        if socket.send(&request).is_err() {
            if successes > 0 {
                loss += 1;
            }
            continue;
        }
        let mut buf = [0u8; 512];
        match socket.recv(&mut buf) {
            Ok(n) => match parse_response(&buf[..n], &txid) {
                Some(mapped) => {
                    successes += 1;
                    total_ms += started.elapsed().as_millis();
                    if mapped.is_some() {
                        public_ip = mapped;
                    }
                    if successes >= PROBE_ENOUGH {
                        break;
                    }
                }
                None => {
                    if successes > 0 {
                        loss += 1;
                    }
                }
            },
            Err(_) => {
                if successes > 0 {
                    loss += 1;
                }
            }
        }
    }

    let latency_ms = if successes > 0 {
        Some((total_ms / successes as u128) as u64)
    } else {
        None
    };
    ProbeResult {
        addr: addr.to_string(),
        latency_ms,
        loss,
        attempts: sent,
        public_ip,
        error: if latency_ms.is_none() {
            Some("无响应".to_string())
        } else {
            None
        },
        custom: false,
    }
}

fn resolve(addr: &str) -> Result<SocketAddr, String> {
    let mut iter = addr
        .to_socket_addrs()
        .map_err(|e| format!("域名解析失败: {}", e))?;
    iter.next().ok_or_else(|| "域名解析无结果".to_string())
}

fn build_request(txid: [u8; 12]) -> [u8; 20] {
    let mut packet = [0u8; 20];
    packet[..2].copy_from_slice(&BINDING_REQUEST.to_be_bytes());
    // Message length stays 0: no attributes in the request.
    packet[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
    packet[8..20].copy_from_slice(&txid);
    packet
}

/// `Some(public_address)` for a valid binding success response to `txid`.
/// The inner option is `None` when the response carried no address attribute,
/// which still proves the server is alive.
fn parse_response(buf: &[u8], txid: &[u8; 12]) -> Option<Option<String>> {
    if buf.len() < 20 {
        return None;
    }
    if u16::from_be_bytes([buf[0], buf[1]]) != BINDING_SUCCESS {
        return None;
    }
    if u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) != MAGIC_COOKIE {
        return None;
    }
    if &buf[8..20] != txid {
        return None;
    }

    let length = u16::from_be_bytes([buf[2], buf[3]]) as usize;
    let end = (20 + length).min(buf.len());
    let mut offset = 20;
    while offset + 4 <= end {
        let attr_type = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
        let attr_len = u16::from_be_bytes([buf[offset + 2], buf[offset + 3]]) as usize;
        let body_start = offset + 4;
        let body_end = body_start + attr_len;
        if body_end > end {
            break;
        }
        let body = &buf[body_start..body_end];
        match attr_type {
            ATTR_XOR_MAPPED_ADDRESS => return Some(xor_mapped_address(body, txid)),
            ATTR_MAPPED_ADDRESS => return Some(mapped_address(body)),
            _ => {}
        }
        // Attributes are padded to a 4 byte boundary.
        offset = body_start + attr_len + (4 - attr_len % 4) % 4;
    }
    Some(None)
}

fn mapped_address(body: &[u8]) -> Option<String> {
    if body.len() < 8 {
        return None;
    }
    match body[1] {
        0x01 => Some(format!("{}.{}.{}.{}", body[4], body[5], body[6], body[7])),
        0x02 => {
            if body.len() < 20 {
                return None;
            }
            let mut parts = Vec::with_capacity(8);
            for i in 0..8 {
                let o = 4 + i * 2;
                parts.push(format!("{:x}", u16::from_be_bytes([body[o], body[o + 1]])));
            }
            Some(parts.join(":"))
        }
        _ => None,
    }
}

fn xor_mapped_address(body: &[u8], txid: &[u8; 12]) -> Option<String> {
    if body.len() < 8 {
        return None;
    }
    let cookie = MAGIC_COOKIE.to_be_bytes();
    match body[1] {
        0x01 => Some(format!(
            "{}.{}.{}.{}",
            body[4] ^ cookie[0],
            body[5] ^ cookie[1],
            body[6] ^ cookie[2],
            body[7] ^ cookie[3]
        )),
        0x02 => {
            if body.len() < 20 {
                return None;
            }
            let mut parts = Vec::with_capacity(8);
            for i in 0..8 {
                let o = 4 + i * 2;
                let raw = u16::from_be_bytes([body[o], body[o + 1]]);
                let mask = if i < 4 {
                    u16::from_be_bytes([cookie[i * 2], cookie[i * 2 + 1]])
                } else {
                    u16::from_be_bytes([txid[(i - 4) * 2], txid[(i - 4) * 2 + 1]])
                };
                parts.push(format!("{:x}", raw ^ mask));
            }
            Some(parts.join(":"))
        }
        _ => None,
    }
}

/// Pseudo random transaction id; uniqueness only has to survive a few packets
/// per server, so a xorshift seeded from the clock, pid and a counter is fine.
fn next_txid() -> [u8; 12] {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut state = nanos
        ^ COUNTER
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ ((std::process::id() as u64) << 32);
    state |= 1;
    let mut txid = [0u8; 12];
    for chunk in txid.chunks_mut(4) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        chunk.copy_from_slice(&(state as u32).to_be_bytes());
    }
    txid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_request_is_well_formed() {
        let txid = [7u8; 12];
        let packet = build_request(txid);
        assert_eq!(&packet[..2], &[0x00, 0x01]);
        assert_eq!(&packet[2..4], &[0x00, 0x00]);
        assert_eq!(&packet[4..8], &MAGIC_COOKIE.to_be_bytes());
        assert_eq!(&packet[8..20], &txid);
    }

    #[test]
    fn transaction_ids_differ() {
        assert_ne!(next_txid(), next_txid());
    }

    #[test]
    fn parses_xor_mapped_address() {
        let txid = [0x11u8; 12];
        let cookie = MAGIC_COOKIE.to_be_bytes();
        let ip = [203u8, 0, 113, 42];
        let port: u16 = 54321;

        let mut body = vec![0x00, 0x01];
        body.extend_from_slice(&(port ^ 0x2112).to_be_bytes());
        for i in 0..4 {
            body.push(ip[i] ^ cookie[i]);
        }

        let mut packet = vec![0x01, 0x01, 0x00, 0x0C];
        packet.extend_from_slice(&cookie);
        packet.extend_from_slice(&txid);
        packet.extend_from_slice(&[0x00, ATTR_XOR_MAPPED_ADDRESS as u8]);
        packet.extend_from_slice(&(body.len() as u16).to_be_bytes());
        packet.extend_from_slice(&body);

        assert_eq!(
            parse_response(&packet, &txid),
            Some(Some("203.0.113.42".to_string()))
        );
    }

    #[test]
    fn rejects_foreign_transaction() {
        let txid = [0x11u8; 12];
        let cookie = MAGIC_COOKIE.to_be_bytes();
        let mut packet = vec![0x01, 0x01, 0x00, 0x00];
        packet.extend_from_slice(&cookie);
        packet.extend_from_slice(&[0x22u8; 12]);
        assert_eq!(parse_response(&packet, &txid), None);
    }

    #[test]
    fn accepts_response_without_attributes() {
        let txid = [0x33u8; 12];
        let mut packet = vec![0x01, 0x01, 0x00, 0x00];
        packet.extend_from_slice(&MAGIC_COOKIE.to_be_bytes());
        packet.extend_from_slice(&txid);
        assert_eq!(parse_response(&packet, &txid), Some(None));
    }

    #[test]
    fn server_address_detection() {
        assert!(is_server_addr("stun.l.google.com:19302"));
        assert!(is_server_addr("127.0.0.1:3478"));
        assert!(!is_server_addr("C:\\stun.txt"));
        assert!(!is_server_addr("stun.l.google.com"));
        assert!(!is_server_addr("stun.x.com:notaport"));
    }

    #[test]
    fn candidates_strip_comments_and_noise() {
        assert_eq!(
            candidates_of("stun.hitv.com:3478  # verified"),
            vec!["stun.hitv.com:3478".to_string()]
        );
        assert_eq!(
            candidates_of("stun.hitv.com:3478,stun.x.com:3478"),
            vec![
                "stun.hitv.com:3478".to_string(),
                "stun.x.com:3478".to_string()
            ]
        );
        assert!(candidates_of("# stun.hitv.com:3478").is_empty());
        assert!(candidates_of("   ").is_empty());
        assert!(candidates_of("https://example.com/list.txt").is_empty());
    }

    #[test]
    fn sorting_puts_fastest_first() {
        let mut results = vec![
            ProbeResult {
                addr: "slow".into(),
                latency_ms: Some(400),
                loss: 0,
                attempts: 2,
                public_ip: None,
                error: None,
                custom: false,
            },
            ProbeResult::failed("dead", "x".into()),
            ProbeResult {
                addr: "fast".into(),
                latency_ms: Some(20),
                loss: 0,
                attempts: 2,
                public_ip: None,
                error: None,
                custom: false,
            },
        ];
        sort_results(&mut results);
        assert_eq!(results[0].addr, "fast");
        assert_eq!(results[1].addr, "slow");
        assert_eq!(results[2].addr, "dead");
    }

    #[test]
    fn grades_follow_latency() {
        let mut r = ProbeResult::failed("x", String::new());
        assert_eq!(r.grade(), "不可用");
        r.latency_ms = Some(50);
        assert_eq!(r.grade(), "优");
        r.latency_ms = Some(150);
        assert_eq!(r.grade(), "良");
        r.latency_ms = Some(300);
        assert_eq!(r.grade(), "一般");
        r.latency_ms = Some(900);
        assert_eq!(r.grade(), "差");
    }

    #[test]
    fn normalizes_node_input() {
        assert_eq!(normalize_node("stun.example.com"), "stun.example.com:3478");
        assert_eq!(
            normalize_node("  stun.example.com:3479  "),
            "stun.example.com:3479"
        );
        assert_eq!(normalize_node("1.2.3.4:3478"), "1.2.3.4:3478");
        assert_eq!(normalize_node(""), "");
        // A URL or path keeps its shape so it can be validated as an error.
        assert_eq!(normalize_node("https://x/y.txt"), "https://x/y.txt");
    }

    /// Custom nodes/sources are persisted in `frp/stun/*.txt`.
    #[test]
    fn custom_nodes_and_sources_are_persisted() {
        let home =
            std::env::temp_dir().join(format!("frpc-tray-stun-custom-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        // Only this test touches the app directory, so the global base dir is
        // resolved to the temporary home for the rest of the process.
        unsafe { std::env::set_var("FRPC_TRAY_HOME", &home) };

        assert!(load_custom_servers().is_empty());
        assert_eq!(
            add_custom_server("stun.mine.com").unwrap().as_deref(),
            Some("stun.mine.com:3478")
        );
        // Same node again (with explicit port) is not duplicated.
        assert_eq!(add_custom_server("stun.mine.com:3478").unwrap(), None);
        assert!(add_custom_server("not a host").is_err());
        assert_eq!(
            load_custom_servers(),
            vec!["stun.mine.com:3478".to_string()]
        );

        assert!(add_custom_source("https://example.com/list.txt").unwrap());
        assert!(!add_custom_source("https://example.com/list.txt").unwrap());
        assert!(add_custom_source("stun.other.com:3478").unwrap());
        assert!(add_custom_source("nonsense").is_err());

        let sources = fs::read_to_string(sources_path()).unwrap();
        assert!(sources.contains(DEFAULT_SOURCE));
        assert!(sources.contains("https://example.com/list.txt"));
        assert!(sources.contains("stun.other.com:3478"));

        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn pool_handles_unreachable_server() {
        let results = probe_all(&["127.0.0.1:9".to_string()]);
        assert_eq!(results.len(), 1);
        assert!(!results[0].is_usable());
        assert!(results[0].error.is_some());
    }

    /// Network dependent smoke test, skipped in CI:
    /// `cargo test -- --ignored --nocapture live_`
    #[test]
    #[ignore]
    fn live_probe_public_stun_servers() {
        for addr in ["stun.l.google.com:19302", "stun.nextcloud.com:443"] {
            let result = probe(addr);
            println!("{} => {:?}", addr, result);
            assert!(result.is_usable(), "{} 无响应: {:?}", addr, result.error);
        }
    }

    /// Verifies the online source list can be downloaded and parsed.
    #[test]
    #[ignore]
    fn live_source_list() {
        let home = std::env::temp_dir().join(format!("frpc-tray-stun-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        unsafe { std::env::set_var("FRPC_TRAY_HOME", &home) };
        let mut notes = Vec::new();
        let addrs = collect_servers(&mut notes);
        println!("candidates: {} notes: {:?}", addrs.len(), notes);
        println!("first: {:?}", addrs.iter().take(3).collect::<Vec<_>>());
        assert!(addrs.len() > 10, "在线列表过小: {}", addrs.len());
    }
}
