use crate::models::data_dir;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::PathBuf,
    sync::Mutex,
};
use tauri::AppHandle;

static LOG_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionLogEntry {
    pub timestamp: u64,
    pub host: String,
    pub rule: String,
    pub exit: String,
    pub failed: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLogEntry {
    pub started_at: u64,
    pub ended_at: u64,
    pub upload: u64,
    pub download: u64,
    #[serde(default)]
    pub by_exit: Value,
    #[serde(default)]
    pub by_rule: Value,
    #[serde(default)]
    pub by_pair: Value,
}

#[cfg(target_os = "windows")]
fn local_date_and_time() -> (String, String) {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTimeWin {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocalTime(lp_system_time: *mut SystemTimeWin);
    }
    let mut st = SystemTimeWin::default();
    unsafe {
        GetLocalTime(&mut st);
    }
    let date = format!("{:04}-{:02}-{:02}", st.year, st.month, st.day);
    let time = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        st.year, st.month, st.day, st.hour, st.minute, st.second, st.milliseconds
    );
    (date, time)
}

#[cfg(not(target_os = "windows"))]
fn local_date_and_time() -> (String, String) {
    ("current".into(), String::new())
}

fn logs_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app).join("logs");
    fs::create_dir_all(&dir).map_err(|e| format!("创建日志目录失败：{e}"))?;
    Ok(dir)
}

fn prune_old_connection_logs(dir: &PathBuf, keep_files: usize) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("connections-") && n.ends_with(".jsonl"))
                .unwrap_or(false)
        })
        .collect();
    if files.len() <= keep_files {
        return;
    }
    files.sort();
    let remove_count = files.len() - keep_files;
    for path in files.into_iter().take(remove_count) {
        let _ = fs::remove_file(path);
    }
}

#[tauri::command]
pub async fn append_connection_logs(
    app: AppHandle,
    entries: Vec<ConnectionLogEntry>,
) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = LOG_WRITE_LOCK.lock().expect("log write lock");
        let dir = logs_dir(&app)?;
        let (date, local_now) = local_date_and_time();
        let file_path = dir.join(format!("connections-{date}.jsonl"));
        let is_new = !file_path.exists();

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .map_err(|e| format!("打开连接日志文件失败：{e}"))?;
        let mut writer = BufWriter::new(file);

        for entry in entries {
            let line = serde_json::json!({
                "timestamp": entry.timestamp,
                "localTime": local_now,
                "host": entry.host,
                "rule": entry.rule,
                "exit": entry.exit,
                "failed": entry.failed,
                "reason": entry.reason,
            });
            serde_json::to_writer(&mut writer, &line).map_err(|e| e.to_string())?;
            writer.write_all(b"\n").map_err(|e| e.to_string())?;
        }
        writer.flush().map_err(|e| e.to_string())?;

        if is_new {
            prune_old_connection_logs(&dir, 30);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn append_session_log(app: AppHandle, session: SessionLogEntry) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = LOG_WRITE_LOCK.lock().expect("log write lock");
        let dir = logs_dir(&app)?;
        let (_, local_now) = local_date_and_time();
        let file_path = dir.join("sessions.jsonl");

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .map_err(|e| format!("打开会话日志文件失败：{e}"))?;
        let mut writer = BufWriter::new(file);
        let line = serde_json::json!({
            "endedLocalTime": local_now,
            "startedAt": session.started_at,
            "endedAt": session.ended_at,
            "upload": session.upload,
            "download": session.download,
            "byExit": session.by_exit,
            "byRule": session.by_rule,
            "byPair": session.by_pair,
        });
        serde_json::to_writer(&mut writer, &line).map_err(|e| e.to_string())?;
        writer.write_all(b"\n").map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn load_session_logs(app: AppHandle) -> Result<Vec<SessionLogEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = LOG_WRITE_LOCK.lock().expect("log write lock");
        let file_path = data_dir(&app).join("logs").join("sessions.jsonl");
        let Ok(raw) = fs::read_to_string(&file_path) else {
            return Ok(Vec::new());
        };
        let mut sessions = Vec::new();
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<SessionLogEntry>(trimmed) {
                sessions.push(entry);
            }
        }
        sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(sessions)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogStats {
    pub file_count: usize,
    pub total_bytes: u64,
}

fn collect_log_stats(dir: &PathBuf) -> LogStats {
    let Ok(entries) = fs::read_dir(dir) else {
        return LogStats {
            file_count: 0,
            total_bytes: 0,
        };
    };
    let mut file_count = 0usize;
    let mut total_bytes = 0u64;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            file_count += 1;
            if let Ok(meta) = entry.metadata() {
                total_bytes += meta.len();
            }
        }
    }
    LogStats {
        file_count,
        total_bytes,
    }
}

#[tauri::command]
pub async fn get_log_stats(app: AppHandle) -> Result<LogStats, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = LOG_WRITE_LOCK.lock().expect("log write lock");
        let dir = data_dir(&app).join("logs");
        Ok(collect_log_stats(&dir))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn clear_logs(app: AppHandle) -> Result<LogStats, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = LOG_WRITE_LOCK.lock().expect("log write lock");
        let dir = data_dir(&app).join("logs");
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let _ = fs::remove_file(path);
                }
            }
        }
        Ok(collect_log_stats(&dir))
    })
    .await
    .map_err(|e| e.to_string())?
}
