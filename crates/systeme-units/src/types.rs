//! 單元類型 (unit type) 與單元名規則。

use std::fmt;

/// systemd 支援的單元類型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitType {
    Service,
    Socket,
    Timer,
    Target,
    Mount,
    Automount,
    Path,
    Swap,
    Slice,
    Scope,
    Device,
}

pub const ALL_UNIT_TYPES: [UnitType; 11] = [
    UnitType::Service,
    UnitType::Socket,
    UnitType::Timer,
    UnitType::Target,
    UnitType::Mount,
    UnitType::Automount,
    UnitType::Path,
    UnitType::Swap,
    UnitType::Slice,
    UnitType::Scope,
    UnitType::Device,
];

impl UnitType {
    /// 從文件名後綴解析: "service" → Service
    pub fn from_suffix(suffix: &str) -> Option<UnitType> {
        match suffix {
            "service" => Some(UnitType::Service),
            "socket" => Some(UnitType::Socket),
            "timer" => Some(UnitType::Timer),
            "target" => Some(UnitType::Target),
            "mount" => Some(UnitType::Mount),
            "automount" => Some(UnitType::Automount),
            "path" => Some(UnitType::Path),
            "swap" => Some(UnitType::Swap),
            "slice" => Some(UnitType::Slice),
            "scope" => Some(UnitType::Scope),
            "device" => Some(UnitType::Device),
            _ => None,
        }
    }

    pub fn suffix(&self) -> &'static str {
        match self {
            UnitType::Service => "service",
            UnitType::Socket => "socket",
            UnitType::Timer => "timer",
            UnitType::Target => "target",
            UnitType::Mount => "mount",
            UnitType::Automount => "automount",
            UnitType::Path => "path",
            UnitType::Swap => "swap",
            UnitType::Slice => "slice",
            UnitType::Scope => "scope",
            UnitType::Device => "device",
        }
    }

    /// 從完整單元名 ("nginx.service") 解析 (前綴, 類型)
    pub fn split_name(name: &str) -> Option<(&str, UnitType)> {
        let dot = name.rfind('.')?;
        let (prefix, suffix) = name.split_at(dot);
        let ty = UnitType::from_suffix(&suffix[1..])?;
        Some((prefix, ty))
    }
}

impl fmt::Display for UnitType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.suffix())
    }
}

/// 單元名是否合法 (字母數字與 `_.@\:-` + 轉義的 %xx;長度 ≤ 255)
pub fn is_valid_unit_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 255 {
        return false;
    }
    name.split_once('.').map(|(prefix, _)| !prefix.is_empty()).unwrap_or(false)
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '@' | ':' | '-' | '\\'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_names() {
        assert_eq!(
            UnitType::split_name("nginx.service"),
            Some(("nginx", UnitType::Service))
        );
        assert_eq!(
            UnitType::split_name("dev-sda1.device"),
            Some(("dev-sda1", UnitType::Device))
        );
        assert_eq!(UnitType::split_name("foo.wrongtype"), None);
        assert_eq!(UnitType::split_name("nosuffix"), None);
    }

    #[test]
    fn validity() {
        assert!(is_valid_unit_name("nginx.service"));
        assert!(is_valid_unit_name("user@1000.service"));
        assert!(is_valid_unit_name("dev-sda1.device"));
        assert!(!is_valid_unit_name(".service"));
        assert!(!is_valid_unit_name(""));
    }
}
