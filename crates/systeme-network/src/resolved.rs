//! systemd-resolved 基礎: resolv.conf 解析/渲染、127.0.0.53 stub 配置、上游 DNS 聚合。
//!
//! bin 層 (systemd-resolved) 用這些純函數管理 stub resolv.conf 並轉發 DNS 查詢。

use crate::network::NetworkConfig;
use std::collections::BTreeSet;

/// 解析後的 resolv.conf 內容。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvConf {
    pub nameservers: Vec<String>,
    pub search: Vec<String>,
    pub options: Vec<String>,
}

/// 解析 resolv.conf 文本。`#`/`;` 開頭為註釋, 空行容忍;
/// `domain` 作為 `search` 的別名接受 (單值); 未知指令忽略 (高兼容不報錯)。
pub fn parse_resolv_conf(text: &str) -> ResolvConf {
    let mut rc = ResolvConf::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let mut it = line.split_whitespace();
        let Some(keyword) = it.next() else { continue };
        match keyword {
            "nameserver" => {
                if let Some(ns) = it.next() {
                    rc.nameservers.push(ns.to_string());
                }
            }
            "search" | "domain" => rc.search.extend(it.map(str::to_string)),
            "options" => rc.options.extend(it.map(str::to_string)),
            _ => {} // sortlist 等罕見指令忽略
        }
    }
    rc
}

/// 生成指向本機 stub (127.0.0.53) 的 resolv.conf 配置。
pub fn stub_resolv_conf() -> ResolvConf {
    ResolvConf {
        nameservers: vec!["127.0.0.53".to_string()],
        search: Vec::new(),
        options: Vec::new(),
    }
}

/// 渲染為 systemd 風格的 resolv.conf 文本 (帶託管頭註釋)。
pub fn render_resolv_conf(cfg: &ResolvConf) -> String {
    let mut out = String::new();
    out.push_str("# This file is managed by man:systemd-resolved(8). Do not edit.\n#\n");
    out.push_str("# This is a dynamic resolv.conf file for connecting local clients to the\n");
    out.push_str("# known DNS servers.\n\n");
    for ns in &cfg.nameservers {
        out.push_str("nameserver ");
        out.push_str(ns);
        out.push('\n');
    }
    if !cfg.search.is_empty() {
        out.push_str("search ");
        out.push_str(&cfg.search.join(" "));
        out.push('\n');
    }
    if !cfg.options.is_empty() {
        out.push_str("options ");
        out.push_str(&cfg.options.join(" "));
        out.push('\n');
    }
    out
}

/// 從多個 .network 配置聚合上游 DNS, 去重並保留首次出現順序。
pub fn collect_dns(networks: &[NetworkConfig]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for n in networks {
        for d in &n.dns {
            if seen.insert(d.clone()) {
                out.push(d.clone());
            }
        }
    }
    out
}
