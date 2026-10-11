//! 會話追蹤純邏輯。

use std::collections::BTreeMap;

/// 會話狀態 (logind 語義)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// 已登入非當前
    Online,
    /// 席位當前活躍
    Active,
    /// 正在關閉
    Closing,
}

impl SessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionState::Online => "online",
            SessionState::Active => "active",
            SessionState::Closing => "closing",
        }
    }
}

/// 單個會話。
#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub user: String,
    pub uid: u32,
    pub seat: Option<String>,
    pub tty: Option<String>,
    pub state: SessionState,
    pub locked: bool,
    pub idle: bool,
}

/// 會話表。
#[derive(Debug, Default)]
pub struct Sessions {
    next_id: u32,
    map: BTreeMap<String, Session>,
}

impl Sessions {
    pub fn new() -> Sessions {
        Sessions { next_id: 1, map: BTreeMap::new() }
    }

    /// 建立會話, 返回分配的 id ("1", "2", ... 單調遞增)。
    /// 同席位新會話成為 Active, 舊 Active 會話降為 Online。
    pub fn create(&mut self, user: &str, uid: u32, seat: Option<&str>, tty: &str) -> String {
        let id = self.next_id.to_string();
        self.next_id += 1;
        let seat = seat.filter(|s| !s.is_empty()).map(|s| s.to_string());
        let tty = if tty.is_empty() { None } else { Some(tty.to_string()) };

        if let Some(seat_name) = &seat {
            for s in self.map.values_mut() {
                if s.state == SessionState::Active && s.seat.as_deref() == Some(seat_name.as_str()) {
                    s.state = SessionState::Online;
                }
            }
        }

        let sess = Session {
            id: id.clone(),
            user: user.to_string(),
            uid,
            seat,
            tty,
            state: SessionState::Active,
            locked: false,
            idle: false,
        };
        self.map.insert(id.clone(), sess);
        id
    }

    pub fn get(&self, id: &str) -> Option<&Session> {
        self.map.get(id)
    }

    /// 關閉會話; 同席位剩餘最新 Online 會話提升為 Active。
    pub fn close(&mut self, id: &str) -> Result<(), String> {
        let removed = self.map.remove(id).ok_or_else(|| format!("No session '{id}'."))?;
        if let Some(seat_name) = removed.seat {
            if self.active_on_seat(&seat_name).is_none() {
                // 提升同席位最新的非 Closing 會話
                let promote = self
                    .map
                    .values()
                    .filter(|s| s.seat.as_deref() == Some(seat_name.as_str()))
                    .filter(|s| s.state != SessionState::Closing)
                    .map(|s| s.id.clone())
                    .next_back();
                if let Some(pid) = promote {
                    if let Some(s) = self.map.get_mut(&pid) {
                        s.state = SessionState::Active;
                    }
                }
            }
        }
        Ok(())
    }

    /// 席位當前活躍會話 id (字典序最大即最新建立的 active)。
    pub fn active_on_seat(&self, seat: &str) -> Option<String> {
        self.map
            .values()
            .filter(|s| s.state == SessionState::Active && s.seat.as_deref() == Some(seat))
            .map(|s| s.id.clone())
            .next()
    }

    /// 全部會話按 id 排序。
    pub fn list(&self) -> Vec<&Session> {
        self.map.values().collect()
    }

    /// 指定 uid 的會話。
    pub fn sessions_of_user(&self, uid: u32) -> Vec<&Session> {
        self.map.values().filter(|s| s.uid == uid).collect()
    }

    pub fn set_locked(&mut self, id: &str, locked: bool) -> Result<(), String> {
        let s = self.map.get_mut(id).ok_or_else(|| format!("No session '{id}'."))?;
        s.locked = locked;
        Ok(())
    }

    pub fn set_idle(&mut self, id: &str, idle: bool) -> Result<(), String> {
        let s = self.map.get_mut(id).ok_or_else(|| format!("No session '{id}'."))?;
        s.idle = idle;
        Ok(())
    }
}

/// 查詢會話所在席位。
pub fn find_seat_of_session(sessions: &Sessions, id: &str) -> Option<String> {
    sessions.get(id).and_then(|s| s.seat.clone())
}
