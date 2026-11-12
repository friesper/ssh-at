// 主机中文别名：仅用于界面与菜单展示，不写入 ssh_config
// @author SIO
// @since 2026-11-12
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// 主机 ASCII 别名 -> 中文显示名
pub type HostAliases = HashMap<String, String>;

fn get_aliases_path() -> Result<PathBuf, String> {
    let home_dir = dirs::home_dir().ok_or("Failed to get home directory")?;
    let ssh_at_dir = home_dir.join(".ssh-at");

    if !ssh_at_dir.exists() {
        fs::create_dir_all(&ssh_at_dir).map_err(|e| e.to_string())?;
    }

    Ok(ssh_at_dir.join("aliases.json"))
}

/// 读取全部中文别名，供命令与托盘菜单复用
pub fn read_aliases() -> Result<HostAliases, String> {
    let path = get_aliases_path()?;

    if !path.exists() {
        return Ok(HostAliases::new());
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read aliases: {}", e))?;

    if content.trim().is_empty() {
        return Ok(HostAliases::new());
    }

    serde_json::from_str(&content).map_err(|e| format!("Failed to parse aliases: {}", e))
}

fn write_aliases(aliases: &HostAliases) -> Result<(), String> {
    let path = get_aliases_path()?;
    let content = serde_json::to_string_pretty(aliases)
        .map_err(|e| format!("Failed to serialize aliases: {}", e))?;

    fs::write(&path, content).map_err(|e| format!("Failed to write aliases: {}", e))
}

/// 读取全部主机中文别名
#[tauri::command]
pub async fn load_host_aliases() -> Result<HostAliases, String> {
    read_aliases()
}

/// 保存主机中文别名；传空字符串表示清除
#[tauri::command]
pub async fn save_host_alias(host: String, alias: String) -> Result<(), String> {
    let mut all = read_aliases()?;
    let trimmed = alias.trim();

    if trimmed.is_empty() {
        all.remove(&host);
    } else {
        all.insert(host, trimmed.to_string());
    }

    write_aliases(&all)
}

/// 主机被删除时同步清理其别名
pub fn remove_host_alias(host: &str) {
    let result = read_aliases().and_then(|mut all| {
        if all.remove(host).is_some() {
            write_aliases(&all)?;
        }
        Ok(())
    });

    if let Err(e) = result {
        eprintln!("[ALIASES] Failed to remove alias for {}: {}", host, e);
    }
}
