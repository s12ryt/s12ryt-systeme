//! systeme-login: logind 會話/席位基礎 — 純邏輯層。
//!
//! 會話生命周期 (create/close/active 提升)、席位關聯、鎖定/閒置提示。
//! bin 層 (真實 utmp/PAM) 留 cfg(unix) 集成。

pub mod cli;
pub mod render;
pub mod sessions;

pub use sessions::{
    find_seat_of_session, Session, SessionState, Sessions,
};

/// 席位類型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeatKind {
    /// 真實席位 (seat0, seat1, ...)
    Physical,
    /// 虛擬/無席位
    Virtual,
}

impl SeatKind {
    /// 按名稱分類: "seat" 前綴 + 數字視為實體席位。
    pub fn classify(name: &str) -> SeatKind {
        if let Some(rest) = name.strip_prefix("seat") {
            if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
                return SeatKind::Physical;
            }
        }
        SeatKind::Virtual
    }
}
