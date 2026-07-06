//! Kodi JSON-RPC client + LAN discovery + path mapping, ported from
//! `kodi_client.py`. Pure logic (reqwest + std::net); no Tauri.

use base64::Engine;
use rayon::prelude::*;
use regex::Regex;
use serde_json::{json, Value};
use std::net::{SocketAddr, UdpSocket};
use std::time::Duration;

const SSDP_ADDR: &str = "239.255.255.250";
const SSDP_PORT: u16 = 1900;

pub struct KodiClient {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub timeout: Duration,
    client: reqwest::blocking::Client,
}

impl KodiClient {
    pub fn new(host: &str, port: u16, user: &str, password: &str, timeout_secs: f64) -> Self {
        KodiClient {
            host: host.to_string(),
            port,
            user: user.to_string(),
            password: password.to_string(),
            timeout: Duration::from_secs_f64(timeout_secs.max(0.1)),
            client: reqwest::blocking::Client::builder().build().unwrap(),
        }
    }

    fn url(&self) -> String {
        format!("http://{}:{}/jsonrpc", self.host, self.port)
    }

    /// Basic auth header encoded from UTF-8 bytes (Kodi accepts UTF-8 creds;
    /// this avoids the latin-1 breakage of stock Basic-auth helpers).
    fn auth_header(&self) -> Option<String> {
        if self.user.is_empty() && self.password.is_empty() {
            return None;
        }
        let raw = format!("{}:{}", self.user, self.password);
        let enc = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());
        Some(format!("Basic {enc}"))
    }

    fn rpc(&self, method: &str, params: Option<Value>) -> Result<Value, String> {
        let mut body = json!({"jsonrpc": "2.0", "id": 1, "method": method});
        if let Some(p) = params {
            body["params"] = p;
        }
        let mut req = self
            .client
            .post(self.url())
            .timeout(self.timeout)
            .json(&body);
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        let resp = req.send().map_err(|e| format!("Kodi unreachable: {e}"))?;
        let status = resp.status();
        if status.as_u16() == 401 {
            return Err("Kodi: 401 Unauthorized — check user/password.".to_string());
        }
        let text = resp.text().unwrap_or_default();
        if status.as_u16() != 200 {
            return Err(format!("Kodi HTTP {}: {}", status.as_u16(), truncate(&text, 300)));
        }
        let data: Value = serde_json::from_str(&text)
            .map_err(|_| "Kodi returned non-JSON response.".to_string())?;
        if let Some(err) = data.get("error") {
            return Err(format!(
                "Kodi RPC error {}: {}",
                err.get("code").and_then(|c| c.as_i64()).unwrap_or(0),
                err.get("message").and_then(|m| m.as_str()).unwrap_or("")
            ));
        }
        Ok(data.get("result").cloned().unwrap_or(Value::Null))
    }

    pub fn ping(&self) -> bool {
        self.ping_with_reason().0
    }

    pub fn ping_with_reason(&self) -> (bool, String) {
        match self.rpc("JSONRPC.Ping", None) {
            Ok(res) => {
                let ok = res == json!("pong") || res.as_bool().unwrap_or(false) || !res.is_null();
                if ok {
                    (true, String::new())
                } else {
                    (false, format!("Unexpected reply: {res:?}"))
                }
            }
            Err(e) => (false, e),
        }
    }

    pub fn get_version(&self) -> String {
        match self.rpc(
            "Application.GetProperties",
            Some(json!({"properties": ["version", "name"]})),
        ) {
            Ok(res) => {
                if let Some(v) = res.get("version") {
                    let major = v.get("major").and_then(|x| x.as_i64());
                    let minor = v.get("minor").and_then(|x| x.as_i64());
                    match (major, minor) {
                        (Some(ma), Some(mi)) => return format!("{ma}.{mi}"),
                        (Some(ma), None) => return format!("{ma}"),
                        _ => {}
                    }
                }
                res.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string()
            }
            Err(_) => String::new(),
        }
    }

    pub fn get_sources(&self, media: &str) -> Result<Vec<Value>, String> {
        let res = self.rpc("Files.GetSources", Some(json!({"media": media})))?;
        Ok(res.get("sources").and_then(|s| s.as_array()).cloned().unwrap_or_default())
    }

    pub fn get_directory(&self, path: &str, media: &str) -> Result<Vec<Value>, String> {
        let res = self.rpc(
            "Files.GetDirectory",
            Some(json!({"directory": path, "media": media, "properties": ["file", "title"]})),
        )?;
        Ok(res.get("files").and_then(|f| f.as_array()).cloned().unwrap_or_default())
    }

    pub fn get_active_video_player_id(&self) -> Option<i64> {
        let res = self.rpc("Player.GetActivePlayers", None).ok()?;
        let items = res.as_array().cloned().unwrap_or_default();
        for p in items {
            if p.get("type").and_then(|t| t.as_str()) == Some("video") {
                return p.get("playerid").and_then(|id| id.as_i64());
            }
        }
        None
    }

    pub fn play_file(&self, kodi_path: &str) -> Result<(), String> {
        self.rpc("Player.Open", Some(json!({"item": {"file": kodi_path}})))?;
        Ok(())
    }

    pub fn show_notification(&self, title: &str, message: &str, displaytime_ms: i64, image: &str) -> bool {
        self.rpc(
            "GUI.ShowNotification",
            Some(json!({"title": title, "message": message, "displaytime": displaytime_ms, "image": image})),
        )
        .is_ok()
    }

    /// Return raw `Player.GetProperties` for the active video player plus the
    /// playing item under `_item`. `Ok(None)` = reachable but no active video.
    pub fn get_player_progress(&self) -> Result<Option<Value>, String> {
        let res = self.rpc("Player.GetActivePlayers", None)?;
        let items = res.as_array().cloned().unwrap_or_default();
        let mut pid = None;
        for p in items {
            if p.get("type").and_then(|t| t.as_str()) == Some("video") {
                pid = p.get("playerid").and_then(|id| id.as_i64());
                break;
            }
        }
        let pid = match pid {
            Some(p) => p,
            None => return Ok(None),
        };
        let mut data = self.rpc(
            "Player.GetProperties",
            Some(json!({
                "playerid": pid,
                "properties": ["time", "totaltime", "percentage", "speed", "subtitles", "currentsubtitle", "subtitleenabled"]
            })),
        )?;
        let item = self
            .rpc("Player.GetItem", Some(json!({"playerid": pid, "properties": ["file", "title"]})))
            .ok()
            .and_then(|v| v.get("item").cloned())
            .unwrap_or(json!({}));
        data["_item"] = item;
        Ok(Some(data))
    }

    /// Attach an external SRT and switch to it (filename → language → last-added),
    /// then nudge a -0.5s seek to force overlay refresh only while paused.
    pub fn set_subtitle(
        &self,
        srt_path: &str,
        target_lang: Option<&str>,
        enable: bool,
        mut log: impl FnMut(String),
    ) -> Result<(), String> {
        let pid = self
            .get_active_video_player_id()
            .ok_or("No active video player on Kodi.")?;

        log(format!("Kodi: AddSubtitle path={srt_path}"));
        self.rpc("Player.AddSubtitle", Some(json!({"playerid": pid, "subtitle": srt_path})))?;
        if !enable {
            return Ok(());
        }

        let subs = self
            .rpc("Player.GetProperties", Some(json!({"playerid": pid, "properties": ["subtitles"]})))
            .ok()
            .and_then(|p| p.get("subtitles").and_then(|s| s.as_array()).cloned())
            .unwrap_or_default();
        log(format!("Kodi: subtitles list size={}", subs.len()));

        let mut chosen: Option<usize> = None;
        if !subs.is_empty() {
            let base_name = srt_path.trim_end_matches('/').rsplit(['/', '\\']).next().unwrap_or("").to_lowercase();
            let base_stem = base_name.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_else(|| base_name.clone());
            // a. filename
            if !base_name.is_empty() {
                for (i, s) in subs.iter().enumerate() {
                    let name = s.get("name").and_then(|n| n.as_str()).unwrap_or("").to_lowercase();
                    if name.is_empty() {
                        continue;
                    }
                    if name.contains(&base_name) || base_name.contains(&name)
                        || (!base_stem.is_empty() && (name.contains(&base_stem) || base_stem.contains(&name)))
                    {
                        chosen = Some(i);
                        break;
                    }
                }
            }
            // b. language
            if chosen.is_none() {
                if let Some(tl) = target_lang.map(|t| t.to_lowercase()) {
                    let tl = tl.trim();
                    if !tl.is_empty() {
                        for (i, s) in subs.iter().enumerate() {
                            let lang = s.get("language").and_then(|l| l.as_str()).unwrap_or("").to_lowercase();
                            let lang = lang.trim();
                            if !lang.is_empty() && (lang.starts_with(tl) || tl.starts_with(lang)) {
                                chosen = Some(i);
                                break;
                            }
                        }
                    }
                }
            }
            // c. last-added
            if chosen.is_none() {
                chosen = Some(subs.len() - 1);
            }
        }

        if let Some(idx) = chosen {
            log(format!("Kodi: SetSubtitle index={idx} enable=true"));
            let _ = self.rpc(
                "Player.SetSubtitle",
                Some(json!({"playerid": pid, "subtitle": idx, "enable": true})),
            );
        }

        // Seek refresh only when paused.
        if let Ok(tprops) = self.rpc("Player.GetProperties", Some(json!({"playerid": pid, "properties": ["time", "speed"]}))) {
            let speed = tprops.get("speed").and_then(|s| s.as_i64()).unwrap_or(1);
            if speed != 0 {
                return Ok(());
            }
            let t = tprops.get("time").cloned().unwrap_or(json!({}));
            let g = |k: &str| t.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
            let cur_ms = g("hours") * 3_600_000 + g("minutes") * 60_000 + g("seconds") * 1000 + g("milliseconds");
            let target = (cur_ms - 500).max(0);
            let seek = json!({
                "hours": target / 3_600_000,
                "minutes": (target / 60_000) % 60,
                "seconds": (target / 1000) % 60,
                "milliseconds": target % 1000,
            });
            let _ = self.rpc("Player.Seek", Some(json!({"playerid": pid, "value": {"time": seek}})));
        }
        Ok(())
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

// ---- path mapping ----

/// Translate a local filesystem path to a Kodi-visible path (e.g. smb://…).
pub fn map_local_to_kodi(local_file: &str, local_parent: &str, kodi_parent: &str) -> Result<String, String> {
    if local_parent.is_empty() {
        return Err("Local parent folder is not configured. Set it on the Kodi tab.".to_string());
    }
    if kodi_parent.is_empty() {
        return Err("Kodi source path is not configured. Set it on the Kodi tab.".to_string());
    }
    let lf = local_file.replace('\\', "/");
    let lp = local_parent.replace('\\', "/");
    let lp = lp.trim_end_matches('/');
    let rel: String = if lf == lp {
        String::new()
    } else if let Some(stripped) = lf.strip_prefix(&format!("{lp}/")) {
        stripped.to_string()
    } else {
        return Err("File is outside the configured local parent folder.".to_string());
    };
    let parent = if kodi_parent.ends_with('/') {
        kodi_parent.to_string()
    } else {
        format!("{kodi_parent}/")
    };
    Ok(format!("{parent}{rel}"))
}

/// Inverse of [`map_local_to_kodi`].
pub fn map_kodi_to_local(kodi_file: &str, kodi_parent: &str, local_parent: &str) -> Result<String, String> {
    if local_parent.is_empty() {
        return Err("Local parent folder is not configured.".to_string());
    }
    if kodi_parent.is_empty() {
        return Err("Kodi source path is not configured.".to_string());
    }
    let parent = if kodi_parent.ends_with('/') {
        kodi_parent.to_string()
    } else {
        format!("{kodi_parent}/")
    };
    let rel: String = if let Some(stripped) = kodi_file.strip_prefix(&parent) {
        stripped.to_string()
    } else if kodi_file.trim_end_matches('/') == kodi_parent.trim_end_matches('/') {
        String::new()
    } else {
        return Err(format!("Kodi file {kodi_file:?} is outside Kodi source {kodi_parent:?}."));
    };
    let lp = local_parent.trim_end_matches('/');
    Ok(format!("{lp}/{rel}"))
}

// ---- discovery ----

#[derive(Debug, Clone, serde::Serialize)]
pub struct KodiInstance {
    pub ip: String,
    pub port: u16,
    pub name: String,
    pub source: String,
}

fn local_subnet() -> Option<String> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:53").ok()?;
    let ip = sock.local_addr().ok()?.ip().to_string();
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    Some(format!("{}.{}.{}.", parts[0], parts[1], parts[2]))
}

fn ssdp_search(timeout: Duration) -> Vec<String> {
    let msg = format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {SSDP_ADDR}:{SSDP_PORT}\r\nMAN: \"ssdp:discover\"\r\nMX: {}\r\nST: ssdp:all\r\n\r\n",
        timeout.as_secs().max(1)
    );
    let sock = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    let _ = sock.set_multicast_ttl_v4(2);
    let _ = sock.set_read_timeout(Some(timeout));
    let addr: SocketAddr = format!("{SSDP_ADDR}:{SSDP_PORT}").parse().unwrap();
    if sock.send_to(msg.as_bytes(), addr).is_err() {
        return vec![];
    }
    let re = Regex::new(r"(?im)^LOCATION:\s*(.+?)\s*$").unwrap();
    let mut locations = Vec::new();
    let mut buf = [0u8; 65535];
    loop {
        match sock.recv_from(&mut buf) {
            Ok((n, _)) => {
                let text = String::from_utf8_lossy(&buf[..n]);
                if let Some(c) = re.captures(&text) {
                    let loc = c[1].to_string();
                    if !locations.contains(&loc) {
                        locations.push(loc);
                    }
                }
            }
            Err(_) => break, // timeout
        }
    }
    locations
}

fn host_from_location(loc: &str) -> Option<String> {
    // Parse "http://HOST:PORT/..." → HOST.
    let after = loc.split("://").nth(1)?;
    let hostport = after.split('/').next()?;
    let host = hostport.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

fn is_kodi_at(ip: &str, port: u16, timeout: Duration) -> Option<KodiInstance> {
    let url = format!("http://{ip}:{port}/jsonrpc");
    let client = reqwest::blocking::Client::builder().timeout(timeout).build().ok()?;
    let resp = client
        .post(&url)
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "JSONRPC.Ping"}))
        .send()
        .or_else(|_| client.get(&url).send())
        .ok()?;
    let code = resp.status().as_u16();
    let text = resp.text().unwrap_or_default();
    let low = text.to_lowercase();
    let looks_like_kodi = text.contains("JSONRPC") || low.contains("kodi") || low.contains("xbmc") || code == 401;
    if code == 200 {
        match serde_json::from_str::<Value>(&text) {
            Ok(j) if j.is_object() && (j.get("jsonrpc").is_some() || j.get("result").is_some() || j.get("error").is_some()) => {}
            _ => return None,
        }
    } else if code != 401 {
        return None;
    }
    if !looks_like_kodi && code != 401 {
        return None;
    }
    Some(KodiInstance {
        ip: ip.to_string(),
        port,
        name: format!("Kodi @ {ip}"),
        source: "scan".to_string(),
    })
}

/// Discover Kodi instances: SSDP first, then a /24 subnet scan if nothing found.
pub fn discover_kodi(port_hint: u16, ssdp_timeout: Duration, scan_timeout: Duration, do_scan: bool) -> Vec<KodiInstance> {
    let mut found: Vec<KodiInstance> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let locations = ssdp_search(ssdp_timeout);
    let mut candidates = Vec::new();
    for loc in &locations {
        if let Some(host) = host_from_location(loc) {
            if seen.insert(host.clone()) {
                candidates.push(host);
            }
        }
    }
    for ip in candidates {
        if let Some(mut info) = is_kodi_at(&ip, port_hint, scan_timeout) {
            info.source = "ssdp".to_string();
            found.push(info);
        }
    }

    if do_scan && found.is_empty() {
        if let Some(prefix) = local_subnet() {
            let ips: Vec<String> = (1..=254).map(|i| format!("{prefix}{i}")).collect();
            let hits: Vec<KodiInstance> = ips
                .par_iter()
                .filter_map(|ip| is_kodi_at(ip, port_hint, scan_timeout))
                .collect();
            for info in hits {
                if seen.insert(info.ip.clone()) {
                    found.push(info);
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_local_to_kodi_happy_path() {
        let out = map_local_to_kodi("/Volumes/m/dir/x.mkv", "/Volumes/m", "smb://nas/movies").unwrap();
        assert_eq!(out, "smb://nas/movies/dir/x.mkv");
    }

    #[test]
    fn map_local_to_kodi_trailing_slash_parent() {
        let out = map_local_to_kodi("/Volumes/m/dir/x.mkv", "/Volumes/m", "smb://nas/movies/").unwrap();
        assert_eq!(out, "smb://nas/movies/dir/x.mkv");
    }

    #[test]
    fn map_local_to_kodi_nested() {
        let out = map_local_to_kodi("/Volumes/m/a/b/c/x.mkv", "/Volumes/m", "smb://nas/movies").unwrap();
        assert_eq!(out, "smb://nas/movies/a/b/c/x.mkv");
    }

    #[test]
    fn map_local_to_kodi_outside_parent_errors() {
        assert!(map_local_to_kodi("/other/x.mkv", "/Volumes/m", "smb://nas/movies").is_err());
    }

    #[test]
    fn map_local_to_kodi_empty_parent_errors() {
        assert!(map_local_to_kodi("/Volumes/m/x.mkv", "", "smb://nas/movies").is_err());
        assert!(map_local_to_kodi("/Volumes/m/x.mkv", "/Volumes/m", "").is_err());
    }

    #[test]
    fn map_local_to_kodi_windows_separator() {
        let out = map_local_to_kodi("C:\\media\\dir\\x.mkv", "C:\\media", "smb://nas/movies").unwrap();
        assert_eq!(out, "smb://nas/movies/dir/x.mkv");
    }

    #[test]
    fn map_kodi_to_local_roundtrip() {
        let out = map_kodi_to_local("smb://nas/movies/dir/x.mkv", "smb://nas/movies", "/Volumes/m").unwrap();
        assert_eq!(out, "/Volumes/m/dir/x.mkv");
    }

    #[test]
    fn host_from_location_parses() {
        assert_eq!(host_from_location("http://192.168.1.5:8080/desc.xml").as_deref(), Some("192.168.1.5"));
    }
}
