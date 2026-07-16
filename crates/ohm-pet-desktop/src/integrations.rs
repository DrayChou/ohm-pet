use anyhow::{anyhow, Context, Result};
use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use toml_edit::{Array, DocumentMut, Item, Value as TomlValue};

use crate::agent_ipc::AGENT_SIGNAL_ADDRESS;

const INTEGRATION_ID: &str = "ohm-pet";
const PI_MARKER: &str = "// OHM_PET_INTEGRATION_V1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Codex,
    Pi,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IntegrationStatus {
    pub claude: bool,
    pub codex: bool,
    pub pi: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct IntegrationState {
    codex_previous_notify: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct IntegrationPaths {
    claude_settings: PathBuf,
    codex_config: PathBuf,
    pi_extension: PathBuf,
    state: PathBuf,
}

pub struct IntegrationManager {
    executable: PathBuf,
    paths: IntegrationPaths,
}

impl IntegrationManager {
    pub fn system() -> Result<Self> {
        let base_dirs = BaseDirs::new().ok_or_else(|| anyhow!("home directory unavailable"))?;
        let home = base_dirs.home_dir();
        let codex_home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        let claude_home = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".claude"));
        let state = ProjectDirs::from("works", "Earendil", "OHM Pet")
            .ok_or_else(|| anyhow!("OHM Pet configuration directory unavailable"))?
            .config_dir()
            .join("integrations.json");
        Ok(Self {
            executable: std::env::current_exe().context("locate OHM Pet executable")?,
            paths: IntegrationPaths {
                claude_settings: claude_home.join("settings.json"),
                codex_config: codex_home.join("config.toml"),
                pi_extension: home.join(".pi/agent/extensions/ohm-pet.ts"),
                state,
            },
        })
    }

    #[cfg(test)]
    fn at(root: &Path) -> Self {
        Self {
            executable: root.join("OHM Pet.exe"),
            paths: IntegrationPaths {
                claude_settings: root.join(".claude/settings.json"),
                codex_config: root.join(".codex/config.toml"),
                pi_extension: root.join(".pi/agent/extensions/ohm-pet.ts"),
                state: root.join("ohm-pet/integrations.json"),
            },
        }
    }

    pub fn status(&self) -> IntegrationStatus {
        IntegrationStatus {
            claude: self.claude_installed(),
            codex: self.codex_installed(),
            pi: self.pi_installed(),
        }
    }

    pub fn install(&self, kind: AgentKind) -> Result<()> {
        match kind {
            AgentKind::Claude => self.install_claude(),
            AgentKind::Codex => self.install_codex(),
            AgentKind::Pi => self.install_pi(),
        }
    }

    pub fn remove(&self, kind: AgentKind) -> Result<()> {
        match kind {
            AgentKind::Claude => self.remove_claude(),
            AgentKind::Codex => self.remove_codex(),
            AgentKind::Pi => self.remove_pi(),
        }
    }

    pub fn forward_previous_codex_notify(&self, payload: &str) -> Result<()> {
        let state = self.load_state();
        let Some((program, arguments)) = state
            .codex_previous_notify
            .as_deref()
            .and_then(|command| command.split_first())
        else {
            return Ok(());
        };
        Command::new(program)
            .args(arguments)
            .arg(payload)
            .spawn()
            .context("forward previous Codex notify command")?;
        Ok(())
    }

    fn install_claude(&self) -> Result<()> {
        let mut settings = read_json_object(&self.paths.claude_settings)?;
        remove_ohm_claude_hooks(&mut settings);
        let hooks = settings
            .entry("hooks")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| anyhow!("Claude settings 'hooks' must be an object"))?;
        for (event, matcher, state) in [
            ("SessionStart", None, "idle"),
            ("UserPromptSubmit", None, "working"),
            ("PreToolUse", None, "working"),
            (
                "Notification",
                Some("permission_prompt|idle_prompt|agent_needs_input"),
                "waiting",
            ),
            ("Stop", None, "completed"),
            ("StopFailure", None, "failed"),
            ("SessionEnd", None, "idle"),
        ] {
            let groups = hooks
                .entry(event)
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .ok_or_else(|| anyhow!("Claude hook event '{event}' must be an array"))?;
            let command = json!({
                "type": "command",
                "command": self.executable,
                "args": [
                    "signal", "--source", "claude", "--event", state,
                    "--integration", INTEGRATION_ID
                ],
                "async": true,
                "timeout": 5
            });
            let mut group = Map::new();
            if let Some(matcher) = matcher {
                group.insert("matcher".into(), Value::String(matcher.into()));
            }
            group.insert("hooks".into(), Value::Array(vec![command]));
            groups.push(Value::Object(group));
        }
        write_json_object(&self.paths.claude_settings, &settings)
    }

    fn remove_claude(&self) -> Result<()> {
        if !self.paths.claude_settings.exists() {
            return Ok(());
        }
        let mut settings = read_json_object(&self.paths.claude_settings)?;
        remove_ohm_claude_hooks(&mut settings);
        write_json_object(&self.paths.claude_settings, &settings)
    }

    fn claude_installed(&self) -> bool {
        read_json_object(&self.paths.claude_settings)
            .ok()
            .is_some_and(|settings| contains_ohm_claude_hook(&settings))
    }

    fn install_codex(&self) -> Result<()> {
        let mut document = read_toml(&self.paths.codex_config)?;
        let current = toml_string_array(document.get("notify"));
        let mut state = self.load_state();
        if current.as_ref().is_some_and(|value| !is_ohm_command(value)) {
            state.codex_previous_notify = current;
            self.save_state(&state)?;
        }
        document["notify"] = Item::Value(TomlValue::Array(string_array(&[
            self.executable.to_string_lossy().as_ref(),
            "signal",
            "--source",
            "codex",
            "--event",
            "completed",
            "--integration",
            INTEGRATION_ID,
        ])));
        write_toml(&self.paths.codex_config, &document)
    }

    fn remove_codex(&self) -> Result<()> {
        if !self.paths.codex_config.exists() {
            return Ok(());
        }
        let mut document = read_toml(&self.paths.codex_config)?;
        if toml_string_array(document.get("notify"))
            .as_ref()
            .is_some_and(|value| is_ohm_command(value))
        {
            let mut state = self.load_state();
            if let Some(previous) = state.codex_previous_notify.take() {
                document["notify"] = Item::Value(TomlValue::Array(string_array_owned(&previous)));
            } else {
                document.remove("notify");
            }
            self.save_state(&state)?;
            write_toml(&self.paths.codex_config, &document)?;
        }
        Ok(())
    }

    fn codex_installed(&self) -> bool {
        read_toml(&self.paths.codex_config)
            .ok()
            .and_then(|document| toml_string_array(document.get("notify")))
            .is_some_and(|value| is_ohm_command(&value))
    }

    fn install_pi(&self) -> Result<()> {
        write_file(&self.paths.pi_extension, &pi_extension_source())
    }

    fn remove_pi(&self) -> Result<()> {
        if self.pi_installed() {
            fs::remove_file(&self.paths.pi_extension).context("remove Pi integration")?;
        }
        Ok(())
    }

    fn pi_installed(&self) -> bool {
        fs::read_to_string(&self.paths.pi_extension)
            .ok()
            .is_some_and(|source| source.contains(PI_MARKER))
    }

    fn load_state(&self) -> IntegrationState {
        fs::read_to_string(&self.paths.state)
            .ok()
            .and_then(|source| serde_json::from_str(&source).ok())
            .unwrap_or_default()
    }

    fn save_state(&self, state: &IntegrationState) -> Result<()> {
        let source = serde_json::to_string_pretty(state).context("serialize integration state")?;
        write_file(&self.paths.state, &(source + "\n"))
    }
}

fn is_ohm_command(command: &[String]) -> bool {
    command
        .windows(2)
        .any(|pair| pair == ["--integration", INTEGRATION_ID])
}

fn is_ohm_hook(value: &Value) -> bool {
    value
        .get("args")
        .and_then(Value::as_array)
        .is_some_and(|arguments| {
            arguments.windows(2).any(|pair| {
                pair[0].as_str() == Some("--integration")
                    && pair[1].as_str() == Some(INTEGRATION_ID)
            })
        })
}

fn contains_ohm_claude_hook(settings: &Map<String, Value>) -> bool {
    settings
        .get("hooks")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(Map::values)
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|group| group.get("hooks").and_then(Value::as_array))
        .flatten()
        .any(is_ohm_hook)
}

fn remove_ohm_claude_hooks(settings: &mut Map<String, Value>) {
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else {
        return;
    };
    hooks.retain(|_, groups| {
        let Some(groups) = groups.as_array_mut() else {
            return true;
        };
        groups.retain_mut(|group| {
            let Some(commands) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            commands.retain(|command| !is_ohm_hook(command));
            !commands.is_empty()
        });
        !groups.is_empty()
    });
    if hooks.is_empty() {
        settings.remove("hooks");
    }
}

fn read_json_object(path: &Path) -> Result<Map<String, Value>> {
    if !path.exists() {
        return Ok(Map::new());
    }
    serde_json::from_str::<Value>(&fs::read_to_string(path).context("read JSON settings")?)
        .context("parse JSON settings")?
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("settings root must be a JSON object"))
}

fn write_json_object(path: &Path, object: &Map<String, Value>) -> Result<()> {
    let source = serde_json::to_string_pretty(object).context("serialize JSON settings")?;
    write_file(path, &(source + "\n"))
}

fn read_toml(path: &Path) -> Result<DocumentMut> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }
    fs::read_to_string(path)
        .context("read Codex config")?
        .parse::<DocumentMut>()
        .context("parse Codex config")
}

fn write_toml(path: &Path, document: &DocumentMut) -> Result<()> {
    write_file(path, &document.to_string())
}

fn toml_string_array(item: Option<&Item>) -> Option<Vec<String>> {
    item?.as_array().map(|array| {
        array
            .iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect()
    })
}

fn string_array(values: &[&str]) -> Array {
    let mut array = Array::new();
    for value in values {
        array.push(*value);
    }
    array
}

fn string_array_owned(values: &[String]) -> Array {
    let mut array = Array::new();
    for value in values {
        array.push(value.as_str());
    }
    array
}

fn write_file(path: &Path, source: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, source).with_context(|| format!("write {}", path.display()))
}

fn pi_extension_source() -> String {
    let port = AGENT_SIGNAL_ADDRESS
        .rsplit_once(':')
        .map_or("47832", |(_, port)| port);
    format!(
        r#"{PI_MARKER}
import dgram from "node:dgram";
import type {{ ExtensionAPI }} from "@earendil-works/pi-coding-agent";

const ADDRESS = "127.0.0.1";
const PORT = {port};

function signal(event: "working" | "waiting" | "completed" | "failed" | "idle") {{
  const socket = dgram.createSocket("udp4");
  const payload = Buffer.from(JSON.stringify({{ source: "pi", event }}));
  socket.send(payload, PORT, ADDRESS, () => socket.close());
}}

export default function (pi: ExtensionAPI) {{
  pi.on("agent_start", () => signal("working"));
  pi.on("tool_execution_start", () => signal("working"));
  pi.on("tool_execution_end", (event) => {{
    if (event.isError) signal("failed");
  }});
  pi.on("agent_settled", () => signal("completed"));
  pi.on("session_shutdown", () => signal("idle"));
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ohm-pet-{name}-{unique}"))
    }

    #[test]
    fn claude_install_is_idempotent_and_preserves_existing_hooks() {
        let root = test_root("claude");
        let manager = IntegrationManager::at(&root);
        write_file(
            &manager.paths.claude_settings,
            r#"{"theme":"dark","hooks":{"Stop":[{"hooks":[{"type":"command","command":"existing"}]}]}}"#,
        )
        .unwrap();
        manager.install(AgentKind::Claude).unwrap();
        manager.install(AgentKind::Claude).unwrap();
        let settings = read_json_object(&manager.paths.claude_settings).unwrap();
        assert_eq!(settings.get("theme").and_then(Value::as_str), Some("dark"));
        assert!(contains_ohm_claude_hook(&settings));
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(encoded.matches("\"--integration\",\"ohm-pet\"").count(), 7);
        manager.remove(AgentKind::Claude).unwrap();
        let settings = read_json_object(&manager.paths.claude_settings).unwrap();
        assert!(!contains_ohm_claude_hook(&settings));
        assert!(serde_json::to_string(&settings)
            .unwrap()
            .contains("existing"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn codex_install_restores_previous_notify_command() {
        let root = test_root("codex");
        let manager = IntegrationManager::at(&root);
        write_file(
            &manager.paths.codex_config,
            "model = \"gpt-5\"\nnotify = [\"old-notify\", \"--flag\"]\n",
        )
        .unwrap();
        manager.install(AgentKind::Codex).unwrap();
        assert!(manager.codex_installed());
        manager.install(AgentKind::Codex).unwrap();
        manager.remove(AgentKind::Codex).unwrap();
        let document = read_toml(&manager.paths.codex_config).unwrap();
        assert_eq!(
            toml_string_array(document.get("notify")).unwrap(),
            vec!["old-notify", "--flag"]
        );
        assert_eq!(document["model"].as_str(), Some("gpt-5"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pi_install_only_removes_owned_extension() {
        let root = test_root("pi");
        let manager = IntegrationManager::at(&root);
        manager.install(AgentKind::Pi).unwrap();
        assert!(manager.pi_installed());
        let source = fs::read_to_string(&manager.paths.pi_extension).unwrap();
        assert!(source.contains("agent_settled"));
        assert!(source.contains("tool_execution_end"));
        manager.remove(AgentKind::Pi).unwrap();
        assert!(!manager.paths.pi_extension.exists());
        let _ = fs::remove_dir_all(root);
    }
}
