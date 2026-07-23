use crate::channels::{ChannelConfigStore, LarkConfig, TelegramConfig};
use anyhow::{anyhow, Context, Result};
use std::process::Command;

pub fn configure_proxy() -> Result<bool> {
    let store =
        ChannelConfigStore::system().ok_or_else(|| anyhow!("config directory unavailable"))?;
    let mut config = store
        .load_checked()
        .context("read channel settings; refusing to overwrite an invalid config file")?;
    let Some(proxy_url) = prompt(
        "Network Proxy",
        "HTTP proxy URL for Telegram/API requests; leave blank for direct connection",
        &config.proxy_url,
        false,
    )?
    else {
        return Ok(false);
    };
    let proxy_url = proxy_url.trim().to_owned();
    if !proxy_url.is_empty() {
        ureq::Proxy::new(&proxy_url)
            .map_err(|_| anyhow!("invalid proxy URL; use a value such as http://127.0.0.1:7890"))?;
    }
    config.proxy_url = proxy_url;
    store.save(&config).context("save network proxy settings")?;
    Ok(true)
}

pub fn configure_telegram() -> Result<bool> {
    let store =
        ChannelConfigStore::system().ok_or_else(|| anyhow!("config directory unavailable"))?;
    let mut config = store
        .load_checked()
        .context("read channel settings; refusing to overwrite an invalid config file")?;
    let Some(enabled) = confirm(
        "Telegram Bot",
        "Enable Telegram task notifications?",
        config.telegram.enabled,
    )?
    else {
        return Ok(false);
    };
    if !enabled {
        config.telegram.enabled = false;
        store.save(&config)?;
        return Ok(true);
    }
    let Some(bot_token) = prompt(
        "Telegram Bot Token",
        "Token from @BotFather (leave blank to keep the current token)",
        "",
        true,
    )?
    else {
        return Ok(false);
    };
    let Some(chat_id) = prompt(
        "Telegram Chat ID",
        "Chat ID that receives task notifications",
        &config.telegram.chat_id,
        false,
    )?
    else {
        return Ok(false);
    };
    let current_allowlist = config
        .telegram
        .allowed_user_ids
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let Some(allowlist) = prompt(
        "Telegram Allowed User IDs",
        "Comma-separated IDs allowed to send remote commands (required for inbound)",
        &current_allowlist,
        false,
    )?
    else {
        return Ok(false);
    };
    let bot_token = if bot_token.trim().is_empty() {
        config.telegram.bot_token.clone()
    } else {
        bot_token.trim().to_owned()
    };
    config.telegram = TelegramConfig {
        enabled: true,
        bot_token,
        chat_id: chat_id.trim().to_owned(),
        allowed_user_ids: parse_i64_list(&allowlist)?,
    };
    store
        .save(&config)
        .context("save Telegram channel settings")?;
    Ok(true)
}

pub fn configure_lark() -> Result<bool> {
    let store =
        ChannelConfigStore::system().ok_or_else(|| anyhow!("config directory unavailable"))?;
    let mut config = store
        .load_checked()
        .context("read channel settings; refusing to overwrite an invalid config file")?;
    let Some(enabled) = confirm(
        "Feishu / Lark Bot",
        "Enable Feishu/Lark task notifications?",
        config.lark.enabled,
    )?
    else {
        return Ok(false);
    };
    if !enabled {
        config.lark.enabled = false;
        store.save(&config)?;
        return Ok(true);
    }
    let Some(app_id) = prompt(
        "Feishu App ID",
        "Custom app App ID",
        &config.lark.app_id,
        false,
    )?
    else {
        return Ok(false);
    };
    let Some(app_secret) = prompt(
        "Feishu App Secret",
        "Custom app App Secret (leave blank to keep the current secret)",
        "",
        true,
    )?
    else {
        return Ok(false);
    };
    let Some(receive_id_type) = prompt(
        "Feishu Receive ID Type",
        "open_id, chat_id, user_id, or email",
        &config.lark.receive_id_type,
        false,
    )?
    else {
        return Ok(false);
    };
    let receive_id_type = receive_id_type.trim().to_owned();
    if !matches!(
        receive_id_type.as_str(),
        "open_id" | "chat_id" | "user_id" | "email"
    ) {
        return Err(anyhow!("unsupported Feishu receive ID type"));
    }
    let Some(receive_id) = prompt(
        "Feishu Receive ID",
        "User or chat that receives task notifications",
        &config.lark.receive_id,
        false,
    )?
    else {
        return Ok(false);
    };
    let current_allowlist = config.lark.allowed_open_ids.join(",");
    let Some(allowlist) = prompt(
        "Feishu Allowed Open IDs",
        "Comma-separated open_id values allowed to send remote commands (required for inbound)",
        &current_allowlist,
        false,
    )?
    else {
        return Ok(false);
    };
    let app_secret = if app_secret.trim().is_empty() {
        config.lark.app_secret.clone()
    } else {
        app_secret.trim().to_owned()
    };
    config.lark = LarkConfig {
        enabled: true,
        app_id: app_id.trim().to_owned(),
        app_secret,
        receive_id_type,
        receive_id: receive_id.trim().to_owned(),
        allowed_open_ids: parse_string_list(&allowlist),
    };
    store
        .save(&config)
        .context("save Feishu channel settings")?;
    Ok(true)
}

fn parse_i64_list(value: &str) -> Result<Vec<i64>> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse::<i64>()
                .with_context(|| format!("invalid Telegram user ID: {value}"))
        })
        .collect()
}

fn parse_string_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(target_os = "macos")]
fn prompt(title: &str, message: &str, default: &str, hidden: bool) -> Result<Option<String>> {
    let hidden_clause = if hidden { " with hidden answer" } else { "" };
    let script = format!(
        "on run argv\nset result to display dialog (item 2 of argv) with title (item 1 of argv) default answer (item 3 of argv){hidden_clause} buttons {{\"Cancel\", \"Save\"}} default button \"Save\" cancel button \"Cancel\"\nreturn text returned of result\nend run"
    );
    let output = Command::new("osascript")
        .args(["-e", &script, title, message, default])
        .output()
        .context("open macOS settings prompt")?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    ))
}

#[cfg(target_os = "macos")]
fn confirm(title: &str, message: &str, enabled: bool) -> Result<Option<bool>> {
    let default_button = if enabled { "Enable" } else { "Disable" };
    let script = format!(
        "on run argv\nset result to display dialog (item 2 of argv) with title (item 1 of argv) buttons {{\"Cancel\", \"Disable\", \"Enable\"}} default button \"{default_button}\" cancel button \"Cancel\"\nreturn button returned of result\nend run"
    );
    let output = Command::new("osascript")
        .args(["-e", &script, title, message])
        .output()
        .context("open macOS settings confirmation")?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim() == "Enable",
    ))
}

#[cfg(target_os = "windows")]
fn prompt(title: &str, message: &str, default: &str, _hidden: bool) -> Result<Option<String>> {
    let script = "Add-Type -AssemblyName Microsoft.VisualBasic; [Microsoft.VisualBasic.Interaction]::InputBox($args[1], $args[0], $args[2])";
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", script, title, message, default])
        .output()
        .context("open Windows settings prompt")?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    ))
}

#[cfg(target_os = "windows")]
fn confirm(title: &str, message: &str, enabled: bool) -> Result<Option<bool>> {
    let script = "Add-Type -AssemblyName PresentationFramework; $result=[System.Windows.MessageBox]::Show($args[1],$args[0],'YesNoCancel','Question'); $result.ToString()";
    let default = if enabled { "Yes" } else { "No" };
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", script, title, message, default])
        .output()
        .context("open Windows settings confirmation")?;
    if !output.status.success() {
        return Ok(None);
    }
    match String::from_utf8_lossy(&output.stdout).trim() {
        "Yes" => Ok(Some(true)),
        "No" => Ok(Some(false)),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_allowlists() {
        assert_eq!(parse_i64_list("1, 2,,3").unwrap(), vec![1, 2, 3]);
        assert_eq!(parse_string_list("ou_a, ou_b"), vec!["ou_a", "ou_b"]);
    }
}
