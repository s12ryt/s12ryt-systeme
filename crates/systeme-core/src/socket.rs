//! socket 單元配置層 — [Socket] 段解析、Listen 目標分類、fd 傳遞環境構造。
//!
//! 純邏輯層: 不創建真實 socket, 只負責解析與分類。
//! 真實監聽 / fd 傳遞 / Accept= 的 per-connection spawn 由 daemon 層 (cfg(unix)) 實現。

use systeme_units::parser::{parse_bool, ParsedUnit};

/// systemd 默認 Backlog (對應 somaxconn 的保守值)。
pub const DEFAULT_BACKLOG: u32 = 128;

/// 單個 Listen 條目的傳輸協議。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenProto {
    /// TCP stream (ListenStream=)
    Stream,
    /// UDP datagram (ListenDatagram=)
    Datagram,
    /// 命名管道 (ListenFIFO=)
    Fifo,
    /// 特殊文件 (ListenSpecial=)
    Special,
}

/// Listen 目標的分類結果 (協議無關)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListenKind {
    /// 純端口: 雙棧監聽 (`ListenStream=8080`)
    Port(u16),
    /// 位址:端口 (`192.168.1.10:8080`、`[::1]:80`、`host:80`)
    AddrPort { addr: String, port: u16 },
    /// 文件系統路徑: Unix socket / FIFO (`/run/foo.sock`)
    Path(String),
    /// Linux abstract namespace (`@foo`, 已剝 @ 前綴)
    Abstract(String),
    /// 其他形式, 保留原值交由 daemon 層解析/報錯
    Network(String),
}

/// 一條 Listen 聲明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenSpec {
    pub proto: ListenProto,
    pub kind: ListenKind,
}

/// [Socket] 段解析結果。
#[derive(Debug, Clone, PartialEq)]
pub struct SocketConfig {
    /// 按文件定義順序排列的監聽聲明 (跨 ListenStream/Datagram/FIFO/Special 保序)
    pub listen: Vec<ListenSpec>,
    /// Accept=yes: 每個連接 spawn 一個 service 實例
    pub accept: bool,
    /// 顯式 Service= 覆蓋 (None → 推導同名 .service)
    pub service: Option<String>,
    /// backlog 大小
    pub backlog: u32,
    /// 解析警告 (壞值降級, 不報錯)
    pub warnings: Vec<String>,
}

impl Default for SocketConfig {
    fn default() -> Self {
        Self {
            listen: Vec::new(),
            accept: false,
            service: None,
            backlog: DEFAULT_BACKLOG,
            warnings: Vec::new(),
        }
    }
}

impl SocketConfig {
    /// 從已解析單元提取 [Socket] 配置。缺失該段 → 全默認。
    ///
    /// 直接遍歷原始 entries 以保證跨 `Listen*` key 的文件順序,
    /// 並支持 systemd 語義「空值清空該 key 此前累積的條目」。
    pub fn from_parsed(unit: &ParsedUnit) -> Self {
        let mut cfg = Self::default();
        let Some(section) = unit.section("Socket") else {
            return cfg;
        };

        for entry in &section.entries {
            let proto = match entry.key.as_str() {
                "ListenStream" => ListenProto::Stream,
                "ListenDatagram" => ListenProto::Datagram,
                "ListenFIFO" => ListenProto::Fifo,
                "ListenSpecial" => ListenProto::Special,
                "Accept" => {
                    match parse_bool(&entry.value) {
                        Some(b) => cfg.accept = b,
                        None => cfg.warnings.push(format!(
                            "Accept value not boolean, ignoring: {:?}",
                            entry.value
                        )),
                    }
                    continue;
                }
                "Service" => {
                    if !entry.value.trim().is_empty() {
                        cfg.service = Some(entry.value.trim().to_string());
                    }
                    continue;
                }
                "Backlog" => {
                    match entry.value.trim().parse::<u32>() {
                        Ok(n) => cfg.backlog = n,
                        Err(_) => cfg.warnings.push(format!(
                            "Backlog value not a number, using default {}: {:?}",
                            DEFAULT_BACKLOG, entry.value
                        )),
                    }
                    continue;
                }
                _ => continue,
            };

            // Listen* 條目: 空值清空同 key 累積, 非空追加 (保序)
            if entry.value.trim().is_empty() {
                cfg.listen.retain(|s| s.proto != proto);
            } else {
                cfg.listen.push(ListenSpec {
                    proto,
                    kind: classify_listen(entry.value.trim()),
                });
            }
        }

        cfg
    }
}

/// 分類 Listen 目標字串。
pub fn classify_listen(value: &str) -> ListenKind {
    let v = value.trim();
    if v.starts_with('/') {
        return ListenKind::Path(v.to_string());
    }
    if let Some(name) = v.strip_prefix('@') {
        return ListenKind::Abstract(name.to_string());
    }
    if let Some((addr, port)) = v.rsplit_once(':') {
        if let Ok(port) = port.parse::<u16>() {
            let addr = addr.trim_start_matches('[').trim_end_matches(']');
            return ListenKind::AddrPort {
                addr: addr.to_string(),
                port,
            };
        }
        // 右段不是端口 → 整體當作未識別形式
        return ListenKind::Network(v.to_string());
    }
    if let Ok(port) = v.parse::<u16>() {
        return ListenKind::Port(port);
    }
    ListenKind::Network(v.to_string())
}

/// socket 激活的 service 目標: 顯式 Service= 優先, 否則剝 .socket 換 .service。
pub fn socket_service_target(cfg: &SocketConfig, socket_name: &str) -> String {
    if let Some(svc) = &cfg.service {
        return svc.clone();
    }
    socket_name
        .strip_suffix(".socket")
        .map(|base| format!("{base}.service"))
        .unwrap_or_else(|| socket_name.to_string())
}

/// sd_listen_fds 協議環境變量 (不含 LISTEN_PID — 需執行時填當前 pid)。
///
/// fd 約定從 3 起連續分配, 因此這裡只需要數量。
pub fn listen_fds_env(count: usize) -> Vec<(String, String)> {
    vec![("LISTEN_FDS".to_string(), count.to_string())]
}
