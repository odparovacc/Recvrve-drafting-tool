//! Counter picks: the champions that beat a pick in its lane, for the team panels.
//!
//! The data comes from OP.GG's MCP server (<https://mcp-api.op.gg/mcp>): ranked solo queue,
//! Master tier and above, current patch. OP.GG names up to three counters per champion and
//! lane ("weak counters"), each with the games played and the pick's wins in that matchup.
//!
//! A download is a snapshot of one patch, kept as `config/counters/<patch>.json` (see
//! [`snapshot_dir`]), so snapshots of several patches pile up for later merging. The app uses
//! the newest snapshot it has, or the one built into it (`assets/counters.json`).
//!
//! The download only reads public statistics. It is strict about what it accepts: HTTPS
//! only, bounded sizes and times, and every name and number checked before use; anything
//! else fails the whole update and leaves the current data as it was.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::champions::{CHAMPIONS, by_value};
use crate::roles::Role;

/// Matchups with fewer games than this are left out. OP.GG itself names no matchup under
/// 100 Master+ games; as one patch is all it offers, more would leave most picks bare.
pub const MIN_GAMES: u32 = 100;
/// Counters shown per pick.
pub const SHOWN: usize = 3;
/// The OP.GG rank filter used.
pub const TIER: &str = "master_plus";
const TIER_LABEL: &str = "Master+";
const ENDPOINT: &str = "https://mcp-api.op.gg/mcp";
/// Format of the snapshot files.
const FORMAT: u32 = 1;
/// The snapshot built into the app.
const BUNDLED: &str = include_str!("../assets/counters.json");

/// Limits on what the server may send.
const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_DEPTH: usize = 32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Requests at a time during an update.
const WORKERS: usize = 6;

/// One counter: a champion that beats the pick (by key, as in `champion_icons/`), and how
/// the pick fared against it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Counter {
    pub champion: String,
    pub games: u32,
    /// The pick's wins against it.
    pub wins: u32,
}

/// A champion in one lane: its games there (all ranks, for picking its usual lane) and the
/// counters OP.GG names for it there (none when the samples are too small).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Lane {
    pub games: u64,
    pub counters: Vec<Counter>,
}

/// One downloaded patch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub format: u32,
    pub source: String,
    pub tier: String,
    pub patch: String,
    /// Download date, `YYYY-MM-DD`.
    pub fetched: String,
    /// By champion key, then by lane (`Role::key`).
    pub champions: BTreeMap<String, BTreeMap<String, Lane>>,
}

impl Snapshot {
    fn patch_number(&self) -> (u32, u32) {
        patch_number(&self.patch).unwrap_or((0, 0))
    }

    /// "OP.GG, Master+ solo queue, patch 16.21 (downloaded 2026-11-02)".
    pub fn describe(&self) -> String {
        format!(
            "{}, {TIER_LABEL} solo queue, patch {} (downloaded {})",
            self.source, self.patch, self.fetched
        )
    }
}

fn patch_number(patch: &str) -> Option<(u32, u32)> {
    let (a, b) = patch.split_once('.')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

/// A counter as the team panels show it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    pub value: u8,
    pub games: u32,
    pub wins: u32,
}

impl Shown {
    /// The pick's win rate against this counter, in percent.
    pub fn pick_win_rate(&self) -> f64 {
        f64::from(self.wins) * 100.0 / f64::from(self.games.max(1))
    }
}

/// The snapshot in use, indexed by champion value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Counters {
    pub snapshot: Option<Snapshot>,
    /// Per champion value: per role (in `Role::ALL` order), games and counters.
    lanes: HashMap<u8, [(u64, Vec<Shown>); 5]>,
}

fn value_of_key(key: &str) -> Option<u8> {
    CHAMPIONS
        .iter()
        .position(|c| c.key.eq_ignore_ascii_case(key))
        .map(crate::champions::value_of)
}

fn role_index(role: Role) -> usize {
    Role::ALL.iter().position(|&r| r == role).unwrap_or(0)
}

impl Counters {
    pub fn new(snapshot: Option<Snapshot>) -> Counters {
        let mut lanes: HashMap<u8, [(u64, Vec<Shown>); 5]> = HashMap::new();
        if let Some(s) = &snapshot {
            for (key, by_lane) in &s.champions {
                let Some(value) = value_of_key(key) else {
                    continue;
                };
                let entry = lanes.entry(value).or_default();
                for (lane, data) in by_lane {
                    let Some(role) = Role::from_key(lane) else {
                        continue;
                    };
                    let shown = data
                        .counters
                        .iter()
                        .filter_map(|c| {
                            Some(Shown {
                                value: value_of_key(&c.champion)?,
                                games: c.games,
                                wins: c.wins.min(c.games),
                            })
                        })
                        .collect();
                    entry[role_index(role)] = (data.games, shown);
                }
            }
        }
        Counters { snapshot, lanes }
    }

    /// The newest snapshot available: downloaded or built in.
    pub fn load() -> Counters {
        Counters::new(newest_snapshot())
    }

    /// The lane counters are looked up in for `value` with role tags `tags`: of the tagged
    /// roles (any role when untagged), the one the champion plays most.
    pub fn lane(&self, value: u8, tags: u8) -> Option<Role> {
        let lanes = self.lanes.get(&value)?;
        Role::ALL
            .into_iter()
            .filter(|r| tags == 0 || tags & r.bit() != 0)
            .map(|r| (r, lanes[role_index(r)].0))
            .filter(|&(_, games)| games > 0)
            .max_by_key(|&(_, games)| games)
            .map(|(r, _)| r)
    }

    /// Up to [`SHOWN`] counters of `value` in `role`, best first, leaving out champions
    /// for which `unavailable` is true and matchups under [`MIN_GAMES`].
    pub fn counters(&self, value: u8, role: Role, unavailable: impl Fn(u8) -> bool) -> Vec<Shown> {
        self.lanes
            .get(&value)
            .map(|l| {
                l[role_index(role)]
                    .1
                    .iter()
                    .filter(|c| c.games >= MIN_GAMES && !unavailable(c.value))
                    .take(SHOWN)
                    .copied()
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Where downloaded snapshots are kept.
pub fn snapshot_dir() -> PathBuf {
    crate::configio::config_dir().join("counters")
}

fn bundled() -> Option<Snapshot> {
    serde_json::from_str(BUNDLED).ok().filter(valid)
}

fn valid(s: &Snapshot) -> bool {
    s.format == FORMAT && patch_number(&s.patch).is_some()
}

/// The newest of the downloaded snapshots and the built-in one.
fn newest_snapshot() -> Option<Snapshot> {
    let mut best = bundled();
    if let Ok(entries) = std::fs::read_dir(snapshot_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "json")
                || entry
                    .metadata()
                    .map_or(true, |m| m.len() > MAX_SNAPSHOT_BYTES)
            {
                continue;
            }
            let Some(s) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| serde_json::from_str::<Snapshot>(&text).ok())
                .filter(valid)
            else {
                continue;
            };
            let newer = best
                .as_ref()
                .is_none_or(|b| (s.patch_number(), &s.fetched) > (b.patch_number(), &b.fetched));
            if newer {
                best = Some(s);
            }
        }
    }
    best
}

/// Saves `s` as its patch's snapshot (replacing an earlier download of the same patch).
pub fn save(s: &Snapshot) -> std::io::Result<PathBuf> {
    let dir = snapshot_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", s.patch));
    let tmp = dir.join(format!("{}.json.tmp", s.patch));
    let text = serde_json::to_string_pretty(s).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

// ---------------------------------------------------------------------------------------
// Download.

/// Progress of an update, for the UI.
#[derive(Clone, Debug, PartialEq)]
pub enum Progress {
    /// Requests done out of all.
    Step(usize, usize),
    Done(Box<Snapshot>),
    Failed(String),
}

/// OP.GG's lane names, with ours.
const LANES: [(&str, Role); 5] = [
    ("top", Role::Top),
    ("jungle", Role::Jungle),
    ("mid", Role::Mid),
    ("adc", Role::Bot),
    ("support", Role::Support),
];

struct Client {
    agent: ureq::Agent,
    session: Option<String>,
}

impl Client {
    fn new() -> Result<Client, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .https_only(true)
            .timeout_global(Some(REQUEST_TIMEOUT))
            .max_redirects(0)
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .build()
            .into();
        let mut client = Client {
            agent,
            session: None,
        };
        client.rpc(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "Recvrve", "version": env!("CARGO_PKG_VERSION") },
            }),
        )?;
        client.notify("notifications/initialized")?;
        Ok(client)
    }

    fn post(&self, body: &Value) -> Result<ureq::http::Response<ureq::Body>, String> {
        let mut request = self
            .agent
            .post(ENDPOINT)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");
        if let Some(s) = &self.session {
            request = request.header("Mcp-Session-Id", s);
        }
        request
            .send(body.to_string())
            .map_err(|e| format!("OP.GG could not be reached: {e}"))
    }

    fn notify(&self, method: &str) -> Result<(), String> {
        self.post(&serde_json::json!({ "jsonrpc": "2.0", "method": method }))
            .map(|_| ())
    }

    /// A JSON-RPC call: its `result`.
    fn rpc(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let mut response = self.post(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": method, "params": params,
        }))?;
        if let Some(id) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .filter(|v| v.len() <= 200 && v.bytes().all(|b| b.is_ascii_graphic()))
        {
            self.session = Some(id.to_string());
        }
        let text = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_to_string()
            .map_err(|e| format!("OP.GG's answer could not be read: {e}"))?;
        let message = parse_rpc_body(&text)?;
        if let Some(error) = message.get("error") {
            let text = error.get("message").and_then(Value::as_str).unwrap_or("");
            return Err(format!("OP.GG refused a request: {}", clip(text, 200)));
        }
        message
            .get("result")
            .cloned()
            .ok_or_else(|| "OP.GG sent an answer without a result".to_string())
    }

    /// Calls an OP.GG tool: its answer, decoded (see [`parse_notation`]).
    fn tool(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        let result = self.rpc(
            "tools/call",
            serde_json::json!({ "name": name, "arguments": arguments }),
        )?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(format!("OP.GG's {name} failed"));
        }
        let text = result
            .get("content")
            .and_then(Value::as_array)
            .and_then(|c| c.iter().find_map(|p| p.get("text")?.as_str()))
            .ok_or_else(|| format!("OP.GG's {name} sent no text"))?;
        parse_notation(text)
    }
}

fn clip(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

/// The JSON-RPC message in a response body: plain JSON, or the last `data:` event of a
/// server-sent event stream.
fn parse_rpc_body(text: &str) -> Result<Value, String> {
    let trimmed = text.trim_start();
    let json = if trimmed.starts_with('{') {
        trimmed.to_string()
    } else {
        text.lines()
            .rev()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(str::trim)
            .find(|d| d.starts_with('{'))
            .ok_or("OP.GG sent an answer in an unknown form")?
            .to_string()
    };
    serde_json::from_str(&json).map_err(|_| "OP.GG sent malformed JSON".to_string())
}

/// Decodes OP.GG's compact answers: `class Name: field,field` lines, a blank line, then
/// one value written as `Name(value, ...)`, `[value, ...]`, `"text"`, numbers, `None`,
/// `True` or `False`. Each `Name(...)` becomes an object with the fields its class line
/// lists. Answers in plain JSON pass through.
pub fn parse_notation(text: &str) -> Result<Value, String> {
    let text = text.trim();
    if text.starts_with('{') || text.starts_with('[') {
        return serde_json::from_str(text).map_err(|_| "OP.GG sent malformed JSON".to_string());
    }
    let mut classes: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut body = "";
    let mut rest = text;
    while let Some(line_end) = rest.find('\n').or(Some(rest.len())) {
        let line = rest[..line_end].trim();
        let next = rest.get(line_end + 1..).unwrap_or("");
        if let Some(decl) = line.strip_prefix("class ") {
            let (name, fields) = decl.split_once(':').ok_or("bad class line")?;
            classes.insert(
                name.trim(),
                fields
                    .split(',')
                    .map(str::trim)
                    .filter(|f| !f.is_empty())
                    .collect(),
            );
            rest = next;
        } else if line.is_empty() && !next.is_empty() {
            rest = next;
        } else {
            body = rest;
            break;
        }
    }
    let mut p = Parser {
        s: body.as_bytes(),
        i: 0,
        classes: &classes,
    };
    let value = p.value(0)?;
    p.skip_space();
    if p.i != p.s.len() {
        return Err("OP.GG sent trailing data".into());
    }
    Ok(value)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    classes: &'a HashMap<&'a str, Vec<&'a str>>,
}

impl Parser<'_> {
    fn skip_space(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_space();
        self.s.get(self.i).copied()
    }

    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("OP.GG's answer is malformed near byte {}", self.i))
        }
    }

    /// Comma-separated values up to `close`.
    fn items(&mut self, close: u8, depth: usize) -> Result<Vec<Value>, String> {
        let mut items = Vec::new();
        if self.peek() == Some(close) {
            self.i += 1;
            return Ok(items);
        }
        loop {
            items.push(self.value(depth + 1)?);
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(c) if c == close => {
                    self.i += 1;
                    return Ok(items);
                }
                _ => return Err(format!("OP.GG's answer is malformed near byte {}", self.i)),
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > MAX_DEPTH {
            return Err("OP.GG's answer is nested too deeply".into());
        }
        match self.peek() {
            Some(b'[') => {
                self.i += 1;
                Ok(Value::Array(self.items(b']', depth)?))
            }
            Some(b'"') | Some(b'\'') => self.string().map(Value::String),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            Some(c) if c.is_ascii_alphabetic() || c == b'_' => {
                let start = self.i;
                while self.i < self.s.len()
                    && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_')
                {
                    self.i += 1;
                }
                let name = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
                match name {
                    "None" | "null" => return Ok(Value::Null),
                    "True" | "true" => return Ok(Value::Bool(true)),
                    "False" | "false" => return Ok(Value::Bool(false)),
                    _ => {}
                }
                self.expect(b'(')?;
                let args = self.items(b')', depth)?;
                let fields = self.classes.get(name).ok_or_else(|| {
                    format!("OP.GG's answer uses an unknown class {}", clip(name, 40))
                })?;
                if fields.len() != args.len() {
                    return Err(format!(
                        "OP.GG's {} has the wrong number of fields",
                        clip(name, 40)
                    ));
                }
                Ok(Value::Object(
                    fields.iter().map(|f| f.to_string()).zip(args).collect(),
                ))
            }
            _ => Err(format!("OP.GG's answer is malformed near byte {}", self.i)),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        let quote = self.s[self.i];
        self.i += 1;
        let mut out = Vec::new();
        while let Some(&c) = self.s.get(self.i) {
            self.i += 1;
            if c == quote {
                return String::from_utf8(out)
                    .map_err(|_| "OP.GG sent text that isn't UTF-8".into());
            }
            if c != b'\\' {
                out.push(c);
                continue;
            }
            let e = *self.s.get(self.i).ok_or("unterminated text")?;
            self.i += 1;
            match e {
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'r' => out.push(b'\r'),
                b'u' => {
                    let hex = self.s.get(self.i..self.i + 4).ok_or("bad escape")?;
                    let code = u32::from_str_radix(std::str::from_utf8(hex).unwrap_or("x"), 16)
                        .map_err(|_| "bad escape")?;
                    self.i += 4;
                    let ch = char::from_u32(code).unwrap_or('\u{FFFD}');
                    out.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
                }
                other => out.push(other),
            }
        }
        Err("OP.GG sent unterminated text".into())
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.i;
        while self.i < self.s.len()
            && matches!(
                self.s[self.i],
                b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
            )
        {
            self.i += 1;
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
        if let Ok(n) = text.parse::<i64>() {
            return Ok(Value::from(n));
        }
        text.parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| "OP.GG sent a malformed number".into())
    }
}

fn get<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(v, |v, k| v.get(k))
}

fn bounded_u32(v: &Value) -> Option<u32> {
    v.as_u64().filter(|&n| n <= 100_000_000).map(|n| n as u32)
}

/// Downloads a snapshot of the current patch. `progress` hears about every request.
pub fn download(progress: impl Fn(Progress) + Send + Sync + 'static) -> Result<Snapshot, String> {
    let mut client = Client::new()?;

    // OP.GG's champion ids and keys, for the champions the app knows. (Ids from 60000 on
    // are another mode's copies.)
    let list = client.tool(
        "lol_list_champions",
        serde_json::json!({ "desired_output_fields": ["data.champions[].{champion_id,key,name}"] }),
    )?;
    let mut by_id: HashMap<u64, &'static str> = HashMap::new();
    let mut by_name: HashMap<String, &'static str> = HashMap::new();
    for c in get(&list, &["data", "champions"])
        .and_then(Value::as_array)
        .ok_or("OP.GG sent no champion list")?
    {
        let (Some(id), Some(key), Some(name)) = (
            c.get("champion_id").and_then(Value::as_u64),
            c.get("key").and_then(Value::as_str),
            c.get("name").and_then(Value::as_str),
        ) else {
            continue;
        };
        if id >= 60_000 {
            continue;
        }
        if let Some(v) = value_of_key(key) {
            let ours = by_value(v).map_or("", |c| c.key);
            by_id.insert(id, ours);
            by_name.insert(name.to_lowercase(), ours);
        }
    }
    if by_id.len() < CHAMPIONS.len() / 2 {
        return Err("OP.GG's champion list doesn't match the app's champions".into());
    }

    // Which lanes each champion is played in, and how often.
    let fields: Vec<String> = LANES
        .iter()
        .map(|(lane, _)| format!("data.positions.{lane}[].{{champion,play}}"))
        .collect();
    let meta = client.tool(
        "lol_list_lane_meta_champions",
        serde_json::json!({ "position": "all", "desired_output_fields": fields }),
    )?;
    let mut champions: BTreeMap<String, BTreeMap<String, Lane>> = BTreeMap::new();
    let mut jobs: Vec<(&'static str, &'static str, Role)> = Vec::new();
    for (lane, role) in LANES {
        let Some(rows) = get(&meta, &["data", "positions", lane]).and_then(Value::as_array) else {
            continue;
        };
        for row in rows {
            let (Some(name), Some(games)) = (
                row.get("champion").and_then(Value::as_str),
                row.get("play").and_then(Value::as_u64),
            ) else {
                continue;
            };
            let Some(&key) = by_name.get(&name.to_lowercase()) else {
                continue;
            };
            champions.entry(key.to_string()).or_default().insert(
                role.key().to_string(),
                Lane {
                    games: games.min(1 << 40),
                    counters: Vec::new(),
                },
            );
            jobs.push((key, lane, role));
        }
    }
    if jobs.is_empty() {
        return Err("OP.GG sent no lane statistics".into());
    }

    // The counters, a few requests at a time.
    let total = jobs.len() + 2;
    progress(Progress::Step(2, total));
    let queue = Arc::new(Mutex::new(jobs));
    let results = Arc::new(Mutex::new(Vec::new()));
    let failure: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let done = Arc::new(std::sync::atomic::AtomicUsize::new(2));
    let progress = Arc::new(progress);
    let session = client.session.clone();
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            let (queue, results, failure, done, progress) = (
                queue.clone(),
                results.clone(),
                failure.clone(),
                done.clone(),
                progress.clone(),
            );
            let mut client = Client {
                agent: client.agent.clone(),
                session: session.clone(),
            };
            let by_id = &by_id;
            scope.spawn(move || {
                loop {
                    if failure.lock().unwrap().is_some() {
                        return;
                    }
                    let Some((key, lane, role)) = queue.lock().unwrap().pop() else {
                        return;
                    };
                    let mut attempt = || {
                        client.tool(
                            "lol_get_champion_analysis",
                            serde_json::json!({
                                "game_mode": "ranked",
                                "champion": key.to_uppercase(),
                                "position": lane,
                                "tier": TIER,
                                "desired_output_fields": [
                                    "data.weak_counters[].{champion_id,play,win}",
                                    "data.trends.win[].version",
                                ],
                            }),
                        )
                    };
                    let answer = attempt().or_else(|_| attempt());
                    match answer {
                        Ok(v) => {
                            let counters: Vec<Counter> = get(&v, &["data", "weak_counters"])
                                .and_then(Value::as_array)
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|c| {
                                            let champion =
                                                by_id.get(&c.get("champion_id")?.as_u64()?)?;
                                            let games = bounded_u32(c.get("play")?)?;
                                            let wins = bounded_u32(c.get("win")?)?.min(games);
                                            Some(Counter {
                                                champion: champion.to_string(),
                                                games,
                                                wins,
                                            })
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            // One trend or a list of them, depending on OP.GG's mood.
                            let trends = match get(&v, &["data", "trends", "win"]) {
                                Some(Value::Array(a)) => a.iter().collect(),
                                Some(w) => vec![w],
                                None => Vec::new(),
                            };
                            let patch = trends
                                .into_iter()
                                .filter_map(|w| w.get("version")?.as_str())
                                .filter_map(|p| patch_number(p).map(|n| (n, p.to_string())))
                                .max();
                            results.lock().unwrap().push((key, role, counters, patch));
                        }
                        Err(e) => {
                            failure.lock().unwrap().get_or_insert(e);
                            return;
                        }
                    }
                    let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    progress(Progress::Step(n, total));
                }
            });
        }
    });
    if let Some(e) = failure.lock().unwrap().take() {
        return Err(e);
    }
    let mut patch: Option<((u32, u32), String)> = None;
    for (key, role, counters, p) in std::mem::take(&mut *results.lock().unwrap()) {
        if let Some(lane) = champions.get_mut(key).and_then(|l| l.get_mut(role.key())) {
            lane.counters = counters;
        }
        patch = patch.max(p);
    }
    let patch = patch
        .ok_or("OP.GG didn't say which patch its statistics are from")?
        .1;
    Ok(Snapshot {
        format: FORMAT,
        source: "OP.GG".into(),
        tier: TIER.into(),
        patch,
        fetched: chrono::Local::now().format("%Y-%m-%d").to_string(),
        champions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notation_decodes() {
        let text = "class LolGetChampionAnalysis: data\nclass Data: weak_counters,trends\nclass WeakCounter: champion_id,play,win\nclass Trends: win\nclass Win: version\n\nLolGetChampionAnalysis(Data([WeakCounter(517,959,403),WeakCounter(75,380,159)],Trends([Win(\"16.21\"),Win(\"16.20\")])))";
        let v = parse_notation(text).unwrap();
        let w = get(&v, &["data", "weak_counters"])
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0]["champion_id"], 517);
        assert_eq!(w[1]["win"], 159);
        assert_eq!(v["data"]["trends"]["win"][1]["version"], "16.20");
    }

    #[test]
    fn notation_handles_text_and_rejects_junk() {
        let text = "class A: name,rate,ok\n\nA(\"Cho'Gath \\\"x\\\"\",0.42,None)";
        let v = parse_notation(text).unwrap();
        assert_eq!(v["name"], "Cho'Gath \"x\"");
        assert_eq!(v["rate"], 0.42);
        assert!(v["ok"].is_null());
        assert!(parse_notation("class A: x\n\nA(1,2)").is_err());
        assert!(parse_notation("class A: x\n\nB(1)").is_err());
        assert!(parse_notation("class A: x\n\nA(1) trailing").is_err());
        let deep = format!("class A: x\n\n{}1{}", "[".repeat(100), "]".repeat(100));
        assert!(parse_notation(&deep).is_err());
        assert!(parse_notation("class A: x\n\nA(\"open").is_err());
    }

    #[test]
    fn rpc_bodies() {
        assert_eq!(parse_rpc_body("{\"result\":1}").unwrap()["result"], 1);
        let sse = "event: message\ndata: {\"result\":2}\n\n";
        assert_eq!(parse_rpc_body(sse).unwrap()["result"], 2);
        assert!(parse_rpc_body("<html>").is_err());
    }

    fn snapshot() -> Snapshot {
        let lane = |games, counters: Vec<(&str, u32, u32)>| Lane {
            games,
            counters: counters
                .into_iter()
                .map(|(c, g, w)| Counter {
                    champion: c.into(),
                    games: g,
                    wins: w,
                })
                .collect(),
        };
        let mut champions = BTreeMap::new();
        champions.insert(
            "Malphite".to_string(),
            BTreeMap::from([
                (
                    "top".to_string(),
                    lane(
                        1000,
                        vec![
                            ("Sylas", 959, 403),
                            ("Nasus", 99, 40),
                            ("Shen", 273, 117),
                            ("Aatrox", 300, 100),
                        ],
                    ),
                ),
                ("mid".to_string(), lane(100, vec![("Ahri", 200, 80)])),
            ]),
        );
        Snapshot {
            format: FORMAT,
            source: "OP.GG".into(),
            tier: TIER.into(),
            patch: "16.21".into(),
            fetched: "2026-11-02".into(),
            champions,
        }
    }

    #[test]
    fn lanes_and_counters() {
        let c = Counters::new(Some(snapshot()));
        let malphite = value_of_key("Malphite").unwrap();
        let sylas = value_of_key("Sylas").unwrap();
        let shen = value_of_key("Shen").unwrap();
        // Untagged: the lane played most; tagged: the most played of the tagged lanes.
        assert_eq!(c.lane(malphite, 0), Some(Role::Top));
        assert_eq!(
            c.lane(malphite, Role::Mid.bit() | Role::Support.bit()),
            Some(Role::Mid)
        );
        assert_eq!(c.lane(malphite, Role::Support.bit()), None);
        // Small samples are left out, then unavailable champions; at most three.
        let all: Vec<u8> = c
            .counters(malphite, Role::Top, |_| false)
            .iter()
            .map(|s| s.value)
            .collect();
        assert_eq!(all.len(), 3);
        assert_eq!(all[..2], [sylas, shen]);
        let some: Vec<u8> = c
            .counters(malphite, Role::Top, |v| v == sylas)
            .iter()
            .map(|s| s.value)
            .collect();
        assert_eq!(some[0], shen);
        assert_eq!(some.len(), 2);
        let s = c.counters(malphite, Role::Top, |_| false)[0];
        assert!((s.pick_win_rate() - 42.02).abs() < 0.01);
    }

    #[test]
    fn bundled_snapshot_loads() {
        let s = bundled().expect("assets/counters.json is a valid snapshot");
        let c = Counters::new(Some(s));
        assert!(!c.lanes.is_empty());
    }

    /// Downloads the current patch into `assets/counters.json` (the built-in snapshot):
    /// `cargo test --release counters::tests::refresh_bundled -- --ignored`.
    #[test]
    #[ignore]
    fn refresh_bundled() {
        let s = download(|p| {
            if let Progress::Step(n, total) = p
                && n % 20 == 0
            {
                eprintln!("{n}/{total}");
            }
        })
        .unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/counters.json");
        std::fs::write(&path, serde_json::to_string_pretty(&s).unwrap() + "\n").unwrap();
        eprintln!("{}: {} champions", s.describe(), s.champions.len());
    }
}
