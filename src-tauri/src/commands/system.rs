use std::process::Command;

/// 将字符串转为 shell 单引号字面量，避免命令注入
#[cfg(target_os = "macos")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// 转义 AppleScript 双引号字符串中的反斜杠与双引号
#[cfg(target_os = "macos")]
fn applescript_escape(value: &str) -> String {
    value.replace('\\', r"\\").replace('"', r#"\""#)
}

/// iTerm2 是否已安装（交给 LaunchServices 查找，兼容非 /Applications 安装）
#[cfg(target_os = "macos")]
fn has_iterm() -> bool {
    Command::new("open")
        .args(["-Ra", "iTerm"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// 探测可用的 SSH agent socket。
/// GUI 进程从 launchd 继承的 SSH_AUTH_SOCK 常常指向无效 socket，
/// 因此候选值逐个用 ssh-add -l 验证，取第一个真正可用的
#[cfg(target_os = "macos")]
fn resolve_agent_socket() -> Option<String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(socket) = std::env::var("SSH_AUTH_SOCK") {
        if !socket.is_empty() {
            candidates.push(socket);
        }
    }

    // 回退到登录 shell（~/.zshrc 等里面的 agent 配置）
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    if let Ok(output) = Command::new(&shell)
        .args(["-lic", "printf %s \"$SSH_AUTH_SOCK\""])
        .output()
    {
        let socket = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !socket.is_empty() && !candidates.contains(&socket) {
            candidates.push(socket);
        }
    }

    candidates.into_iter().find(|socket| {
        Command::new("ssh-add")
            .arg("-l")
            .env("SSH_AUTH_SOCK", socket)
            .output()
            .map(|out| out.status.code() == Some(0))
            .unwrap_or(false)
    })
}

/// 生成「快速打开」脚本并返回路径。
/// iTerm2 的 command 参数不经过 shell（无法 export / 重定向），
/// 因此把环境变量与 ssh 调用落到脚本里，由 /bin/sh 执行
#[cfg(target_os = "macos")]
fn write_launch_script(host: &str, agent_socket: Option<&str>) -> Result<std::path::PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;

    let home = dirs::home_dir().ok_or("Failed to get home directory")?;
    let dir = home.join(".ssh-at").join("run");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create {}: {}", dir.display(), e))?;

    let file_name: String = host
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect();
    let path = dir.join(format!("open-{}.sh", file_name));

    let mut content = String::from("#!/bin/sh\n");
    if let Some(socket) = agent_socket {
        content.push_str(&format!("export SSH_AUTH_SOCK={}\n", shell_quote(socket)));
    }
    content.push_str(&format!("exec ssh {}\n", shell_quote(host)));

    std::fs::write(&path, content).map_err(|e| format!("Failed to write script: {}", e))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("Failed to chmod script: {}", e))?;

    Ok(path)
}

/// 在新窗口中打开到指定主机的 SSH 会话，优先 iTerm2，未安装则回退 Terminal.app
#[cfg(target_os = "macos")]
pub fn open_ssh_session(host: &str) -> Result<(), String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("Host name is empty".to_string());
    }

    let agent_socket = resolve_agent_socket();
    let script_path = write_launch_script(host, agent_socket.as_deref())?;
    let command = applescript_escape(&format!("/bin/sh {}", shell_quote(&script_path.to_string_lossy())));

    let script = if has_iterm() {
        format!(
            "tell application \"iTerm\"\n  activate\n  create window with default profile command \"{}\"\nend tell",
            command
        )
    } else {
        format!(
            "tell application \"Terminal\"\n  activate\n  do script \"{}\"\nend tell",
            command
        )
    };

    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("Failed to launch terminal: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to open session: {}", stderr.trim()));
    }

    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn open_ssh_session(_host: &str) -> Result<(), String> {
    Err("Quick open is only supported on macOS".to_string())
}

/// 主机列表 / 托盘菜单共用的快速打开入口
#[tauri::command]
pub async fn open_host_session(host: String) -> Result<(), String> {
    open_ssh_session(&host)
}

#[tauri::command]
pub async fn open_config_dir() -> Result<(), String> {
    let home_dir = dirs::home_dir().ok_or("Failed to get home directory")?;
    let ssh_at_dir = home_dir.join(".ssh-at");

    // 确保目录存在
    if !ssh_at_dir.exists() {
        std::fs::create_dir_all(&ssh_at_dir).map_err(|e| e.to_string())?;
    }

    let path_str = ssh_at_dir.to_str().ok_or("Invalid path")?;

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(path_str)
            .spawn()
            .map_err(|e| format!("Failed to open directory: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(path_str)
            .spawn()
            .map_err(|e| format!("Failed to open directory: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(path_str)
            .spawn()
            .map_err(|e| format!("Failed to open directory: {}", e))?;
    }

    Ok(())
}
