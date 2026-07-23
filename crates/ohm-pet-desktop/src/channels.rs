use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelCommand {
    pub channel: String,
    pub conversation_id: String,
    pub sender_id: String,
    pub text: String,
    pub reply_to_message_id: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChannelConfig {
    pub proxy_url: String,
    pub telegram: TelegramConfig,
    pub lark: LarkConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TelegramConfig {
    pub enabled: bool,
    pub bot_token: String,
    pub chat_id: String,
    pub allowed_user_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LarkConfig {
    pub enabled: bool,
    pub app_id: String,
    pub app_secret: String,
    pub receive_id_type: String,
    pub receive_id: String,
    pub allowed_open_ids: Vec<String>,
}

impl Default for LarkConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            app_id: String::new(),
            app_secret: String::new(),
            receive_id_type: "open_id".into(),
            receive_id: String::new(),
            allowed_open_ids: Vec::new(),
        }
    }
}

impl ChannelConfig {
    pub fn http_agent(&self) -> Result<ureq::Agent, String> {
        if self.proxy_url.trim().is_empty() {
            return Ok(ureq::Agent::new_with_defaults());
        }
        let proxy = ureq::Proxy::new(self.proxy_url.trim())
            .map_err(|_| "The configured proxy URL is invalid".to_owned())?;
        Ok(ureq::Agent::config_builder()
            .proxy(Some(proxy))
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::NativeTls)
                    .build(),
            )
            .build()
            .into())
    }
}

impl TelegramConfig {
    pub fn ready(&self) -> bool {
        self.enabled && !self.bot_token.trim().is_empty() && !self.chat_id.trim().is_empty()
    }
}

impl LarkConfig {
    pub fn ready(&self) -> bool {
        self.enabled
            && !self.app_id.trim().is_empty()
            && !self.app_secret.trim().is_empty()
            && !self.receive_id.trim().is_empty()
            && matches!(
                self.receive_id_type.as_str(),
                "open_id" | "chat_id" | "user_id" | "email"
            )
    }
}

#[derive(Debug, Clone)]
pub struct ChannelConfigStore {
    path: PathBuf,
}

impl ChannelConfigStore {
    pub fn system() -> Option<Self> {
        ProjectDirs::from("works", "Earendil", "OHM Pet").map(|dirs| Self {
            path: dirs.config_dir().join("channels.json"),
        })
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn load(&self) -> ChannelConfig {
        self.load_checked().unwrap_or_default()
    }

    pub fn load_checked(&self) -> io::Result<ChannelConfig> {
        match fs::read_to_string(&self.path) {
            Ok(source) => serde_json::from_str(&source)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ChannelConfig::default()),
            Err(error) => Err(error),
        }
    }

    pub fn state_path(&self) -> PathBuf {
        self.path.with_file_name("channel-state.json")
    }

    pub fn save(&self, config: &ChannelConfig) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let source = serde_json::to_string_pretty(config).map_err(io::Error::other)?;
        write_private_file(&self.path, source.as_bytes())
    }

    pub fn save_runtime_state(&self, source: &[u8]) -> io::Result<()> {
        write_private_file(&self.state_path(), source)
    }
}

fn write_private_file(path: &std::path::Path, source: &[u8]) -> io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(source)?;
    file.sync_all()?;
    restrict_permissions(&temporary)?;
    #[cfg(target_os = "windows")]
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)
}

#[cfg(unix)]
fn restrict_permissions(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &std::path::Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_loads_channel_configuration() {
        let root = tempfile::tempdir().unwrap();
        let store = ChannelConfigStore::at(root.path().join("channels.json"));
        let mut config = ChannelConfig::default();
        config.telegram.enabled = true;
        config.telegram.bot_token = "secret".into();
        config.telegram.chat_id = "42".into();
        store.save(&config).unwrap();
        let loaded = store.load();
        assert!(loaded.telegram.ready());
        assert_eq!(loaded.telegram.bot_token, "secret");
    }

    #[test]
    fn refuses_malformed_configuration_without_overwriting_it() {
        let root = tempfile::tempdir().unwrap();
        let store = ChannelConfigStore::at(root.path().join("channels.json"));
        fs::write(store.path(), "{invalid").unwrap();
        assert_eq!(
            store.load_checked().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(fs::read_to_string(store.path()).unwrap(), "{invalid");
    }

    #[test]
    fn leaves_no_temporary_file_after_save() {
        let root = tempfile::tempdir().unwrap();
        let store = ChannelConfigStore::at(root.path().join("channels.json"));
        store.save(&ChannelConfig::default()).unwrap();
        assert!(!root.path().join("channels.tmp").exists());
    }

    #[cfg(unix)]
    #[test]
    fn stores_secrets_with_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let store = ChannelConfigStore::at(root.path().join("channels.json"));
        store.save(&ChannelConfig::default()).unwrap();
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
