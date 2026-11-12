use ssh_at_core::config::{SshConfig, HostEntry};
use std::net::IpAddr;
use serde::Serialize;

// 定义结构化的验证错误，前端根据 code 查翻译
#[derive(Serialize)]
#[serde(tag = "code", content = "params")]
enum ValidationError {
    HostRequired,
    HostAliasInvalidChar { ch: String },
    HostNameRequired,
    HostNameEmpty,
    HostNameInvalidDomain { value: String },
    HostNameTooLong { length: usize },
    HostNameInvalidIp { value: String },
    HostNameConsecutiveDots,
    HostNameLabelTooLong { label: String },
    HostNameLabelInvalidHyphen { label: String },
    HostNameInvalidChar { ch: String, label: String },
    PortInvalid,
    UserRequired,
    IdentityFileExpandFailed,
    IdentityFileNotExist { path: String },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 序列化为 JSON，前端解析后查翻译
        write!(f, "{}", serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string()))
    }
}

impl From<ValidationError> for String {
    fn from(err: ValidationError) -> String {
        serde_json::to_string(&err).unwrap_or_else(|_| "{}".to_string())
    }
}

// Validation helper: Host 别名必须为 ASCII
// OpenSSH 在 ssh.c 中校验命令行 host 参数，非 ASCII 字符会直接
// fatal("hostname contains invalid characters")，且在匹配 config 之前执行，
// 因此配置里写中文 Host 别名永远无法使用；中文只能作为 HostName 的值
fn validate_host_alias(alias: &str) -> Result<(), ValidationError> {
    if let Some(ch) = alias
        .chars()
        .find(|c| !c.is_ascii() || c.is_ascii_whitespace() || c.is_ascii_control())
    {
        return Err(ValidationError::HostAliasInvalidChar { ch: ch.to_string() });
    }

    Ok(())
}

// Validation helper: check if hostname is valid IP or domain
fn validate_hostname(hostname: &str) -> Result<(), ValidationError> {
    let trimmed = hostname.trim();

    if trimmed.is_empty() {
        return Err(ValidationError::HostNameEmpty);
    }

    // Try parse as IP address with proper validation
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        match ip {
            IpAddr::V4(ipv4) => {
                if ipv4.is_unspecified() {
                    return Err(ValidationError::HostNameInvalidIp { value: trimmed.to_string() });
                }
            }
            IpAddr::V6(ipv6) => {
                if ipv6.is_unspecified() {
                    return Err(ValidationError::HostNameInvalidIp { value: trimmed.to_string() });
                }
            }
        }
        return Ok(());
    }

    // Validate as domain name (RFC 1035 rules)
    if !trimmed.contains('.') {
        return Err(ValidationError::HostNameInvalidDomain { value: trimmed.to_string() });
    }

    if trimmed.len() > 253 {
        return Err(ValidationError::HostNameTooLong { length: trimmed.len() });
    }

    // Validate each label (segment between dots)
    for label in trimmed.split('.') {
        if label.is_empty() {
            return Err(ValidationError::HostNameConsecutiveDots);
        }
        if label.len() > 63 {
            return Err(ValidationError::HostNameLabelTooLong { label: label.to_string() });
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(ValidationError::HostNameLabelInvalidHyphen { label: label.to_string() });
        }
        for ch in label.chars() {
            if !ch.is_alphanumeric() && ch != '-' {
                return Err(ValidationError::HostNameInvalidChar {
                    ch: ch.to_string(),
                    label: label.to_string()
                });
            }
        }
    }

    Ok(())
}

// Validation helper: check if port is in valid range
fn validate_port(port: u16) -> Result<(), ValidationError> {
    if port == 0 {
        return Err(ValidationError::PortInvalid);
    }
    Ok(())
}

// Validation helper: check if identity file exists (OPTIONAL field)
// 未指定 IdentityFile 时交由 ssh-agent($SSH_AUTH_SOCK) 或 ~/.ssh 下的默认密钥认证
fn validate_identity_file(identity_file: &Option<std::path::PathBuf>) -> Result<(), ValidationError> {
    let Some(identity_file) = identity_file.as_ref() else {
        return Ok(());
    };

    let path_str = identity_file.to_string_lossy();
    let trimmed = path_str.trim();

    if trimmed.is_empty() {
        return Ok(());
    }

    let expanded_path = if let Some(stripped) = trimmed.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            home.join(stripped)
        } else {
            return Err(ValidationError::IdentityFileExpandFailed);
        }
    } else {
        identity_file.clone()
    };

    if !expanded_path.exists() {
        return Err(ValidationError::IdentityFileNotExist { path: trimmed.to_string() });
    }

    Ok(())
}

// 用户常把 $SSH_AUTH_SOCK 直接填进 IdentityAgent，但 ssh_config 不展开环境变量，
// 这里归一成 OpenSSH 认识的关键字 SSH_AUTH_SOCK
fn normalize_identity_agent(entry: &mut HostEntry) {
    if let Some(agent) = entry.identity_agent.as_mut() {
        let normalized = agent
            .trim()
            .trim_start_matches('$')
            .trim_start_matches('{')
            .trim_end_matches('}');

        if normalized.eq_ignore_ascii_case("SSH_AUTH_SOCK") {
            *agent = "SSH_AUTH_SOCK".to_string();
        }
    }
}

// Unified validation for HostEntry
fn validate_entry(entry: &HostEntry) -> Result<(), ValidationError> {
    // Validate host name
    if entry.host.trim().is_empty() {
        return Err(ValidationError::HostRequired);
    }
    validate_host_alias(&entry.host)?;

    // Validate hostname (IP or domain)
    if let Some(ref hostname) = entry.hostname {
        validate_hostname(hostname)?;
    } else {
        return Err(ValidationError::HostNameRequired);
    }

    // Validate user
    if entry.user.as_ref().is_none_or(|u| u.trim().is_empty()) {
        return Err(ValidationError::UserRequired);
    }

    // Validate port if provided
    if let Some(port) = entry.port {
        validate_port(port)?;
    }

    // Validate identity file if provided (optional: ssh-agent / default keys)
    validate_identity_file(&entry.identity_file)?;

    Ok(())
}

#[tauri::command]
pub async fn test_simple() -> Result<String, String> {
    Ok("Hello".to_string())
}

#[tauri::command]
pub async fn load_ssh_config(app: tauri::AppHandle) -> Result<SshConfig, String> {
    eprintln!("[COMMAND] load_ssh_config called");

    // Use spawn_blocking to execute in a dedicated blocking thread pool
    let config = tokio::task::spawn_blocking(|| {
        eprintln!("[COMMAND] In blocking thread pool");

        let handle = std::thread::Builder::new()
            .name("load-config-sync".to_string())
            .stack_size(64 * 1024 * 1024) // 64MB stack
            .spawn(|| {
                eprintln!("[COMMAND] In 64MB stack thread");

                let home = dirs::home_dir()
                    .ok_or("Failed to get home directory")?;
                let config_path = home.join(".ssh/config");

                if !config_path.exists() {
                    return Ok(SshConfig {
                        hosts: vec![],
                        global_options: std::collections::HashMap::new(),
                    });
                }

                let content = std::fs::read_to_string(&config_path)
                    .map_err(|e| format!("Failed to read config: {}", e))?;

                eprintln!("[COMMAND] Parsing {} bytes", content.len());

                ssh_at_core::config::simple_parser::parse_ssh_config(&content)
                    .map_err(|e| format!("Parse error: {}", e))
            })
            .map_err(|e| format!("Failed to spawn thread: {}", e))?;

        handle.join()
            .map_err(|e| format!("Thread panicked: {:?}", e))?
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    eprintln!("[COMMAND] Successfully loaded {} hosts", config.hosts.len());

    // 托盘「快速打开」列表跟随最新配置刷新；通配符别名无法直接 ssh，跳过
    // 有中文别名时显示「中文名 (ascii)」，菜单项 id 始终用 ASCII 别名
    let aliases = crate::commands::aliases::read_aliases().unwrap_or_default();
    let tray_hosts: Vec<(String, String)> = config
        .hosts
        .iter()
        .filter(|host| !host.host.contains('*') && !host.host.contains('?'))
        .map(|host| {
            let label = aliases
                .get(&host.host)
                .map(|alias| format!("{} ({})", alias, host.host))
                .unwrap_or_else(|| host.host.clone());
            (host.host.clone(), label)
        })
        .collect();
    crate::tray::refresh_hosts(&app, tray_hosts);

    Ok(config)
}

#[tauri::command]
pub async fn save_ssh_config(config: SshConfig) -> Result<(), String> {
    ssh_at_core::config::save_config(config)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_host(mut entry: HostEntry) -> Result<(), String> {
    normalize_identity_agent(&mut entry);
    validate_entry(&entry)?;

    ssh_at_core::config::add_host(entry)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_host(name: String, mut entry: HostEntry) -> Result<(), String> {
    normalize_identity_agent(&mut entry);
    validate_entry(&entry)?;

    ssh_at_core::config::update_host(&name, entry)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_host(name: String) -> Result<(), String> {
    let result = ssh_at_core::config::delete_host(&name)
        .await
        .map_err(|e| e.to_string());

    if result.is_ok() {
        crate::commands::tags::remove_host_tags(&name);
        crate::commands::aliases::remove_host_alias(&name);
    }

    result
}

#[tauri::command]
pub async fn search_hosts(query: String) -> Result<Vec<HostEntry>, String> {
    ssh_at_core::config::search_hosts(&query)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn serialize_ssh_config(config: SshConfig) -> Result<String, String> {
    ssh_at_core::config::serialize_ssh_config(&config)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn parse_ssh_config(content: String) -> Result<SshConfig, String> {
    ssh_at_core::config::simple_parser::parse_ssh_config(&content)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> HostEntry {
        let mut entry = HostEntry::new("example".to_string());
        entry.hostname = Some("example.com".to_string());
        entry.user = Some("root".to_string());
        entry
    }

    // 未配置 IdentityFile 时应校验通过（交由 ssh-agent / 默认密钥认证）
    #[test]
    fn validate_entry_allows_missing_identity_file() {
        let entry = sample_entry();
        assert!(validate_entry(&entry).is_ok());
    }

    // 中文 Host 别名必须被拒绝：ssh 命令行不支持非 ASCII
    #[test]
    fn validate_entry_rejects_non_ascii_alias() {
        let mut entry = sample_entry();
        entry.host = "黑龙江加墨".to_string();
        assert!(validate_entry(&entry).is_err());
    }

    // $SSH_AUTH_SOCK / ${SSH_AUTH_SOCK} 归一成 ssh_config 关键字
    #[test]
    fn normalize_identity_agent_strips_shell_syntax() {
        for raw in ["$SSH_AUTH_SOCK", "${SSH_AUTH_SOCK}", " ssh_auth_sock "] {
            let mut entry = sample_entry();
            entry.identity_agent = Some(raw.to_string());
            normalize_identity_agent(&mut entry);
            assert_eq!(entry.identity_agent.as_deref(), Some("SSH_AUTH_SOCK"));
        }
    }

    // 具体 socket 路径应保持原样
    #[test]
    fn normalize_identity_agent_keeps_path() {
        let mut entry = sample_entry();
        entry.identity_agent = Some("~/.1password/agent.sock".to_string());
        normalize_identity_agent(&mut entry);
        assert_eq!(entry.identity_agent.as_deref(), Some("~/.1password/agent.sock"));
    }

    // 通配符别名属于合法 ssh 配置写法，不能被误拦
    #[test]
    fn validate_entry_allows_wildcard_alias() {
        let mut entry = sample_entry();
        entry.host = "*.corp.example.com".to_string();
        assert!(validate_entry(&entry).is_ok());
    }

    // 显式指定已存在的密钥文件仍通过
    #[test]
    fn validate_entry_allows_existing_identity_file() {
        let mut entry = sample_entry();
        let path = std::env::temp_dir().join("ssh-at-id-test");
        std::fs::write(&path, b"dummy").unwrap();
        entry.identity_file = Some(path.clone());
        assert!(validate_entry(&entry).is_ok());
        let _ = std::fs::remove_file(path);
    }

    // 指定不存在的密钥文件仍需报错
    #[test]
    fn validate_entry_rejects_unknown_identity_file() {
        let mut entry = sample_entry();
        entry.identity_file = Some(std::path::PathBuf::from("/nonexistent/id_ed25519"));
        assert!(validate_entry(&entry).is_err());
    }
}
