// 主机标签存储：ssh_config 不允许自定义关键字，标签单独存放于 ~/.ssh-at/tags.json
// @author SIO
// @since 2026-11-12
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// 主机别名 -> 标签列表
pub type HostTags = HashMap<String, Vec<String>>;

fn get_tags_path() -> Result<PathBuf, String> {
    let home_dir = dirs::home_dir().ok_or("Failed to get home directory")?;
    let ssh_at_dir = home_dir.join(".ssh-at");

    if !ssh_at_dir.exists() {
        fs::create_dir_all(&ssh_at_dir).map_err(|e| e.to_string())?;
    }

    Ok(ssh_at_dir.join("tags.json"))
}

fn read_tags() -> Result<HostTags, String> {
    let path = get_tags_path()?;

    if !path.exists() {
        return Ok(HostTags::new());
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read tags: {}", e))?;

    if content.trim().is_empty() {
        return Ok(HostTags::new());
    }

    serde_json::from_str(&content).map_err(|e| format!("Failed to parse tags: {}", e))
}

fn write_tags(tags: &HostTags) -> Result<(), String> {
    let path = get_tags_path()?;
    let content = serde_json::to_string_pretty(tags)
        .map_err(|e| format!("Failed to serialize tags: {}", e))?;

    fs::write(&path, content).map_err(|e| format!("Failed to write tags: {}", e))
}

/// 去空格、丢弃空标签并按出现顺序去重
fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();

    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() || result.iter().any(|item| item == trimmed) {
            continue;
        }
        result.push(trimmed.to_string());
    }

    result
}

/// 读取全部主机标签
#[tauri::command]
pub async fn load_host_tags() -> Result<HostTags, String> {
    read_tags()
}

/// 覆盖某个主机的标签；传空数组表示清除
#[tauri::command]
pub async fn save_host_tags(host: String, tags: Vec<String>) -> Result<(), String> {
    let mut all = read_tags()?;
    let cleaned = normalize_tags(tags);

    if cleaned.is_empty() {
        all.remove(&host);
    } else {
        all.insert(host, cleaned);
    }

    write_tags(&all)
}

/// 主机被删除时同步清理其标签，避免遗留脏数据
pub fn remove_host_tags(host: &str) {
    let result = read_tags().and_then(|mut all| {
        if all.remove(host).is_some() {
            write_tags(&all)?;
        }
        Ok(())
    });

    if let Err(e) = result {
        eprintln!("[TAGS] Failed to remove tags for {}: {}", host, e);
    }
}
