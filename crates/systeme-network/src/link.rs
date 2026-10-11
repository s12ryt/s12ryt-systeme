//! .link 文件解析 ([Match]/[Link])。

use crate::network::NetworkMatch;
use systeme_units::parser::ParsedUnit;

/// .link 文件配置 (udev 替代方案的接口層設置)。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LinkConfig {
    pub match_: NetworkMatch,
    pub mac: Option<String>,
    pub description: Option<String>,
    pub mtu_bytes: Option<u64>,
    pub warnings: Vec<String>,
}

impl LinkConfig {
    pub fn from_parsed(p: &ParsedUnit) -> Self {
        let match_ = NetworkMatch::from_parsed(p);
        let mut mac = None;
        let mut description = None;
        let mut mtu_bytes = None;
        let mut warnings = Vec::new();

        if let Some(s) = p.section("Link") {
            if let Some(v) = s.get("MACAddress") {
                if !v.trim().is_empty() {
                    mac = Some(v.trim().to_string());
                }
            }
            if let Some(v) = s.get("Description") {
                if !v.trim().is_empty() {
                    description = Some(v.trim().to_string());
                }
            }
            if let Some(v) = s.get("MTUBytes") {
                match parse_size_bytes(v) {
                    Some(n) => mtu_bytes = Some(n),
                    None => warnings.push(format!("Ignoring invalid MTUBytes value: {v}")),
                }
            }
        }

        LinkConfig {
            match_,
            mac,
            description,
            mtu_bytes,
            warnings,
        }
    }
}

/// 解析帶可選 K/M/G 後綴的字節數 (1024 進制); 壞值 None。
pub fn parse_size_bytes(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, mult) = if let Some(rest) = s.strip_suffix(['k', 'K']) {
        (rest, 1024u64)
    } else if let Some(rest) = s.strip_suffix(['m', 'M']) {
        (rest, 1024 * 1024)
    } else if let Some(rest) = s.strip_suffix(['g', 'G']) {
        (rest, 1024 * 1024 * 1024)
    } else {
        (s, 1)
    };
    num.trim().parse::<u64>().ok().and_then(|v| v.checked_mul(mult))
}

#[cfg(test)]
mod tests {
    use super::parse_size_bytes;

    #[test]
    fn size_bytes_plain() {
        assert_eq!(parse_size_bytes("1500"), Some(1500));
        assert_eq!(parse_size_bytes(""), None);
        assert_eq!(parse_size_bytes("-1"), None);
    }

    #[test]
    fn size_bytes_overflow_rejected() {
        assert_eq!(parse_size_bytes("18446744073709551616G"), None);
    }
}
