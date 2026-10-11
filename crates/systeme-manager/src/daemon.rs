//! DaemonCore: IPC 請求 → ManagerCore 調度的接線層。
//!
//! 純邏輯、無 I/O 副作用 (進程/文件系統全部經由 trait 注入), 跨平台可測。
//! 守護進程二進制 (`src/bin/systemd.rs`) 把 socket 收到的 [`Request`]
//! 餵給 [`DaemonCore::handle`], 並將返回的 [`Response`] 序列化回寫。

use std::collections::BTreeMap;

use systeme_core::ipc::{unit_info_of, Request, Response};
use systeme_core::manager::{ActiveState, ManagerCore, ServiceExecutor};
use systeme_core::mount::{automount_target, MountConfig};
use systeme_core::socket::{socket_service_target, SocketConfig};
use systeme_units::search::{load_unit, Loaded, SearchPaths, UnitFs};

use crate::mounts::{real_mount, real_umount};
use crate::sockets::{bind_socket_unit, SocketActivation};

/// 守護進程核心: ManagerCore + 搜索路徑 + 請求分發。
pub struct DaemonCore {
    pub manager: ManagerCore,
    pub search_paths: SearchPaths,
    /// 已激活 socket 單元 → 綁定 listeners (drop 即關閉監聽)
    pub sockets: BTreeMap<String, SocketActivation>,
}

impl DaemonCore {
    pub fn new(manager: ManagerCore, search_paths: SearchPaths) -> Self {
        DaemonCore {
            manager,
            search_paths,
            sockets: BTreeMap::new(),
        }
    }

    /// 處理一條 IPC 請求。
    pub fn handle(
        &mut self,
        req: &Request,
        exec: &mut dyn ServiceExecutor,
        fs: &dyn UnitFs,
    ) -> Response {
        match req {
            Request::ListUnits => Response::Units(
                self.manager.units.values().map(unit_info_of).collect(),
            ),

            Request::Status { unit } => match self.manager.unit(unit) {
                Some(entry) => Response::UnitStatus(unit_info_of(entry)),
                None => not_found(unit),
            },

            Request::Start { unit } => self.start(unit, exec),

            Request::Stop { unit } => self.stop(unit, exec),

            // restart = stop + start; stop 對 inactive 單元冪等成功, 因此
            // 「restart 未運行單元」等價於 start (systemd 語義)。
            Request::Restart { unit } => {
                let stopped = self.stop(unit, exec);
                if !matches!(stopped, Response::Ok) {
                    return stopped;
                }
                self.start(unit, exec)
            }

            Request::IsActive { unit } => match self.manager.active_state(unit) {
                Some(state) => Response::Active(state == ActiveState::Active),
                None => not_found(unit),
            },

            Request::Cat { unit } => self.cat(unit, fs),

            Request::DaemonReload => {
                let sp = self.search_paths.clone();
                self.manager.load_all(fs, &sp);
                Response::Ok
            }

            // 文件系統操作族: 由 systemctl 客戶端側或後續版本實現
            Request::Enable { .. } => Response::NotImplemented {
                request: "enable".into(),
            },
            Request::Disable { .. } => Response::NotImplemented {
                request: "disable".into(),
            },
            Request::Mask { .. } => Response::NotImplemented {
                request: "mask".into(),
            },
            Request::Unmask { .. } => Response::NotImplemented {
                request: "unmask".into(),
            },
            // reload (unit) 需要 ExecReload 支持, 後續版本
            Request::Reload { .. } => Response::NotImplemented {
                request: "reload".into(),
            },
        }
    }

    // ─── start/stop 分發 (含非 service 單元攔截) ────────────────────

    fn start(&mut self, unit: &str, exec: &mut dyn ServiceExecutor) -> Response {
        if unit.ends_with(".socket") {
            return self.start_socket(unit, exec);
        }
        if unit.ends_with(".mount") {
            return self.start_mount(unit, exec);
        }
        if unit.ends_with(".automount") {
            return self.start_automount(unit, exec);
        }
        match self.manager.start_unit(unit, exec) {
            Ok(_) => Response::Ok,
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        }
    }

    /// 啟動 .socket: 綁定監聽 → 標 Active → fd 預載 → 拉起目標 service。
    ///
    /// 簡化: 提前啟動關聯 service (systemd 的「第一連接才啟動」惰性激活
    /// 需要 watcher, 留 bin 層後續); 事務閉包內的 .socket 依賴不做攔截。
    fn start_socket(&mut self, unit: &str, exec: &mut dyn ServiceExecutor) -> Response {
        let Some(entry) = self.manager.unit(unit) else {
            return not_found(unit);
        };
        if entry.masked {
            return masked(unit);
        }
        // 冪等: 已綁定即成功
        if self.sockets.contains_key(unit) {
            return Response::Ok;
        }
        let cfg = SocketConfig::from_parsed(&entry.parsed);
        let activation = match bind_socket_unit(&cfg) {
            Ok(a) => a,
            Err(e) => return Response::Error { message: e },
        };
        if let Err(e) = self.manager.start_unit(unit, exec) {
            return Response::Error {
                message: e.to_string(),
            };
        }
        let fds = activation.fds();
        self.sockets.insert(unit.to_string(), activation);

        let target = socket_service_target(&cfg, unit);
        exec.prepare_socket_fds(&fds);
        match self.manager.start_unit(&target, exec) {
            Ok(_) => Response::Ok,
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        }
    }

    /// 啟動 .mount: 真實掛載成功才標 Active。
    fn start_mount(&mut self, unit: &str, exec: &mut dyn ServiceExecutor) -> Response {
        let Some(entry) = self.manager.unit(unit) else {
            return not_found(unit);
        };
        if entry.masked {
            return masked(unit);
        }
        let cfg = MountConfig::from_parsed(&entry.parsed);
        if let Err(e) = real_mount(&cfg) {
            return Response::Error { message: e };
        }
        match self.manager.start_unit(unit, exec) {
            Ok(_) => Response::Ok,
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        }
    }

    /// 啟動 .automount: 簡化 — 自身直達 Active; 對應 .mount 存在則一併掛載。
    /// (真正的 autofs 需要 kernel 支持, 由對應 .mount 提前掛載代替)
    fn start_automount(&mut self, unit: &str, exec: &mut dyn ServiceExecutor) -> Response {
        let Some(entry) = self.manager.unit(unit) else {
            return not_found(unit);
        };
        if entry.masked {
            return masked(unit);
        }
        let target = automount_target(unit);
        if target.ends_with(".mount") && self.manager.units.contains_key(&target) {
            let r = self.start_mount(&target, exec);
            if !matches!(r, Response::Ok) {
                return r;
            }
        }
        match self.manager.start_unit(unit, exec) {
            Ok(_) => Response::Ok,
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        }
    }

    fn stop(&mut self, unit: &str, exec: &mut dyn ServiceExecutor) -> Response {
        if unit.ends_with(".socket") {
            // 移除 listeners (drop 關閉 fd) + 標 Inactive
            self.sockets.remove(unit);
        } else if unit.ends_with(".mount") {
            // Active 才真實 umount; inactive 冪等跳過
            if matches!(self.manager.active_state(unit), Some(ActiveState::Active)) {
                let cfg = MountConfig::from_parsed(&self.manager.unit(unit).unwrap().parsed);
                if let Err(e) = real_umount(&cfg) {
                    return Response::Error { message: e };
                }
            }
        }
        // .automount: 簡化 — 僅標 Inactive (對應 .mount 不動)
        match self.manager.stop_unit(unit, exec) {
            Ok(_) => Response::Ok,
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        }
    }

    /// cat: 從搜索路徑重新讀取單元文件原文。
    fn cat(&self, unit: &str, fs: &dyn UnitFs) -> Response {
        match load_unit(unit, &self.search_paths, fs) {
            None => not_found(unit),
            Some(Loaded::Masked(_)) => Response::Error {
                message: format!("Unit {unit} is masked."),
            },
            Some(Loaded::Unit(found, _)) => match fs.read(&found.path) {
                Ok(text) => Response::Cat {
                    name: found.real_name.clone(),
                    text,
                },
                Err(e) => Response::Error {
                    message: format!("failed to read {}: {e}", found.path.display()),
                },
            },
        }
    }
}

fn not_found(unit: &str) -> Response {
    Response::Error {
        message: format!("Unit {unit} could not be found."),
    }
}

fn masked(unit: &str) -> Response {
    Response::Error {
        message: format!("Unit {unit} is masked."),
    }
}
