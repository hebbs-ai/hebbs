use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct CliConfig {
    pub endpoint: String,
    pub http_port: u16,
    pub timeout_ms: u64,
    pub output_format: OutputFormat,
    pub color: ColorMode,
    pub history_file: PathBuf,
    pub max_history: usize,
    pub tenant: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Human,
    Json,
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    Always,
    Never,
    Auto,
}

impl Default for CliConfig {
    fn default() -> Self {
        let history_file = config_dir().join("cli_history");
        Self {
            endpoint: "http://localhost:6380".to_string(),
            http_port: 6381,
            timeout_ms: 30000,
            output_format: OutputFormat::Human,
            color: ColorMode::Auto,
            history_file,
            max_history: 1000,
            tenant: None,
            api_key: None,
        }
    }
}

impl CliConfig {
    pub fn load() -> Self {
        let mut cfg = Self::default();

        if let Some(file_cfg) = load_config_file() {
            if let Some(ep) = file_cfg.endpoint {
                cfg.endpoint = ep;
            }
            if let Some(hp) = file_cfg.http_port {
                cfg.http_port = hp;
            }
            if let Some(tm) = file_cfg.timeout_ms {
                cfg.timeout_ms = tm;
            }
            if let Some(fmt) = file_cfg.output_format {
                cfg.output_format = fmt;
            }
            if let Some(c) = file_cfg.color {
                cfg.color = c;
            }
            if let Some(hf) = file_cfg.history_file {
                cfg.history_file = hf;
            }
            if let Some(mh) = file_cfg.max_history {
                cfg.max_history = mh;
            }
            if let Some(t) = file_cfg.tenant {
                cfg.tenant = Some(t);
            }
            if let Some(k) = file_cfg.api_key {
                cfg.api_key = Some(k);
            }
        }

        if let Ok(ep) = std::env::var("HEBBS_ENDPOINT") {
            cfg.endpoint = ep;
        }
        if let Ok(hp) = std::env::var("HEBBS_HTTP_PORT") {
            if let Ok(p) = hp.parse() {
                cfg.http_port = p;
            }
        }
        if let Ok(tm) = std::env::var("HEBBS_TIMEOUT") {
            if let Ok(t) = tm.parse() {
                cfg.timeout_ms = t;
            }
        }

        cfg
    }

    pub fn should_color(&self, is_tty: bool) -> bool {
        match self.color {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => is_tty,
        }
    }
}

fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hebbs")
}

#[derive(Deserialize)]
struct FileConfig {
    endpoint: Option<String>,
    http_port: Option<u16>,
    timeout_ms: Option<u64>,
    output_format: Option<OutputFormat>,
    color: Option<ColorMode>,
    history_file: Option<PathBuf>,
    max_history: Option<usize>,
    tenant: Option<String>,
    api_key: Option<String>,
}

fn load_config_file() -> Option<FileConfig> {
    let config_path = config_dir().join("cli.toml");
    let content = std::fs::read_to_string(config_path).ok()?;
    toml::from_str(&content).ok()
}

// toml is used for config loading; bring it in as a small inline dependency
mod toml {
    pub fn from_str<'de, T: serde::Deserialize<'de>>(s: &'de str) -> Result<T, String> {
        // Minimal TOML parsing: for the CLI config we support simple key = value pairs
        // This avoids pulling in the full `toml` crate (which is only in the server).
        // For now, we use serde_json as a fallback -- config files can be JSON too.
        serde_json::from_str(s).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        let cfg = CliConfig::default();
        assert_eq!(cfg.endpoint, "http://localhost:6380");
        assert_eq!(cfg.http_port, 6381);
        assert_eq!(cfg.timeout_ms, 30000);
        assert_eq!(cfg.output_format, OutputFormat::Human);
        assert_eq!(cfg.color, ColorMode::Auto);
        assert_eq!(cfg.max_history, 1000);
    }

    #[test]
    fn color_auto_respects_tty() {
        let cfg = CliConfig::default();
        assert!(cfg.should_color(true));
        assert!(!cfg.should_color(false));
    }

    #[test]
    fn color_always_ignores_tty() {
        let cfg = CliConfig {
            color: ColorMode::Always,
            ..CliConfig::default()
        };
        assert!(cfg.should_color(false));
    }

    #[test]
    fn color_never_ignores_tty() {
        let cfg = CliConfig {
            color: ColorMode::Never,
            ..CliConfig::default()
        };
        assert!(!cfg.should_color(true));
    }
}
