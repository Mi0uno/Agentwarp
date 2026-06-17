use std::cell::RefCell;
use std::collections::HashMap;
#[cfg(not(target_family = "wasm"))]
use std::ffi::OsString;
#[cfg(not(target_family = "wasm"))]
use std::fs;
#[cfg(not(target_family = "wasm"))]
use std::io::{Read as _, Write as _};
#[cfg(not(target_family = "wasm"))]
use std::net::{TcpStream, ToSocketAddrs};
#[cfg(not(target_family = "wasm"))]
use std::path::{Path, PathBuf};
#[cfg(not(target_family = "wasm"))]
use std::time::Duration;

use pathfinder_color::ColorU;
use pathfinder_geometry::vector::{vec2f, Vector2F};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warp_core::ui::theme::Fill as ThemeFill;
use warp_core::user_preferences::GetUserPreferences as _;
#[cfg(not(target_family = "wasm"))]
use warp_terminal::shell::ShellType;
use warpui::elements::{
    Align, Border, ChildAnchor, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox,
    Container, CornerRadius, CrossAxisAlignment, DispatchEventResult, DropShadow, Element, Empty,
    EventHandler, Fill as ElementFill, Flex, Hoverable, MainAxisAlignment, MainAxisSize,
    MouseStateHandle, OffsetPositioning, ParentAnchor, ParentElement, ParentOffsetBounds, Radius,
    ScrollbarWidth, Shrinkable, Stack, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::text_layout::ClipConfig;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::ui_components::text_input::TextInput;
use warpui::{
    AppContext, Entity, EntityId, FocusContext, ModelContext, SingletonEntity, TypedActionView,
    View, ViewContext, ViewHandle, WindowId,
};

use crate::appearance::Appearance;
use crate::editor::{
    EditorView, Event as EditorEvent, PropagateAndNoOpNavigationKeys,
    PropagateHorizontalNavigationKeys, SingleLineEditorOptions, TextOptions,
};
#[cfg(not(target_family = "wasm"))]
use crate::terminal::available_shells::AvailableShell;
use crate::ui_components::icons::Icon;

const SSH_REMOTE_HOSTS_PREF_KEY: &str = "WorkspaceSshRemoteHosts";
const SIDEBAR_HORIZONTAL_PADDING: f32 = 12.;
const ICON_BUTTON_SIZE: f32 = 22.;
const WIZARD_WIDTH: f32 = 700.;
const WIZARD_HEIGHT: f32 = 600.;
const WIZARD_RIGHT_WIDTH: f32 = WIZARD_WIDTH;
const WIZARD_RIGHT_HORIZONTAL_PADDING: f32 = 22.;
const WIZARD_BODY_WIDTH: f32 = WIZARD_RIGHT_WIDTH - WIZARD_RIGHT_HORIZONTAL_PADDING * 2.;
const WIZARD_BODY_INNER_PADDING: f32 = 12.;
const WIZARD_BODY_CONTENT_WIDTH: f32 = WIZARD_BODY_WIDTH - WIZARD_BODY_INNER_PADDING * 2.;
const WIZARD_HEADER_HEIGHT: f32 = 58.;
const RESOURCE_SUMMARY_WIDTH: f32 = 238.;
const DEFAULT_REMOTE_SETUP_DIR: &str = "/home/.miowarp";
const NODE_INSTALL_VERSION: &str = "v22.11.0";
const RIPGREP_INSTALL_VERSION: &str = "14.1.1";
const DELETE_CONFIRMATION_PROMPT_WIDTH: f32 = 260.;
const DELETE_CONFIRMATION_PROMPT_OFFSET: f32 = 10.;
pub const SSH_REMOTE_LOCAL_ENVIRONMENT_ID: &str = "local";

#[cfg(not(target_family = "wasm"))]
const SSH_PREFERRED_KEX_ALGORITHMS: &[&str] = &[
    "curve25519-sha256",
    "curve25519-sha256@libssh.org",
    "ecdh-sha2-nistp256",
    "ecdh-sha2-nistp384",
    "ecdh-sha2-nistp521",
    "diffie-hellman-group-exchange-sha256",
    "diffie-hellman-group16-sha512",
    "diffie-hellman-group18-sha512",
    "diffie-hellman-group14-sha256",
    "diffie-hellman-group14-sha1",
    "diffie-hellman-group-exchange-sha1",
    "diffie-hellman-group1-sha1",
    "ext-info-c",
    "kex-strict-c-v00@openssh.com",
];

#[cfg(not(target_family = "wasm"))]
const SSH_PREFERRED_HOST_KEY_ALGORITHMS: &[&str] = &[
    "ssh-ed25519",
    "ssh-ed25519-cert-v01@openssh.com",
    "ecdsa-sha2-nistp256",
    "ecdsa-sha2-nistp384",
    "ecdsa-sha2-nistp521",
    "ecdsa-sha2-nistp256-cert-v01@openssh.com",
    "ecdsa-sha2-nistp384-cert-v01@openssh.com",
    "ecdsa-sha2-nistp521-cert-v01@openssh.com",
    "rsa-sha2-512",
    "rsa-sha2-256",
    "rsa-sha2-512-cert-v01@openssh.com",
    "rsa-sha2-256-cert-v01@openssh.com",
    "ssh-rsa",
    "ssh-rsa-cert-v01@openssh.com",
    "ssh-dss",
];

#[cfg(not(target_family = "wasm"))]
const SSH_PREFERRED_CIPHER_ALGORITHMS: &[&str] = &[
    "chacha20-poly1305@openssh.com",
    "aes256-gcm@openssh.com",
    "aes128-gcm@openssh.com",
    "aes256-ctr",
    "aes192-ctr",
    "aes128-ctr",
    "aes256-cbc",
    "rijndael-cbc@lysator.liu.se",
    "aes192-cbc",
    "aes128-cbc",
    "blowfish-cbc",
    "cast128-cbc",
    "3des-cbc",
    "arcfour128",
    "arcfour",
];

#[cfg(not(target_family = "wasm"))]
const SSH_PREFERRED_MAC_ALGORITHMS: &[&str] = &[
    "hmac-sha2-512-etm@openssh.com",
    "hmac-sha2-256-etm@openssh.com",
    "hmac-sha2-512",
    "hmac-sha2-256",
    "hmac-sha1-etm@openssh.com",
    "hmac-sha1",
    "hmac-sha1-96",
    "hmac-ripemd160@openssh.com",
    "hmac-ripemd160",
    "hmac-md5",
    "hmac-md5-96",
];

#[derive(Clone, Debug)]
pub enum SshRemoteModelEvent {
    HostsChanged,
    ActiveHostChanged,
    ConnectionStateChanged,
}

pub fn ssh_remote_environment_id(host_id: &str) -> String {
    format!("ssh:{host_id}")
}

pub fn ssh_remote_host_id_from_environment_id(environment_id: &str) -> Option<&str> {
    environment_id.strip_prefix("ssh:")
}

#[cfg(not(target_family = "wasm"))]
fn embedded_ssh_pty_executable() -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    let exe_name = if cfg!(windows) {
        "agentwarp-ssh-pty.exe"
    } else {
        "agentwarp-ssh-pty"
    };
    let helper = current_exe.parent()?.join(exe_name);
    helper.exists().then_some(helper)
}

#[cfg(not(target_family = "wasm"))]
fn ssh_remote_bridge_env(
    target: &SshRemoteResolvedTarget,
    remote_shell: &str,
    remote_setup_dir: &str,
    remote_cwd: Option<&Path>,
) -> HashMap<OsString, OsString> {
    let mut env = HashMap::new();
    env.insert(
        OsString::from("AGENTWARP_SSH_HOST"),
        OsString::from(target.host.trim()),
    );
    env.insert(
        OsString::from("AGENTWARP_SSH_PORT"),
        OsString::from(target.port.to_string()),
    );
    env.insert(
        OsString::from("AGENTWARP_SSH_USER"),
        OsString::from(target.user.trim()),
    );
    match &target.auth {
        SshRemoteResolvedAuth::Password(password) => {
            env.insert(
                OsString::from("AGENTWARP_SSH_AUTH"),
                OsString::from("password"),
            );
            env.insert(
                OsString::from("AGENTWARP_SSH_PASSWORD"),
                OsString::from(password),
            );
        }
        SshRemoteResolvedAuth::PrivateKey(identity_file) => {
            env.insert(
                OsString::from("AGENTWARP_SSH_AUTH"),
                OsString::from("private_key"),
            );
            env.insert(
                OsString::from("AGENTWARP_SSH_IDENTITY_FILE"),
                OsString::from(identity_file.as_os_str()),
            );
        }
    }
    if !remote_shell.trim().is_empty() {
        env.insert(
            OsString::from("AGENTWARP_SSH_REMOTE_SHELL"),
            OsString::from(remote_shell.trim()),
        );
    }
    if !remote_setup_dir.trim().is_empty() {
        env.insert(
            OsString::from("AGENTWARP_SSH_REMOTE_SETUP_DIR"),
            OsString::from(remote_setup_dir.trim()),
        );
    }
    if let Some(remote_cwd) = remote_cwd {
        env.insert(
            OsString::from("AGENTWARP_SSH_REMOTE_CWD"),
            OsString::from(remote_cwd.as_os_str()),
        );
    }
    env
}

#[cfg(not(target_family = "wasm"))]
fn ssh_remote_shell_type(host: &SshRemoteHost) -> ShellType {
    let shell = host.remote_shell.trim().to_ascii_lowercase();
    if shell.contains("zsh") {
        ShellType::Zsh
    } else if shell.contains("fish") {
        ShellType::Fish
    } else if shell.contains("pwsh") || shell.contains("powershell") {
        ShellType::PowerShell
    } else {
        ShellType::Bash
    }
}

#[cfg(not(target_family = "wasm"))]
pub fn ssh_remote_terminal_launch(
    host: &SshRemoteHost,
    remote_cwd: Option<&Path>,
) -> Result<(AvailableShell, HashMap<OsString, OsString>), String> {
    let target = host.resolve_embedded_target()?;
    let helper = embedded_ssh_pty_executable().ok_or_else(|| {
        "Embedded SSH terminal helper was not found next to the app binary. Build agentwarp-ssh-pty and try again.".to_owned()
    })?;
    let env_vars = ssh_remote_bridge_env(
        &target,
        &host.remote_shell,
        &host.remote_setup_dir,
        remote_cwd,
    );
    let shell_type = ssh_remote_shell_type(host);
    Ok((
        AvailableShell::new_custom_shell("agentwarp-ssh-pty".to_owned(), helper, shell_type),
        env_vars,
    ))
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SshRemoteConnectionMethod {
    Manual,
    SshConfig,
}

impl Default for SshRemoteConnectionMethod {
    fn default() -> Self {
        Self::Manual
    }
}

impl SshRemoteConnectionMethod {
    fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual SSH",
            Self::SshConfig => "SSH config alias",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Manual => "Fill host, port, user, and authentication options.",
            Self::SshConfig => "Use an existing host entry from ~/.ssh/config.",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Manual => Icon::Terminal,
            Self::SshConfig => Icon::Settings,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SshRemoteAuthMethod {
    PasswordPrompt,
    PrivateKey,
}

impl Default for SshRemoteAuthMethod {
    fn default() -> Self {
        Self::PasswordPrompt
    }
}

impl SshRemoteAuthMethod {
    fn label(self) -> &'static str {
        match self {
            Self::PasswordPrompt => "Password prompt",
            Self::PrivateKey => "Private key",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::PasswordPrompt => "SSH asks for the password inside the terminal session.",
            Self::PrivateKey => "Use a local private key path with the ssh -i option.",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::PasswordPrompt => Icon::Lock,
            Self::PrivateKey => Icon::Key,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SshRemoteAgentInstallStrategy {
    Prompt,
    RemoteDownload,
    LocalUpload,
}

impl Default for SshRemoteAgentInstallStrategy {
    fn default() -> Self {
        Self::RemoteDownload
    }
}

impl SshRemoteAgentInstallStrategy {
    pub fn as_label(&self) -> &'static str {
        match self {
            Self::Prompt => "Ask each time",
            Self::RemoteDownload => "Remote download",
            Self::LocalUpload => "Local download + upload",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::Prompt => "Decide after Agentwarp checks the remote machine.",
            Self::RemoteDownload => "Download missing packages directly on the remote host.",
            Self::LocalUpload => "Download locally, then upload packages to the remote host.",
        }
    }

    fn icon(&self) -> Icon {
        match self {
            Self::Prompt => Icon::HelpCircle,
            Self::RemoteDownload => Icon::Download,
            Self::LocalUpload => Icon::UploadCloud,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SshRemoteResource {
    NodeRuntime,
    TerminalRuntime,
    ProxyRuntime,
    ClaudeCli,
    CodexCli,
    GeminiCli,
    OpenCodeCli,
    Ripgrep,
}

impl SshRemoteResource {
    fn all() -> &'static [Self] {
        &[
            Self::NodeRuntime,
            Self::TerminalRuntime,
            Self::ProxyRuntime,
            Self::ClaudeCli,
            Self::CodexCli,
            Self::GeminiCli,
            Self::OpenCodeCli,
            Self::Ripgrep,
        ]
    }

    fn required() -> &'static [Self] {
        &[
            Self::NodeRuntime,
            Self::TerminalRuntime,
            Self::ProxyRuntime,
            Self::Ripgrep,
        ]
    }

    fn selectable() -> &'static [Self] {
        &[
            Self::ClaudeCli,
            Self::CodexCli,
            Self::GeminiCli,
            Self::OpenCodeCli,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            Self::NodeRuntime => "Node.js Runtime",
            Self::TerminalRuntime => "Terminal Runtime",
            Self::ProxyRuntime => "Proxy Runtime",
            Self::ClaudeCli => "Claude CLI",
            Self::CodexCli => "Codex CLI",
            Self::GeminiCli => "Gemini CLI",
            Self::OpenCodeCli => "OpenCode CLI",
            Self::Ripgrep => "Ripgrep",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::NodeRuntime => "Node runtime required by bundled CLI tooling.",
            Self::TerminalRuntime => "Remote terminal bootstrap and shell utilities.",
            Self::ProxyRuntime => "Compatibility proxy for agent CLI runtimes.",
            Self::ClaudeCli => "Claude agent command line integration.",
            Self::CodexCli => "Codex agent command line integration.",
            Self::GeminiCli => "Gemini agent command line integration.",
            Self::OpenCodeCli => "OpenCode agent command line integration.",
            Self::Ripgrep => "Fast remote project search.",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::NodeRuntime => Icon::NodeJS,
            Self::TerminalRuntime => Icon::Terminal,
            Self::ProxyRuntime => Icon::Dataflow,
            Self::ClaudeCli => Icon::Stars,
            Self::CodexCli => Icon::Code2,
            Self::GeminiCli => Icon::Stars,
            Self::OpenCodeCli => Icon::TerminalInput,
            Self::Ripgrep => Icon::Search,
        }
    }

    fn command_name(self) -> Option<&'static str> {
        match self {
            Self::NodeRuntime => Some("node"),
            Self::TerminalRuntime => Some("sh"),
            Self::ProxyRuntime => Some("agentwarp-proxy"),
            Self::ClaudeCli => Some("claude"),
            Self::CodexCli => Some("codex"),
            Self::GeminiCli => Some("gemini"),
            Self::OpenCodeCli => Some("opencode"),
            Self::Ripgrep => Some("rg"),
        }
    }

    fn is_required(self) -> bool {
        matches!(
            self,
            Self::NodeRuntime | Self::TerminalRuntime | Self::ProxyRuntime | Self::Ripgrep
        )
    }

    fn from_stored_name(name: &str) -> Option<Self> {
        match name {
            "node_runtime" => Some(Self::NodeRuntime),
            "terminal_runtime" => Some(Self::TerminalRuntime),
            "proxy_runtime" => Some(Self::ProxyRuntime),
            "claude_cli" => Some(Self::ClaudeCli),
            "codex_cli" => Some(Self::CodexCli),
            "gemini_cli" => Some(Self::GeminiCli),
            "open_code_cli" => Some(Self::OpenCodeCli),
            "ripgrep" => Some(Self::Ripgrep),
            _ => None,
        }
    }
}

fn default_resources() -> Vec<SshRemoteResource> {
    normalize_resources(SshRemoteResource::required().to_vec())
}

fn default_remote_setup_dir() -> String {
    DEFAULT_REMOTE_SETUP_DIR.to_owned()
}

fn normalize_resources(resources: Vec<SshRemoteResource>) -> Vec<SshRemoteResource> {
    SshRemoteResource::all()
        .iter()
        .filter(|resource| resource.is_required() || resources.contains(resource))
        .copied()
        .collect()
}

fn deserialize_resources<'de, D>(deserializer: D) -> Result<Vec<SshRemoteResource>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let stored = Vec::<String>::deserialize(deserializer)?;
    Ok(normalize_resources(
        stored
            .iter()
            .filter_map(|resource| SshRemoteResource::from_stored_name(resource))
            .collect(),
    ))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SshRemoteHost {
    pub id: String,
    pub name: String,
    pub host: String,
    pub user: String,
    pub port: Option<u16>,
    pub identity_file: String,
    #[serde(default)]
    pub password: String,
    pub remote_shell: String,
    pub agent_install_strategy: SshRemoteAgentInstallStrategy,
    #[serde(default)]
    pub connection_method: SshRemoteConnectionMethod,
    #[serde(default)]
    pub ssh_config_alias: String,
    #[serde(default)]
    pub auth_method: SshRemoteAuthMethod,
    #[serde(
        default = "default_resources",
        deserialize_with = "deserialize_resources"
    )]
    pub resources: Vec<SshRemoteResource>,
    #[serde(default = "default_remote_setup_dir")]
    pub remote_setup_dir: String,
}

impl SshRemoteHost {
    pub fn display_name(&self) -> &str {
        if self.name.trim().is_empty() {
            if self.connection_method == SshRemoteConnectionMethod::SshConfig
                && !self.ssh_config_alias.trim().is_empty()
            {
                self.ssh_config_alias.trim()
            } else {
                self.host.trim()
            }
        } else {
            self.name.trim()
        }
    }

    pub fn user_host(&self) -> String {
        if self.connection_method == SshRemoteConnectionMethod::SshConfig
            && !self.ssh_config_alias.trim().is_empty()
        {
            self.ssh_config_alias.trim().to_owned()
        } else if self.user.trim().is_empty() {
            self.host.trim().to_owned()
        } else {
            format!("{}@{}", self.user.trim(), self.host.trim())
        }
    }

    pub fn selected_resources(&self) -> Vec<SshRemoteResource> {
        if self.resources.is_empty() {
            default_resources()
        } else {
            normalize_resources(self.resources.clone())
        }
    }
}

fn normalize_host(mut host: SshRemoteHost) -> SshRemoteHost {
    host.resources = if host.resources.is_empty() {
        default_resources()
    } else {
        normalize_resources(host.resources)
    };

    if host.agent_install_strategy == SshRemoteAgentInstallStrategy::Prompt {
        host.agent_install_strategy = SshRemoteAgentInstallStrategy::RemoteDownload;
    }
    if host.remote_setup_dir.trim().is_empty() {
        host.remote_setup_dir = default_remote_setup_dir();
    }

    host
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        "''".to_owned()
    } else if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':' | '@'))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[derive(Debug)]
enum EmbeddedSshSetupFailure {
    Failed(String),
}

impl EmbeddedSshSetupFailure {
    fn message(self) -> String {
        match self {
            Self::Failed(message) => message,
        }
    }
}

#[cfg(not(target_family = "wasm"))]
fn expand_tilde_path(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

#[cfg(not(target_family = "wasm"))]
fn local_username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "root".to_owned())
}

#[cfg(not(target_family = "wasm"))]
#[derive(Clone, Debug)]
pub enum SshRemoteResolvedAuth {
    Password(String),
    PrivateKey(PathBuf),
}

#[cfg(not(target_family = "wasm"))]
#[derive(Clone, Debug)]
pub struct SshRemoteResolvedTarget {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: SshRemoteResolvedAuth,
}

#[cfg(not(target_family = "wasm"))]
#[derive(Default)]
struct ParsedSshConfigHost {
    host_name: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<String>,
    unsupported_options: Vec<String>,
}

#[cfg(not(target_family = "wasm"))]
fn strip_ssh_config_comment(line: &str) -> String {
    let mut quoted = false;
    let mut escaped = false;
    let mut out = String::new();
    for ch in line.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if quoted => {
                escaped = true;
                out.push(ch);
            }
            '"' => {
                quoted = !quoted;
                out.push(ch);
            }
            '#' if !quoted => break,
            _ => out.push(ch),
        }
    }
    out.trim().to_owned()
}

#[cfg(not(target_family = "wasm"))]
fn ssh_config_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ch if ch.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

#[cfg(not(target_family = "wasm"))]
fn ssh_config_pattern_matches(pattern: &str, value: &str) -> bool {
    fn matches(pattern: &[u8], value: &[u8]) -> bool {
        match pattern.split_first() {
            None => value.is_empty(),
            Some((&b'*', rest)) => {
                matches(rest, value) || (!value.is_empty() && matches(pattern, &value[1..]))
            }
            Some((&b'?', rest)) => !value.is_empty() && matches(rest, &value[1..]),
            Some((&expected, rest)) => value.split_first().is_some_and(|(&actual, value_rest)| {
                expected == actual && matches(rest, value_rest)
            }),
        }
    }
    matches(pattern.as_bytes(), value.as_bytes())
}

#[cfg(not(target_family = "wasm"))]
fn ssh_config_host_matches(patterns: &[String], alias: &str) -> bool {
    let mut matched = false;
    for pattern in patterns {
        if let Some(negated) = pattern.strip_prefix('!') {
            if ssh_config_pattern_matches(negated, alias) {
                return false;
            }
        } else if ssh_config_pattern_matches(pattern, alias) {
            matched = true;
        }
    }
    matched
}

#[cfg(not(target_family = "wasm"))]
fn default_ssh_config_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".ssh/config"))
        .or_else(|| {
            std::env::var("USERPROFILE")
                .ok()
                .map(|home| PathBuf::from(home).join(".ssh/config"))
        })
}

#[cfg(not(target_family = "wasm"))]
fn substitute_ssh_config_tokens(value: &str, host: &str, user: &str, port: u16) -> String {
    value
        .replace("%h", host)
        .replace("%n", host)
        .replace("%r", user)
        .replace("%p", &port.to_string())
}

#[cfg(not(target_family = "wasm"))]
fn parse_ssh_config_alias(alias: &str) -> Result<ParsedSshConfigHost, String> {
    let config_path = default_ssh_config_path()
        .ok_or_else(|| "Could not locate ~/.ssh/config for this platform.".to_owned())?;
    let contents = fs::read_to_string(&config_path).map_err(|err| {
        format!(
            "Failed to read SSH config at {}: {err}",
            config_path.display()
        )
    })?;

    let mut parsed = ParsedSshConfigHost::default();
    let mut active = false;
    let mut matched_any = false;

    for raw_line in contents.lines() {
        let line = strip_ssh_config_comment(raw_line);
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let keyword = parts.next().unwrap_or_default().to_ascii_lowercase();
        let value = parts.next().unwrap_or_default().trim();

        if keyword == "host" {
            let patterns = ssh_config_words(value);
            active = ssh_config_host_matches(&patterns, alias);
            matched_any |= active;
            continue;
        }

        if !active {
            continue;
        }

        match keyword.as_str() {
            "hostname" if parsed.host_name.is_none() => {
                parsed.host_name = ssh_config_words(value).into_iter().next();
            }
            "user" if parsed.user.is_none() => {
                parsed.user = ssh_config_words(value).into_iter().next();
            }
            "port" if parsed.port.is_none() => {
                parsed.port = ssh_config_words(value)
                    .into_iter()
                    .next()
                    .and_then(|port| port.parse::<u16>().ok());
            }
            "identityfile" if parsed.identity_file.is_none() => {
                parsed.identity_file = ssh_config_words(value).into_iter().next();
            }
            "proxyjump" | "proxycommand" | "canonicalizehostname" | "match" => {
                if !parsed
                    .unsupported_options
                    .iter()
                    .any(|option| option == &keyword)
                {
                    parsed.unsupported_options.push(keyword);
                }
            }
            _ => {}
        }
    }

    if matched_any {
        Ok(parsed)
    } else {
        Err(format!("SSH config alias '{alias}' was not found."))
    }
}

#[cfg(not(target_family = "wasm"))]
impl SshRemoteHost {
    pub fn resolve_embedded_target(&self) -> Result<SshRemoteResolvedTarget, String> {
        let mut remote_host = self.host.trim().to_owned();
        let mut port = self.port.unwrap_or(22);
        let mut user = if self.user.trim().is_empty() {
            local_username()
        } else {
            self.user.trim().to_owned()
        };
        let mut identity_file = self.identity_file.trim().to_owned();

        if self.connection_method == SshRemoteConnectionMethod::SshConfig {
            let alias = self.ssh_config_alias.trim();
            if alias.is_empty() {
                return Err("SSH config alias is required.".to_owned());
            }
            let parsed = parse_ssh_config_alias(alias)?;
            if !parsed.unsupported_options.is_empty() {
                return Err(format!(
                    "SSH config alias '{alias}' uses unsupported options for embedded SSH: {}.",
                    parsed.unsupported_options.join(", ")
                ));
            }
            remote_host = parsed.host_name.unwrap_or_else(|| alias.to_owned());
            port = parsed.port.unwrap_or(port);
            user = parsed.user.unwrap_or(user);
            if identity_file.is_empty() {
                identity_file = parsed.identity_file.unwrap_or_default();
            }
        }

        if remote_host.is_empty() {
            return Err("Host is required for embedded SSH.".to_owned());
        }

        let auth = if self.connection_method == SshRemoteConnectionMethod::SshConfig
            && self.auth_method == SshRemoteAuthMethod::PasswordPrompt
            && self.password.trim().is_empty()
            && !identity_file.trim().is_empty()
        {
            SshRemoteResolvedAuth::PrivateKey(expand_tilde_path(&substitute_ssh_config_tokens(
                identity_file.trim(),
                &remote_host,
                &user,
                port,
            )))
        } else {
            match self.auth_method {
                SshRemoteAuthMethod::PasswordPrompt => {
                    let password = self.password.trim();
                    if password.is_empty() {
                        return Err(
                            "Saved password is required for embedded SSH password mode.".to_owned()
                        );
                    }
                    SshRemoteResolvedAuth::Password(password.to_owned())
                }
                SshRemoteAuthMethod::PrivateKey => {
                    if identity_file.trim().is_empty() {
                        return Err(
                            "Private key path is required for embedded SSH key mode.".to_owned()
                        );
                    }
                    SshRemoteResolvedAuth::PrivateKey(expand_tilde_path(
                        &substitute_ssh_config_tokens(
                            identity_file.trim(),
                            &remote_host,
                            &user,
                            port,
                        ),
                    ))
                }
            }
        };

        Ok(SshRemoteResolvedTarget {
            host: remote_host,
            port,
            user,
            auth,
        })
    }
}

fn send_install_log_blocking(
    tx: &async_channel::Sender<SshRemoteInstallEvent>,
    line: impl Into<String>,
) {
    let _ = tx.send_blocking(SshRemoteInstallEvent::Log(line.into()));
}

#[cfg(not(target_family = "wasm"))]
fn emit_install_chunk_blocking(
    pending: &mut String,
    bytes: &[u8],
    tx: &async_channel::Sender<SshRemoteInstallEvent>,
) {
    pending.push_str(&String::from_utf8_lossy(bytes));
    while let Some(index) = pending.find('\n') {
        let mut line = pending.drain(..=index).collect::<String>();
        while line.ends_with('\n') || line.ends_with('\r') {
            line.pop();
        }
        send_install_log_blocking(tx, line);
    }
}

#[cfg(not(target_family = "wasm"))]
fn apply_embedded_ssh_algorithm_preferences(session: &ssh2::Session) {
    set_preferred_ssh_algorithms(
        session,
        ssh2::MethodType::Kex,
        SSH_PREFERRED_KEX_ALGORITHMS,
        "kex",
    );
    set_preferred_ssh_algorithms(
        session,
        ssh2::MethodType::HostKey,
        SSH_PREFERRED_HOST_KEY_ALGORITHMS,
        "host key",
    );
    set_preferred_ssh_algorithms(
        session,
        ssh2::MethodType::CryptCs,
        SSH_PREFERRED_CIPHER_ALGORITHMS,
        "client cipher",
    );
    set_preferred_ssh_algorithms(
        session,
        ssh2::MethodType::CryptSc,
        SSH_PREFERRED_CIPHER_ALGORITHMS,
        "server cipher",
    );
    set_preferred_ssh_algorithms(
        session,
        ssh2::MethodType::MacCs,
        SSH_PREFERRED_MAC_ALGORITHMS,
        "client mac",
    );
    set_preferred_ssh_algorithms(
        session,
        ssh2::MethodType::MacSc,
        SSH_PREFERRED_MAC_ALGORITHMS,
        "server mac",
    );
}

#[cfg(not(target_family = "wasm"))]
fn set_preferred_ssh_algorithms(
    session: &ssh2::Session,
    method_type: ssh2::MethodType,
    preferred_algorithms: &[&str],
    label: &str,
) {
    let algorithms = match session.supported_algs(method_type) {
        Ok(supported_algorithms) => preferred_algorithms
            .iter()
            .copied()
            .filter(|algorithm| {
                supported_algorithms
                    .iter()
                    .any(|supported| supported == algorithm)
            })
            .collect::<Vec<_>>(),
        Err(err) => {
            log::debug!("Embedded SSH could not inspect supported {label} algorithms: {err}");
            preferred_algorithms.to_vec()
        }
    };

    if algorithms.is_empty() {
        return;
    }

    let preferences = algorithms.join(",");
    if let Err(err) = session.method_pref(method_type, &preferences) {
        log::debug!("Embedded SSH could not set {label} algorithm preferences: {err}");
    }
}

#[cfg(not(target_family = "wasm"))]
fn embedded_ssh_handshake_error(error: ssh2::Error) -> EmbeddedSshSetupFailure {
    let error = error.to_string();
    let hint = if error.contains("Unable to exchange encryption keys") {
        " The SSH server and embedded client could not agree on key exchange, host key, cipher, or MAC algorithms."
    } else {
        ""
    };
    EmbeddedSshSetupFailure::Failed(format!("Embedded SSH handshake failed: {error}.{hint}"))
}

#[cfg(not(target_family = "wasm"))]
fn connect_embedded_ssh_session(
    target: &SshRemoteResolvedTarget,
) -> Result<ssh2::Session, EmbeddedSshSetupFailure> {
    let mut addresses = (target.host.as_str(), target.port)
        .to_socket_addrs()
        .map_err(|err| {
            EmbeddedSshSetupFailure::Failed(format!(
                "Embedded SSH failed to resolve {}:{}: {err}",
                target.host, target.port
            ))
        })?;
    let address = addresses.next().ok_or_else(|| {
        EmbeddedSshSetupFailure::Failed(format!(
            "Embedded SSH could not resolve {}:{}",
            target.host, target.port
        ))
    })?;
    let tcp = TcpStream::connect_timeout(&address, Duration::from_secs(20)).map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!(
            "Embedded SSH failed to connect {}:{}: {err}",
            target.host, target.port
        ))
    })?;
    tcp.set_nodelay(true).ok();

    let mut session = ssh2::Session::new().map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH session init failed: {err}"))
    })?;
    session.set_tcp_stream(tcp);
    session.set_blocking(true);
    apply_embedded_ssh_algorithm_preferences(&session);
    session.handshake().map_err(embedded_ssh_handshake_error)?;

    match &target.auth {
        SshRemoteResolvedAuth::Password(password) => {
            session
                .userauth_password(&target.user, password)
                .map_err(|err| {
                    EmbeddedSshSetupFailure::Failed(format!(
                        "Embedded SSH password authentication failed: {err}"
                    ))
                })?;
        }
        SshRemoteResolvedAuth::PrivateKey(identity_file) => {
            session
                .userauth_pubkey_file(&target.user, None, Path::new(identity_file), None)
                .map_err(|err| {
                    EmbeddedSshSetupFailure::Failed(format!(
                        "Embedded SSH private key authentication failed: {err}"
                    ))
                })?;
        }
    }

    if session.authenticated() {
        Ok(session)
    } else {
        Err(EmbeddedSshSetupFailure::Failed(
            "Embedded SSH authentication did not complete.".to_owned(),
        ))
    }
}

#[cfg(not(target_family = "wasm"))]
fn run_embedded_ssh_script_blocking(
    target: SshRemoteResolvedTarget,
    remote_script: String,
    tx: async_channel::Sender<SshRemoteInstallEvent>,
) -> Result<(), EmbeddedSshSetupFailure> {
    send_install_log_blocking(
        &tx,
        format!(
            "[info] embedded ssh connecting to {}@{}:{}",
            target.user, target.host, target.port
        ),
    );
    let session = connect_embedded_ssh_session(&target)?;
    send_install_log_blocking(&tx, "[ok] embedded ssh authenticated");

    let mut channel = session.channel_session().map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to open channel: {err}"))
    })?;
    channel.exec("sh -s").map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to start remote shell: {err}"))
    })?;
    channel.write_all(b"exec 2>&1\n").map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!(
            "Embedded SSH failed to write setup prelude: {err}"
        ))
    })?;
    channel
        .write_all(remote_script.as_bytes())
        .and_then(|_| channel.write_all(b"\n"))
        .and_then(|_| channel.flush())
        .map_err(|err| {
            EmbeddedSshSetupFailure::Failed(format!(
                "Embedded SSH failed to upload setup script: {err}"
            ))
        })?;
    channel.send_eof().map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to close script input: {err}"))
    })?;

    let mut pending = String::new();
    let mut buffer = [0_u8; 8192];
    loop {
        match channel.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => emit_install_chunk_blocking(&mut pending, &buffer[..count], &tx),
            Err(err) => {
                return Err(EmbeddedSshSetupFailure::Failed(format!(
                    "Embedded SSH failed to read setup output: {err}"
                )));
            }
        }
    }
    if !pending.is_empty() {
        send_install_log_blocking(&tx, pending);
    }

    channel.wait_close().map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to close channel: {err}"))
    })?;
    let exit_status = channel.exit_status().map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to read exit status: {err}"))
    })?;
    if exit_status == 0 {
        Ok(())
    } else {
        Err(EmbeddedSshSetupFailure::Failed(format!(
            "Remote setup exited with code {exit_status}"
        )))
    }
}

#[cfg(not(target_family = "wasm"))]
async fn run_embedded_ssh_script(
    host: SshRemoteHost,
    remote_script: String,
    tx: async_channel::Sender<SshRemoteInstallEvent>,
) -> Result<(), EmbeddedSshSetupFailure> {
    let target = host
        .resolve_embedded_target()
        .map_err(EmbeddedSshSetupFailure::Failed)?;
    tokio::task::spawn_blocking(move || run_embedded_ssh_script_blocking(target, remote_script, tx))
        .await
        .map_err(|err| {
            EmbeddedSshSetupFailure::Failed(format!("Embedded SSH task failed: {err}"))
        })?
}

#[cfg(target_family = "wasm")]
async fn run_embedded_ssh_script(
    _host: SshRemoteHost,
    _remote_script: String,
    _tx: async_channel::Sender<SshRemoteInstallEvent>,
) -> Result<(), EmbeddedSshSetupFailure> {
    Err(EmbeddedSshSetupFailure::Failed(
        "Embedded SSH transport is not available in the browser build.".to_owned(),
    ))
}

#[cfg(not(target_family = "wasm"))]
fn verify_embedded_ssh_connection_blocking(
    target: SshRemoteResolvedTarget,
) -> Result<(), EmbeddedSshSetupFailure> {
    let _session = connect_embedded_ssh_session(&target)?;
    Ok(())
}

#[cfg(not(target_family = "wasm"))]
pub async fn verify_embedded_ssh_connection(host: SshRemoteHost) -> Result<(), String> {
    let target = host.resolve_embedded_target()?;
    tokio::task::spawn_blocking(move || verify_embedded_ssh_connection_blocking(target))
        .await
        .map_err(|err| format!("Embedded SSH probe task failed: {err}"))?
        .map_err(EmbeddedSshSetupFailure::message)
}

#[cfg(target_family = "wasm")]
pub async fn verify_embedded_ssh_connection(_host: SshRemoteHost) -> Result<(), String> {
    Err("Embedded SSH transport is not available in the browser build.".to_owned())
}

#[cfg(not(target_family = "wasm"))]
#[allow(dead_code)]
fn read_remote_agent_api_usage_log_blocking(
    target: SshRemoteResolvedTarget,
) -> Result<String, EmbeddedSshSetupFailure> {
    let session = connect_embedded_ssh_session(&target)?;
    let mut channel = session.channel_session().map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to open channel: {err}"))
    })?;
    channel
        .exec("cat \"$HOME/.agentwarp/agent-api-usage.ndjson\" 2>/dev/null || true")
        .map_err(|err| {
            EmbeddedSshSetupFailure::Failed(format!(
                "Embedded SSH failed to read Agent API usage log: {err}"
            ))
        })?;

    let mut output = String::new();
    channel.read_to_string(&mut output).map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!(
            "Embedded SSH failed to read Agent API usage output: {err}"
        ))
    })?;
    channel.wait_close().map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to close channel: {err}"))
    })?;
    Ok(output)
}

#[cfg(not(target_family = "wasm"))]
#[allow(dead_code)]
pub async fn read_remote_agent_api_usage_log(host: SshRemoteHost) -> Result<String, String> {
    let target = host.resolve_embedded_target()?;
    tokio::task::spawn_blocking(move || read_remote_agent_api_usage_log_blocking(target))
        .await
        .map_err(|err| format!("Embedded SSH usage log task failed: {err}"))?
        .map_err(EmbeddedSshSetupFailure::message)
}

#[cfg(target_family = "wasm")]
pub async fn read_remote_agent_api_usage_log(_host: SshRemoteHost) -> Result<String, String> {
    Err("Embedded SSH usage log reading is not available in the browser build.".to_owned())
}

#[cfg(not(target_family = "wasm"))]
fn run_embedded_ssh_script_capture_blocking(
    target: SshRemoteResolvedTarget,
    remote_script: String,
) -> Result<String, EmbeddedSshSetupFailure> {
    let session = connect_embedded_ssh_session(&target)?;
    let mut channel = session.channel_session().map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to open channel: {err}"))
    })?;
    channel.exec("sh -s").map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to start remote shell: {err}"))
    })?;
    channel.write_all(b"exec 2>&1\n").map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!(
            "Embedded SSH failed to write script prelude: {err}"
        ))
    })?;
    channel
        .write_all(remote_script.as_bytes())
        .and_then(|_| channel.write_all(b"\n"))
        .and_then(|_| channel.flush())
        .map_err(|err| {
            EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to upload script: {err}"))
        })?;
    channel.send_eof().map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to close script input: {err}"))
    })?;

    let mut output = String::new();
    channel.read_to_string(&mut output).map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to read output: {err}"))
    })?;
    channel.wait_close().map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to close channel: {err}"))
    })?;
    let exit_status = channel.exit_status().map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to read exit status: {err}"))
    })?;
    if exit_status == 0 {
        Ok(output)
    } else {
        Err(EmbeddedSshSetupFailure::Failed(format!(
            "Remote script exited with code {exit_status}: {}",
            output.trim()
        )))
    }
}

#[cfg(not(target_family = "wasm"))]
fn claude_agent_api_settings_env_vars(
    env_vars: HashMap<String, String>,
) -> HashMap<String, String> {
    const KEYS: &[&str] = &[
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    ];

    env_vars
        .into_iter()
        .filter(|(key, value)| KEYS.contains(&key.as_str()) && !value.trim().is_empty())
        .collect()
}

#[cfg(not(target_family = "wasm"))]
fn codex_agent_api_settings_env_vars(env_vars: HashMap<String, String>) -> HashMap<String, String> {
    const KEYS: &[&str] = &["OPENAI_API_KEY", "OPENAI_BASE_URL", "OPENAI_MODEL"];

    env_vars
        .into_iter()
        .filter(|(key, value)| KEYS.contains(&key.as_str()) && !value.trim().is_empty())
        .collect()
}

#[cfg(not(target_family = "wasm"))]
fn remote_claude_agent_api_settings_sync_script(
    host: &SshRemoteHost,
    env_vars: HashMap<String, String>,
) -> Result<String, String> {
    let env_vars = claude_agent_api_settings_env_vars(env_vars);
    if env_vars.is_empty() {
        return Err("No Claude Agent API environment variables to sync.".to_owned());
    }

    let env_json = serde_json::to_string(&env_vars)
        .map_err(|error| format!("Failed to serialize Claude Agent API env: {error}"))?;
    let fallback_settings_json = serde_json::to_string_pretty(&serde_json::json!({
        "skipIntroduction": true,
        "skipDangerousModePermissionPrompt": true,
        "env": env_vars,
    }))
    .map_err(|error| format!("Failed to serialize Claude settings fallback: {error}"))?;

    let setup_dir = if host.remote_setup_dir.trim().is_empty() {
        DEFAULT_REMOTE_SETUP_DIR
    } else {
        host.remote_setup_dir.trim()
    };
    let quoted_setup_dir = shell_quote(setup_dir);
    let quoted_env_json = shell_quote(&env_json);

    Ok(format!(
        r#"set -eu
ROOT={quoted_setup_dir}
export PATH="$ROOT/bin:$ROOT/node/bin:$ROOT/npm-global/bin:$PATH"
export AGENTWARP_AGENT_API_ENV_JSON={quoted_env_json}
if command -v node >/dev/null 2>&1; then
  node <<'AGENTWARP_CLAUDE_SETTINGS'
const fs = require("fs");
const os = require("os");
const path = require("path");
const env = JSON.parse(process.env.AGENTWARP_AGENT_API_ENV_JSON || "{{}}");
const dir = path.join(os.homedir(), ".claude");
const file = path.join(dir, "settings.json");
let settings = {{}};
try {{ settings = JSON.parse(fs.readFileSync(file, "utf8")); }} catch {{}}
if (!settings || typeof settings !== "object" || Array.isArray(settings)) settings = {{}};
settings.skipIntroduction = true;
settings.skipDangerousModePermissionPrompt = true;
const existingEnv = settings.env && typeof settings.env === "object" && !Array.isArray(settings.env) ? settings.env : {{}};
const nextEnv = {{ ...existingEnv }};
for (const [key, value] of Object.entries(env)) {{
  if (typeof value === "string" && value.trim()) nextEnv[key] = value;
}}
settings.env = nextEnv;
fs.mkdirSync(dir, {{ recursive: true }});
fs.writeFileSync(file, JSON.stringify(settings, null, 2) + "\n");
AGENTWARP_CLAUDE_SETTINGS
else
  if [ ! -s "$HOME/.claude/settings.json" ]; then
    mkdir -p "$HOME/.claude"
    cat > "$HOME/.claude/settings.json" <<'AGENTWARP_CLAUDE_SETTINGS_JSON'
{fallback_settings_json}
AGENTWARP_CLAUDE_SETTINGS_JSON
  fi
fi
"#,
    ))
}

#[cfg(not(target_family = "wasm"))]
fn remote_codex_agent_api_settings_sync_script(
    host: &SshRemoteHost,
    env_vars: HashMap<String, String>,
) -> Result<String, String> {
    let env_vars = codex_agent_api_settings_env_vars(env_vars);
    if env_vars.is_empty() {
        return Err("No Codex Agent API environment variables to sync.".to_owned());
    }

    let env_json = serde_json::to_string(&env_vars)
        .map_err(|error| format!("Failed to serialize Codex Agent API env: {error}"))?;
    let setup_dir = if host.remote_setup_dir.trim().is_empty() {
        DEFAULT_REMOTE_SETUP_DIR
    } else {
        host.remote_setup_dir.trim()
    };
    let quoted_setup_dir = shell_quote(setup_dir);
    let quoted_env_json = shell_quote(&env_json);

    Ok(format!(
        r#"set -eu
ROOT={quoted_setup_dir}
export PATH="$ROOT/bin:$ROOT/node/bin:$ROOT/npm-global/bin:$PATH"
export AGENTWARP_AGENT_API_ENV_JSON={quoted_env_json}
if command -v node >/dev/null 2>&1; then
  node <<'AGENTWARP_CODEX_SETTINGS'
const fs = require("fs");
const os = require("os");
const path = require("path");
const env = JSON.parse(process.env.AGENTWARP_AGENT_API_ENV_JSON || "{{}}");
const dir = process.env.CODEX_HOME && process.env.CODEX_HOME.trim()
  ? process.env.CODEX_HOME.trim()
  : path.join(os.homedir(), ".codex");
function ensureDir() {{
  fs.mkdirSync(dir, {{ recursive: true, mode: 0o700 }});
}}
function readJson(file) {{
  try {{
    const parsed = JSON.parse(fs.readFileSync(file, "utf8"));
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed : {{}};
  }} catch {{
    return {{}};
  }}
}}
function writeJson600(file, value) {{
  ensureDir();
  fs.writeFileSync(file, JSON.stringify(value, null, 2) + "\n", {{ mode: 0o600 }});
  try {{ fs.chmodSync(file, 0o600); }} catch {{}}
}}
function tomlString(value) {{
  return JSON.stringify(String(value));
}}
function isSection(line) {{
  return /^\s*\[[^\]]+\]\s*$/.test(line);
}}
function isAssignmentFor(line, key) {{
  const trimmed = line.trimStart();
  return trimmed.startsWith(key) && trimmed.slice(key.length).trimStart().startsWith("=");
}}
function finish(lines) {{
  return lines.join("\n").replace(/\s*$/, "\n");
}}
function upsertTop(content, key, value) {{
  const lines = content ? content.split(/\n/) : [];
  if (lines.length && lines[lines.length - 1] === "") lines.pop();
  const firstSection = lines.findIndex(isSection);
  const limit = firstSection < 0 ? lines.length : firstSection;
  const assignment = `${{key}} = ${{value}}`;
  for (let i = 0; i < limit; i++) {{
    if (isAssignmentFor(lines[i], key)) {{
      lines[i] = assignment;
      return finish(lines);
    }}
  }}
  if (firstSection < 0) lines.push(assignment);
  else lines.splice(firstSection, 0, assignment);
  return finish(lines);
}}
function upsertSection(content, section, key, value) {{
  const lines = content ? content.split(/\n/) : [];
  if (lines.length && lines[lines.length - 1] === "") lines.pop();
  let start = lines.findIndex(line => line.trim() === section);
  if (start < 0) {{
    if (lines.length && lines[lines.length - 1].trim() !== "") lines.push("");
    lines.push(section, `${{key}} = ${{value}}`);
    return finish(lines);
  }}
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i++) {{
    if (isSection(lines[i])) {{
      end = i;
      break;
    }}
  }}
  for (let i = start + 1; i < end; i++) {{
    if (isAssignmentFor(lines[i], key)) {{
      lines[i] = `${{key}} = ${{value}}`;
      return finish(lines);
    }}
  }}
  lines.splice(end, 0, `${{key}} = ${{value}}`);
  return finish(lines);
}}
ensureDir();
const apiKey = (env.OPENAI_API_KEY || "").trim();
if (apiKey) {{
  const authPath = path.join(dir, "auth.json");
  const auth = readJson(authPath);
  auth.OPENAI_API_KEY = apiKey;
  if (!auth.auth_mode) auth.auth_mode = "apikey";
  writeJson600(authPath, auth);
}}
const configPath = path.join(dir, "config.toml");
let config = "";
try {{ config = fs.readFileSync(configPath, "utf8"); }} catch {{}}
const baseUrl = (env.OPENAI_BASE_URL || "").trim();
if (baseUrl) config = upsertTop(config, "openai_base_url", tomlString(baseUrl));
config = upsertTop(config, "check_for_update_on_startup", "false");
const model = (env.OPENAI_MODEL || "").trim();
if (model && model !== "default") {{
  config = upsertTop(config, "model", tomlString(model));
  config = upsertSection(config, "[notice.model_migrations]", tomlString(model), tomlString("gpt-5.4"));
}}
fs.writeFileSync(configPath, config);
AGENTWARP_CODEX_SETTINGS
fi
"#,
    ))
}

#[cfg(not(target_family = "wasm"))]
pub async fn sync_remote_claude_agent_api_settings(
    host: SshRemoteHost,
    env_vars: HashMap<String, String>,
) -> Result<(), String> {
    let remote_script = remote_claude_agent_api_settings_sync_script(&host, env_vars)?;
    let target = host.resolve_embedded_target()?;
    tokio::task::spawn_blocking(move || {
        run_embedded_ssh_script_capture_blocking(target, remote_script)
    })
    .await
    .map_err(|error| format!("Embedded SSH settings sync task failed: {error}"))?
    .map(|_| ())
    .map_err(EmbeddedSshSetupFailure::message)
}

#[cfg(target_family = "wasm")]
pub async fn sync_remote_claude_agent_api_settings(
    _host: SshRemoteHost,
    _env_vars: HashMap<String, String>,
) -> Result<(), String> {
    Err("Embedded SSH settings sync is not available in the browser build.".to_owned())
}

#[cfg(not(target_family = "wasm"))]
pub async fn sync_remote_codex_agent_api_settings(
    host: SshRemoteHost,
    env_vars: HashMap<String, String>,
) -> Result<(), String> {
    let remote_script = remote_codex_agent_api_settings_sync_script(&host, env_vars)?;
    let target = host.resolve_embedded_target()?;
    tokio::task::spawn_blocking(move || {
        run_embedded_ssh_script_capture_blocking(target, remote_script)
    })
    .await
    .map_err(|error| format!("Embedded SSH settings sync task failed: {error}"))?
    .map(|_| ())
    .map_err(EmbeddedSshSetupFailure::message)
}

#[cfg(target_family = "wasm")]
pub async fn sync_remote_codex_agent_api_settings(
    _host: SshRemoteHost,
    _env_vars: HashMap<String, String>,
) -> Result<(), String> {
    Err("Embedded SSH settings sync is not available in the browser build.".to_owned())
}

#[derive(Clone, Debug)]
pub struct SshRemoteDirectoryEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Clone, Debug)]
pub struct SshRemoteDirectoryListing {
    pub path: String,
    pub parent_path: Option<String>,
    pub entries: Vec<SshRemoteDirectoryEntry>,
}

fn remote_path_separator(path: &str) -> char {
    if path.contains('\\') && !path.contains('/') {
        '\\'
    } else {
        '/'
    }
}

fn remote_join_path(base: &str, name: &str) -> String {
    let separator = remote_path_separator(base);
    if base.ends_with(separator) {
        format!("{base}{name}")
    } else {
        format!("{base}{separator}{name}")
    }
}

fn remote_parent_path(path: &str) -> Option<String> {
    let separator = remote_path_separator(path);
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return None;
    }
    let index = trimmed.rfind(separator)?;
    if index == 0 {
        return Some(separator.to_string());
    }
    let parent = &trimmed[..index];
    if parent.ends_with(':') {
        Some(format!("{parent}{separator}"))
    } else {
        Some(parent.to_owned())
    }
}

#[cfg(not(target_family = "wasm"))]
fn ensure_remote_directory(sftp: &ssh2::Sftp, path: &str) -> Result<(), EmbeddedSshSetupFailure> {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() || trimmed.ends_with(':') {
        return Ok(());
    }
    if sftp.stat(Path::new(path)).is_ok() {
        return Ok(());
    }
    if let Some(parent) = remote_parent_path(path) {
        if parent != path {
            ensure_remote_directory(sftp, &parent)?;
        }
    }
    match sftp.mkdir(Path::new(path), 0o700) {
        Ok(()) => Ok(()),
        Err(_) if sftp.stat(Path::new(path)).is_ok() => Ok(()),
        Err(err) => Err(EmbeddedSshSetupFailure::Failed(format!(
            "Embedded SSH failed to create remote directory {path}: {err}"
        ))),
    }
}

#[cfg(not(target_family = "wasm"))]
fn remote_upload_safe_file_name(name: &str, index: usize) -> String {
    let sanitized = name
        .chars()
        .map(|ch| {
            if ch == '/' || ch == '\\' || ch == '\0' || ch.is_control() {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();
    let sanitized = sanitized.trim();
    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        format!("upload-{index}")
    } else {
        sanitized.to_owned()
    }
}

#[cfg(not(target_family = "wasm"))]
fn remote_upload_temp_dir(remote_setup_dir: &str) -> String {
    let setup_dir = remote_setup_dir
        .trim()
        .trim_end_matches(['/', '\\'])
        .to_owned();
    let setup_dir = if setup_dir.is_empty() {
        DEFAULT_REMOTE_SETUP_DIR.to_owned()
    } else {
        setup_dir
    };
    let tmp_dir = remote_join_path(&setup_dir, "tmp");
    let uploads_dir = remote_join_path(&tmp_dir, "agent-uploads");
    remote_join_path(&uploads_dir, &Uuid::new_v4().to_string())
}

#[cfg(not(target_family = "wasm"))]
fn upload_local_files_to_remote_temp_blocking(
    target: SshRemoteResolvedTarget,
    remote_setup_dir: String,
    local_paths: Vec<String>,
) -> Result<Vec<String>, EmbeddedSshSetupFailure> {
    let session = connect_embedded_ssh_session(&target)?;
    let sftp = session.sftp().map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to open SFTP: {err}"))
    })?;
    let remote_dir = remote_upload_temp_dir(&remote_setup_dir);
    ensure_remote_directory(&sftp, &remote_dir)?;

    let mut remote_paths = Vec::new();
    for (index, local_path) in local_paths.into_iter().enumerate() {
        let path = PathBuf::from(&local_path);
        let metadata = fs::metadata(&path).map_err(|err| {
            EmbeddedSshSetupFailure::Failed(format!(
                "Failed to read local file metadata for {}: {err}",
                path.display()
            ))
        })?;
        if !metadata.is_file() {
            return Err(EmbeddedSshSetupFailure::Failed(format!(
                "Only regular files can be uploaded to SSH remotes: {}",
                path.display()
            )));
        }

        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("upload-{index}"));
        let file_name = remote_upload_safe_file_name(&file_name, index);
        let remote_path = remote_join_path(&remote_dir, &file_name);
        let mut local_file = fs::File::open(&path).map_err(|err| {
            EmbeddedSshSetupFailure::Failed(format!(
                "Failed to open local file {}: {err}",
                path.display()
            ))
        })?;
        let mut remote_file = sftp.create(Path::new(&remote_path)).map_err(|err| {
            EmbeddedSshSetupFailure::Failed(format!(
                "Embedded SSH failed to create remote file {remote_path}: {err}"
            ))
        })?;
        std::io::copy(&mut local_file, &mut remote_file).map_err(|err| {
            EmbeddedSshSetupFailure::Failed(format!(
                "Embedded SSH failed to upload {} to {remote_path}: {err}",
                path.display()
            ))
        })?;
        remote_paths.push(remote_path);
    }

    Ok(remote_paths)
}

#[cfg(not(target_family = "wasm"))]
pub async fn upload_local_files_to_remote_temp(
    host: SshRemoteHost,
    local_paths: Vec<String>,
) -> Result<Vec<String>, String> {
    if local_paths.is_empty() {
        return Ok(Vec::new());
    }
    let target = host.resolve_embedded_target()?;
    let remote_setup_dir = host.remote_setup_dir.clone();
    tokio::task::spawn_blocking(move || {
        upload_local_files_to_remote_temp_blocking(target, remote_setup_dir, local_paths)
    })
    .await
    .map_err(|err| format!("Embedded SSH upload task failed: {err}"))?
    .map_err(EmbeddedSshSetupFailure::message)
}

#[cfg(target_family = "wasm")]
pub async fn upload_local_files_to_remote_temp(
    _host: SshRemoteHost,
    _local_paths: Vec<String>,
) -> Result<Vec<String>, String> {
    Err("Embedded SSH uploads are not available in the browser build.".to_owned())
}

#[cfg(not(target_family = "wasm"))]
fn list_remote_directories_blocking(
    target: SshRemoteResolvedTarget,
    requested_path: String,
) -> Result<SshRemoteDirectoryListing, EmbeddedSshSetupFailure> {
    let session = connect_embedded_ssh_session(&target)?;
    let sftp = session.sftp().map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!("Embedded SSH failed to open SFTP: {err}"))
    })?;
    let requested_path = requested_path.trim();
    let requested_path = if requested_path.is_empty() {
        "."
    } else {
        requested_path
    };
    let resolved_path = sftp
        .realpath(Path::new(requested_path))
        .unwrap_or_else(|_| PathBuf::from(requested_path));
    let resolved_path = resolved_path.to_string_lossy().to_string();
    let entries = sftp.readdir(Path::new(&resolved_path)).map_err(|err| {
        EmbeddedSshSetupFailure::Failed(format!(
            "Embedded SSH failed to list {resolved_path}: {err}"
        ))
    })?;

    let mut directory_entries = entries
        .into_iter()
        .filter_map(|(path, stat)| {
            let name = path.file_name()?.to_string_lossy().to_string();
            if name == "." || name == ".." {
                return None;
            }
            Some(SshRemoteDirectoryEntry {
                path: remote_join_path(&resolved_path, &name),
                name,
                is_dir: stat.is_dir(),
            })
        })
        .collect::<Vec<_>>();
    directory_entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(SshRemoteDirectoryListing {
        parent_path: remote_parent_path(&resolved_path),
        path: resolved_path,
        entries: directory_entries,
    })
}

#[cfg(not(target_family = "wasm"))]
pub async fn list_remote_directories(
    host: SshRemoteHost,
    path: String,
) -> Result<SshRemoteDirectoryListing, String> {
    let target = host.resolve_embedded_target()?;
    tokio::task::spawn_blocking(move || list_remote_directories_blocking(target, path))
        .await
        .map_err(|err| format!("Embedded SSH directory list task failed: {err}"))?
        .map_err(EmbeddedSshSetupFailure::message)
}

#[cfg(target_family = "wasm")]
pub async fn list_remote_directories(
    _host: SshRemoteHost,
    _path: String,
) -> Result<SshRemoteDirectoryListing, String> {
    Err("Embedded SSH directory picker is not available in the browser build.".to_owned())
}

fn resource_npm_package(resource: SshRemoteResource) -> Option<&'static str> {
    match resource {
        SshRemoteResource::ClaudeCli => Some("@anthropic-ai/claude-code"),
        SshRemoteResource::CodexCli => Some("@openai/codex"),
        SshRemoteResource::GeminiCli => Some("@google/gemini-cli"),
        SshRemoteResource::OpenCodeCli => Some("opencode-ai"),
        _ => None,
    }
}

fn resource_setup_command(resource: SshRemoteResource) -> Option<String> {
    match resource {
        SshRemoteResource::NodeRuntime => Some("ensure_node".to_owned()),
        SshRemoteResource::TerminalRuntime => Some("ensure_terminal_runtime".to_owned()),
        SshRemoteResource::Ripgrep => Some("ensure_ripgrep".to_owned()),
        SshRemoteResource::ProxyRuntime => {
            Some("ensure_internal_runtime agentwarp-proxy 'Proxy Runtime'".to_owned())
        }
        resource => {
            let package = resource_npm_package(resource)?;
            let command = resource.command_name()?;
            Some(format!(
                "ensure_npm_cli {} {} {}",
                shell_quote(command),
                shell_quote(package),
                shell_quote(resource.label())
            ))
        }
    }
}

fn remote_setup_script(host: &SshRemoteHost) -> String {
    let setup_dir = if host.remote_setup_dir.trim().is_empty() {
        DEFAULT_REMOTE_SETUP_DIR
    } else {
        host.remote_setup_dir.trim()
    };
    let selected_resources = host.selected_resources();
    let resource_setup_lines = selected_resources
        .iter()
        .filter_map(|resource| resource_setup_command(*resource))
        .collect::<Vec<_>>()
        .join("\n");
    let command_checks = selected_resources
        .iter()
        .filter_map(|resource| resource.command_name())
        .map(|command| shell_quote(command))
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        r#"set -u
ROOT={setup_dir}
NODE_VERSION={node_version}
RG_VERSION={rg_version}

log() {{ printf '%s\n' "$*"; }}
have() {{ command -v "$1" >/dev/null 2>&1; }}
download() {{
  url="$1"
  out="$2"
  if have curl; then
    curl -fL "$url" -o "$out"
  elif have wget; then
    wget -O "$out" "$url"
  else
    log "[error] curl or wget is required for remote download"
    return 1
  fi
}}

log "[info] initializing remote workspace services for $(whoami)@$(hostname)"
log "[info] setup root: $ROOT"
mkdir -p "$ROOT"/bin "$ROOT"/tmp "$ROOT"/logs "$ROOT"/node "$ROOT"/npm-global
export PATH="$ROOT/bin:$ROOT/node/bin:$ROOT/npm-global/bin:$PATH"
export npm_config_prefix="$ROOT/npm-global"
export NPM_CONFIG_PREFIX="$ROOT/npm-global"
write_env_file() {{
  root_escaped=$(printf '%s' "$ROOT" | sed "s/'/'\\\\''/g")
  {{
    printf "export AGENTWARP_REMOTE_ROOT='%s'\n" "$root_escaped"
    printf "export PATH='%s/bin:%s/node/bin:%s/npm-global/bin:'\"\\${{PATH:-}}\"\n" "$root_escaped" "$root_escaped" "$root_escaped"
    printf "export npm_config_prefix='%s/npm-global'\n" "$root_escaped"
    printf "export NPM_CONFIG_PREFIX='%s/npm-global'\n" "$root_escaped"
  }} > "$ROOT/agentwarp-env.sh"
  chmod 600 "$ROOT/agentwarp-env.sh" 2>/dev/null || true
}}
write_env_file
cat > "$ROOT/bin/agentwarp-agent-api-proxy" <<'AGENTWARP_AGENT_API_PROXY'
#!/usr/bin/env node
const http = require('http');
const https = require('https');
const {{ spawn }} = require('child_process');
const fs = require('fs');
const path = require('path');

function commandArgs() {{
  const args = process.argv.slice(2);
  return args[0] === '--' ? args.slice(1) : args;
}}

function profilesFromEnv() {{
  try {{
    return JSON.parse(process.env.AGENTWARP_AGENT_API_FALLBACKS || '[]')
      .filter(profile => profile && profile.enabled !== false && profile.base_url);
  }} catch (_) {{
    return [];
  }}
}}

function shouldRetry(status) {{
  return status === 408 || status === 409 || status === 429 || status >= 500;
}}

function usageLogPath() {{
  if (process.env.AGENTWARP_AGENT_API_USAGE_LOG) return process.env.AGENTWARP_AGENT_API_USAGE_LOG;
  if (process.env.HOME) return path.join(process.env.HOME, '.agentwarp', 'agent-api-usage.ndjson');
  return '';
}}

function writeUsageEvent(event) {{
  const filePath = usageLogPath();
  if (!filePath) return;
  try {{
    fs.mkdirSync(path.dirname(filePath), {{ recursive: true }});
    fs.appendFileSync(filePath, JSON.stringify(event) + '\n');
  }} catch (error) {{
    console.error(`agentwarp-agent-api-proxy: failed to write usage log: ${{error.message}}`);
  }}
}}

function tokenUsage(responseBody) {{
  try {{
    const parsed = JSON.parse(responseBody.toString('utf8'));
    const usage = parsed.usage || parsed.usageMetadata || parsed;
    const promptTokens = Number(
      usage.prompt_tokens || usage.input_tokens || usage.promptTokenCount || 0
    );
    const completionTokens = Number(
      usage.completion_tokens || usage.output_tokens || usage.candidatesTokenCount || 0
    );
    const totalTokens = Number(
      usage.total_tokens || usage.totalTokens || usage.totalTokenCount || 0
    );
    const safePromptTokens = Number.isFinite(promptTokens) ? promptTokens : 0;
    const safeCompletionTokens = Number.isFinite(completionTokens) ? completionTokens : 0;
    const safeTotalTokens = Number.isFinite(totalTokens) ? totalTokens : 0;
    return {{
      prompt_tokens: safePromptTokens,
      completion_tokens: safeCompletionTokens,
      total_tokens: safeTotalTokens > 0
        ? safeTotalTokens
        : safePromptTokens + safeCompletionTokens
    }};
  }} catch (_error) {{
    return {{ prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 }};
  }}
}}

function estimatedCostUsd(profile, usage) {{
  const inputCost = Number(profile.input_cost_per_million_tokens || 0);
  const outputCost = Number(profile.output_cost_per_million_tokens || 0);
  const safeInputCost = Number.isFinite(inputCost) && inputCost > 0 ? inputCost : 0;
  const safeOutputCost = Number.isFinite(outputCost) && outputCost > 0 ? outputCost : 0;
  const cost = (
    usage.prompt_tokens * safeInputCost +
    usage.completion_tokens * safeOutputCost
  ) / 1000000;
  return Number.isFinite(cost) && cost > 0 ? cost : 0;
}}

function targetUrl(profile, originalUrl) {{
  const base = String(profile.base_url || '').replace(/\/+$/, '');
  if (profile.full_url_mode) return base;
  if (base.endsWith('/v1') && originalUrl.startsWith('/v1/')) {{
    return base.slice(0, -3) + originalUrl;
  }}
  return base + originalUrl;
}}

function preferredModel(profile) {{
  if (String(profile.model || '').trim()) return String(profile.model).trim();
  const mappings = Array.isArray(profile.model_mappings) ? profile.model_mappings : [];
  const preferred = mappings.find(m => String(m.role || '').toLowerCase() === 'sonnet')
    || mappings.find(m => String(m.role || '').toLowerCase() === 'default')
    || mappings.find(m => String(m.model || '').trim());
  return preferred ? String(preferred.model || '').trim() : '';
}}

function mappingMatches(mapping, requestedModel) {{
  const requested = String(requestedModel || '').trim().toLowerCase();
  if (!requested) return false;
  for (const candidate of [mapping.role, mapping.display_name, mapping.model]) {{
    const value = String(candidate || '').trim().toLowerCase();
    if (value && (requested === value || requested.includes(value))) return true;
  }}
  return false;
}}

function mappedModel(profile, requestedModel) {{
  const mappings = Array.isArray(profile.model_mappings) ? profile.model_mappings : [];
  const mapping = mappings.find(m => String(m.model || '').trim() && mappingMatches(m, requestedModel));
  return mapping ? String(mapping.model || '').trim() : '';
}}

function rewriteBody(profile, body) {{
  try {{
    const parsed = JSON.parse(body.toString('utf8'));
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return body;
    const currentModel = String(parsed.model || '').trim();
    const nextModel = currentModel ? mappedModel(profile, currentModel) : preferredModel(profile);
    if (!nextModel) return body;
    parsed.model = nextModel;
    return Buffer.from(JSON.stringify(parsed));
  }} catch (_error) {{
    return body;
  }}
}}

function headersForProfile(headers, profile) {{
  const out = {{ ...headers }};
  delete out.host;
  delete out.authorization;
  delete out['x-api-key'];
  delete out['x-goog-api-key'];
  delete out['content-length'];
  const key = String(profile.api_key || '').trim();
  const agent = String(profile.agent || '').toLowerCase();
  if (key) {{
    if (agent.includes('claude')) out['x-api-key'] = key;
    else if (agent.includes('gemini')) out['x-goog-api-key'] = key;
    else out.authorization = `Bearer ${{key}}`;
  }}
  for (const [name, value] of Object.entries(profile.extra_env || {{}})) {{
    if (name.startsWith('header:') && String(value).trim()) {{
      out[name.slice(7).trim()] = String(value).trim();
    }}
  }}
  return out;
}}

function forward(profile, req, body) {{
  return new Promise((resolve, reject) => {{
    const url = new URL(targetUrl(profile, req.url));
    const rewrittenBody = rewriteBody(profile, body);
    const client = url.protocol === 'https:' ? https : http;
    const upstream = client.request(
      url,
      {{ method: req.method, headers: headersForProfile(req.headers, profile) }},
      res => {{
        const chunks = [];
        res.on('data', chunk => chunks.push(chunk));
        res.on('end', () => resolve({{ status: res.statusCode || 502, headers: res.headers, body: Buffer.concat(chunks) }}));
      }}
    );
    upstream.on('error', reject);
    upstream.write(rewrittenBody);
    upstream.end();
  }});
}}

async function handleRequest(profiles, req, res) {{
  const chunks = [];
  req.on('data', chunk => chunks.push(chunk));
  req.on('end', async () => {{
    const body = Buffer.concat(chunks);
    let lastError = 'No usable Agent API profile is configured';
    for (let index = 0; index < profiles.length; index++) {{
      const profile = profiles[index];
      const attempt = index + 1;
      const finalAttempt = attempt === profiles.length;
      const startedAt = Date.now();
      try {{
        const response = await forward(profile, req, body);
        const latencyMs = Date.now() - startedAt;
        const usage = tokenUsage(response.body);
        const estimatedCost = estimatedCostUsd(profile, usage);
        if (shouldRetry(response.status)) {{
          writeUsageEvent({{
            timestamp_epoch_ms: Date.now(),
            profile_id: String(profile.id || ''),
            profile_name: String(profile.name || ''),
            agent: String(profile.agent || ''),
            method: req.method,
            path: req.url,
            status: response.status,
            success: false,
            retryable: true,
            final_attempt: finalAttempt,
            attempt,
            latency_ms: latencyMs,
            request_bytes: body.length,
            response_bytes: response.body.length,
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
            estimated_cost_usd: estimatedCost,
            error: `retryable status ${{response.status}}`
          }});
          lastError = `${{profile.name || profile.id || 'profile'}} returned retryable status ${{response.status}}`;
          continue;
        }}
        writeUsageEvent({{
          timestamp_epoch_ms: Date.now(),
          profile_id: String(profile.id || ''),
          profile_name: String(profile.name || ''),
          agent: String(profile.agent || ''),
          method: req.method,
          path: req.url,
          status: response.status,
          success: response.status >= 200 && response.status < 300,
          retryable: false,
          final_attempt: true,
          attempt,
          latency_ms: latencyMs,
          request_bytes: body.length,
          response_bytes: response.body.length,
          prompt_tokens: usage.prompt_tokens,
          completion_tokens: usage.completion_tokens,
          total_tokens: usage.total_tokens,
          estimated_cost_usd: estimatedCost,
          error: ''
        }});
        res.writeHead(response.status, response.headers);
        res.end(response.body);
        return;
      }} catch (error) {{
        writeUsageEvent({{
          timestamp_epoch_ms: Date.now(),
          profile_id: String(profile.id || ''),
          profile_name: String(profile.name || ''),
          agent: String(profile.agent || ''),
          method: req.method,
          path: req.url,
          status: 0,
          success: false,
          retryable: true,
          final_attempt: finalAttempt,
          attempt,
          latency_ms: Date.now() - startedAt,
          request_bytes: body.length,
          response_bytes: 0,
          prompt_tokens: 0,
          completion_tokens: 0,
          total_tokens: 0,
          estimated_cost_usd: 0,
          error: error.message
        }});
        lastError = `${{profile.name || profile.id || 'profile'}} request failed: ${{error.message}}`;
      }}
    }}
    res.writeHead(502, {{ 'content-type': 'application/json' }});
    res.end(JSON.stringify({{ error: {{ type: 'agentwarp_agent_api_proxy_error', message: lastError }} }}));
  }});
}}

function proxyEnv(agent, proxyUrl) {{
  const env = {{ ...process.env, AGENTWARP_AGENT_API_PROXY_URL: proxyUrl, AGENTWARP_AGENT_API_PROXY_ACTIVE: '1' }};
  const lower = String(agent || '').toLowerCase();
  if (lower.includes('claude')) env.ANTHROPIC_BASE_URL = proxyUrl;
  else if (lower.includes('gemini')) env.GOOGLE_GEMINI_BASE_URL = proxyUrl;
  else env.OPENAI_BASE_URL = proxyUrl;
  return env;
}}

const args = commandArgs();
if (args.length === 0) {{
  console.error('usage: agentwarp-agent-api-proxy -- <agent-command> [args...]');
  process.exit(2);
}}

const profiles = profilesFromEnv();
if (profiles.length < 2) {{
  const child = spawn(args[0], args.slice(1), {{ stdio: 'inherit' }});
  child.on('exit', code => process.exit(code || 0));
}} else {{
  const server = http.createServer((req, res) => handleRequest(profiles, req, res));
  server.listen(0, '127.0.0.1', () => {{
    const proxyUrl = `http://127.0.0.1:${{server.address().port}}`;
    const child = spawn(args[0], args.slice(1), {{
      stdio: 'inherit',
      env: proxyEnv(process.env.AGENTWARP_AGENT_API_AGENT, proxyUrl)
    }});
    child.on('exit', code => {{
      server.close(() => process.exit(code || 0));
    }});
  }});
}}
AGENTWARP_AGENT_API_PROXY
chmod +x "$ROOT/bin/agentwarp-agent-api-proxy"

OS="$(uname -s 2>/dev/null || printf unknown)"
ARCH="$(uname -m 2>/dev/null || printf unknown)"
log "[info] detecting remote env..."
log "[info] os=$OS arch=$ARCH shell=${{SHELL:-unknown}}"

ensure_node() {{
  if [ -x "$ROOT/node/bin/node" ] && [ -x "$ROOT/node/bin/npm" ]; then
    log "[ok] managed node -> $ROOT/node/bin/node"
    log "[ok] managed npm -> $ROOT/node/bin/npm"
    return 0
  fi
  if [ "$OS" != "Linux" ]; then
    log "[error] automatic Node.js bootstrap currently supports Linux remotes; install Node.js manually for $OS"
    return 1
  fi
  case "$ARCH" in
    x86_64|amd64) node_arch="linux-x64" ;;
    aarch64|arm64) node_arch="linux-arm64" ;;
    *) log "[error] unsupported Node.js architecture: $ARCH"; return 1 ;;
  esac
  archive="node-$NODE_VERSION-$node_arch.tar.xz"
  url="https://nodejs.org/dist/$NODE_VERSION/$archive"
  log "[install] downloading Node.js $NODE_VERSION for $node_arch"
  download "$url" "$ROOT/tmp/$archive" || return 1
  rm -rf "$ROOT/node"
  mkdir -p "$ROOT/node"
  tar -xJf "$ROOT/tmp/$archive" -C "$ROOT/node" --strip-components=1 || return 1
  log "[ok] node installed -> $ROOT/node/bin/node"
  log "[ok] npm installed -> $ROOT/node/bin/npm"
}}

ensure_terminal_runtime() {{
  if have sh; then
    log "[ok] terminal shell -> $(command -v sh)"
    return 0
  fi
  log "[error] POSIX sh is required for terminal runtime"
  return 1
}}

ensure_internal_runtime() {{
  command_name="$1"
  label="$2"
  target="$ROOT/bin/$command_name"
  if [ -x "$target" ]; then
    log "[ok] $label -> $target"
    return 0
  fi
  log "[install] preparing $label runtime shim"
  case "$command_name" in
    agentwarp-proxy)
      cat > "$target" <<'EOF'
#!/bin/sh
case "$1" in
  --version|-V|version) printf '%s\n' "agentwarp-proxy 0.1.0"; exit 0 ;;
esac
if [ "$#" -gt 0 ]; then
  exec "$@"
fi
printf '%s\n' "Agentwarp proxy runtime ready"
EOF
      ;;
    *)
      log "[error] unknown internal runtime: $command_name"
      return 1
      ;;
  esac
  chmod +x "$target" || return 1
  log "[ok] $label installed -> $target"
}}

ensure_ripgrep() {{
  if have rg; then
    log "[ok] rg -> $(command -v rg)"
    return 0
  fi
  if [ "$OS" != "Linux" ]; then
    log "[warn] automatic ripgrep bootstrap skipped for $OS"
    return 0
  fi
  case "$ARCH" in
    x86_64|amd64) rg_target="x86_64-unknown-linux-musl" ;;
    aarch64|arm64) rg_target="aarch64-unknown-linux-gnu" ;;
    *) log "[warn] unsupported ripgrep architecture: $ARCH"; return 0 ;;
  esac
  archive="ripgrep-$RG_VERSION-$rg_target.tar.gz"
  url="https://github.com/BurntSushi/ripgrep/releases/download/$RG_VERSION/$archive"
  log "[install] downloading ripgrep $RG_VERSION for $rg_target"
  download "$url" "$ROOT/tmp/$archive" || return 1
  tar -xzf "$ROOT/tmp/$archive" -C "$ROOT/tmp" || return 1
  cp "$ROOT/tmp/ripgrep-$RG_VERSION-$rg_target/rg" "$ROOT/bin/rg" || return 1
  chmod +x "$ROOT/bin/rg"
  log "[ok] rg installed -> $ROOT/bin/rg"
}}

ensure_npm_cli() {{
  command_name="$1"
  package_name="$2"
  label="$3"
  managed="$ROOT/npm-global/bin/$command_name"
  if [ -x "$managed" ] && validate_command "$command_name"; then
    log "[ok] $label -> $managed"
    return 0
  fi
  ensure_node || return 1
  log "[install] npm install -g --include=optional $package_name"
  "$ROOT/node/bin/npm" install -g --include=optional "$package_name" --prefix "$ROOT/npm-global" || return 1
  hash -r 2>/dev/null || true
  if validate_command "$command_name"; then
    log "[ok] $label installed -> $(command -v "$command_name")"
  else
    log "[error] $label package installed but '$command_name' is not runnable"
    return 1
  fi
}}

validate_command() {{
  command_name="$1"
  command_path="$(command -v "$command_name" 2>/dev/null || true)"
  if [ -z "$command_path" ]; then
    return 1
  fi
  case "$command_name" in
    sh) "$command_path" -c ':' >/dev/null 2>&1 ;;
    node|npm|rg|agentwarp-proxy) "$command_path" --version >/dev/null 2>&1 || "$command_path" >/dev/null 2>&1 ;;
    *) "$command_path" --version >/dev/null 2>&1 || "$command_path" -v >/dev/null 2>&1 ;;
  esac
}}

log "[info] checking selected commands: {command_checks}"
{resource_setup_lines}

log "[info] final command check"
missing=0
for c in {command_checks}; do
  if validate_command "$c"; then
    log "[ok] $c -> $(command -v "$c")"
  else
    log "[missing] $c"
    missing=1
  fi
done
[ "$missing" -eq 0 ] || exit 1
log "[done] remote environment prepared"
"#,
        setup_dir = shell_quote(setup_dir),
        node_version = shell_quote(NODE_INSTALL_VERSION),
        rg_version = shell_quote(RIPGREP_INSTALL_VERSION),
        command_checks = command_checks,
        resource_setup_lines = resource_setup_lines,
    )
}

async fn send_install_event(
    tx: &async_channel::Sender<SshRemoteInstallEvent>,
    event: SshRemoteInstallEvent,
) {
    let _ = tx.send(event).await;
}

async fn run_remote_setup(host: SshRemoteHost, tx: async_channel::Sender<SshRemoteInstallEvent>) {
    send_install_event(
        &tx,
        SshRemoteInstallEvent::Log(format!("[info] starting setup for {}", host.user_host())),
    )
    .await;
    send_install_event(
        &tx,
        SshRemoteInstallEvent::Log(format!(
            "[info] install strategy: {}",
            host.agent_install_strategy.as_label()
        )),
    )
    .await;

    let remote_script = remote_setup_script(&host);
    match run_embedded_ssh_script(host.clone(), remote_script, tx.clone()).await {
        Ok(()) => {
            send_install_event(&tx, SshRemoteInstallEvent::Finished(Ok(()))).await;
        }
        Err(error) => {
            send_install_event(&tx, SshRemoteInstallEvent::Finished(Err(error.message()))).await;
        }
    }
}

pub struct SshRemoteModel {
    hosts: Vec<SshRemoteHost>,
    active_host_id: Option<String>,
    pending_active_host_id: Option<String>,
    connection_states: HashMap<String, SshRemoteConnectionStatus>,
    terminal_host_ids: HashMap<EntityId, String>,
}

impl Entity for SshRemoteModel {
    type Event = SshRemoteModelEvent;
}

impl SingletonEntity for SshRemoteModel {}

impl SshRemoteModel {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        Self {
            hosts: read_hosts(ctx),
            active_host_id: None,
            pending_active_host_id: None,
            connection_states: HashMap::new(),
            terminal_host_ids: HashMap::new(),
        }
    }

    pub fn hosts(&self) -> &[SshRemoteHost] {
        &self.hosts
    }

    pub fn host(&self, id: &str) -> Option<&SshRemoteHost> {
        self.hosts.iter().find(|host| host.id == id)
    }

    pub fn active_host(&self) -> Option<&SshRemoteHost> {
        self.active_host_id.as_deref().and_then(|id| self.host(id))
    }

    pub fn pending_active_host(&self) -> Option<&SshRemoteHost> {
        self.pending_active_host_id
            .as_deref()
            .and_then(|id| self.host(id))
    }

    pub fn active_environment_id(&self) -> String {
        self.active_host_id
            .as_deref()
            .map(ssh_remote_environment_id)
            .unwrap_or_else(|| SSH_REMOTE_LOCAL_ENVIRONMENT_ID.to_owned())
    }

    pub fn terminal_environment_id(&self, terminal_view_id: EntityId) -> Option<String> {
        self.terminal_host_ids
            .get(&terminal_view_id)
            .map(|host_id| ssh_remote_environment_id(host_id))
    }

    pub fn register_terminal_host(
        &mut self,
        terminal_view_id: EntityId,
        host_id: String,
        ctx: &mut ModelContext<Self>,
    ) {
        self.terminal_host_ids.insert(terminal_view_id, host_id);
        ctx.emit(SshRemoteModelEvent::ConnectionStateChanged);
    }

    pub fn unregister_terminal_host(
        &mut self,
        terminal_view_id: EntityId,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.terminal_host_ids.remove(&terminal_view_id).is_some() {
            ctx.emit(SshRemoteModelEvent::ConnectionStateChanged);
        }
    }

    pub fn connection_status(&self, host_id: &str) -> SshRemoteConnectionStatus {
        if self.active_host_id.as_deref() == Some(host_id) {
            SshRemoteConnectionStatus::Active
        } else {
            self.connection_states
                .get(host_id)
                .cloned()
                .unwrap_or(SshRemoteConnectionStatus::Idle)
        }
    }

    pub fn set_active_host(&mut self, host_id: Option<String>, ctx: &mut ModelContext<Self>) {
        if self.active_host_id == host_id {
            return;
        }
        if let Some(host_id) = host_id.as_deref() {
            self.connection_states.remove(host_id);
        }
        self.pending_active_host_id = None;
        self.active_host_id = host_id;
        ctx.emit(SshRemoteModelEvent::ActiveHostChanged);
        ctx.emit(SshRemoteModelEvent::ConnectionStateChanged);
    }

    pub fn set_host_connecting(&mut self, host_id: &str, ctx: &mut ModelContext<Self>) {
        self.pending_active_host_id = Some(host_id.to_owned());
        self.connection_states
            .insert(host_id.to_owned(), SshRemoteConnectionStatus::Connecting);
        ctx.emit(SshRemoteModelEvent::ConnectionStateChanged);
    }

    pub fn set_host_failed(
        &mut self,
        host_id: &str,
        message: String,
        ctx: &mut ModelContext<Self>,
    ) {
        if self.active_host_id.as_deref() == Some(host_id) {
            self.active_host_id = None;
            ctx.emit(SshRemoteModelEvent::ActiveHostChanged);
        }
        if self.pending_active_host_id.as_deref() == Some(host_id) {
            self.pending_active_host_id = None;
        }
        self.connection_states.insert(
            host_id.to_owned(),
            SshRemoteConnectionStatus::Failed(message),
        );
        ctx.emit(SshRemoteModelEvent::ConnectionStateChanged);
    }

    pub fn upsert_host(&mut self, mut host: SshRemoteHost, ctx: &mut ModelContext<Self>) {
        if host.id.is_empty() {
            host.id = Uuid::new_v4().to_string();
        }
        host = normalize_host(host);

        if let Some(existing) = self
            .hosts
            .iter_mut()
            .find(|existing| existing.id == host.id)
        {
            *existing = host;
        } else {
            self.hosts.push(host);
        }

        self.persist_and_emit(ctx);
    }

    pub fn delete_host(&mut self, host_id: &str, ctx: &mut ModelContext<Self>) {
        self.hosts.retain(|host| host.id != host_id);
        self.connection_states.remove(host_id);
        self.terminal_host_ids.retain(|_, id| id != host_id);
        if self.pending_active_host_id.as_deref() == Some(host_id) {
            self.pending_active_host_id = None;
        }
        if self.active_host_id.as_deref() == Some(host_id) {
            self.active_host_id = None;
            ctx.emit(SshRemoteModelEvent::ActiveHostChanged);
        }
        self.persist_and_emit(ctx);
    }

    fn persist_and_emit(&self, ctx: &mut ModelContext<Self>) {
        if let Ok(serialized) = serde_json::to_string(&self.hosts) {
            if let Err(err) = ctx
                .private_user_preferences()
                .write_value(SSH_REMOTE_HOSTS_PREF_KEY, serialized)
            {
                log::error!("Failed to persist SSH remote hosts: {err}");
            }
        }
        ctx.emit(SshRemoteModelEvent::HostsChanged);
    }
}

fn read_hosts(ctx: &ModelContext<SshRemoteModel>) -> Vec<SshRemoteHost> {
    ctx.private_user_preferences()
        .read_value(SSH_REMOTE_HOSTS_PREF_KEY)
        .ok()
        .flatten()
        .and_then(|serialized| serde_json::from_str::<Vec<SshRemoteHost>>(&serialized).ok())
        .map(|hosts| hosts.into_iter().map(normalize_host).collect())
        .unwrap_or_default()
}

#[derive(Clone, Debug)]
pub enum SshRemoteViewEvent {
    ConnectHost(String),
    DisconnectHost(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SshRemoteConnectionStatus {
    Idle,
    Connecting,
    Active,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SshRemoteInstallStatus {
    Idle,
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug)]
enum SshRemoteInstallEvent {
    Log(String),
    Finished(Result<(), String>),
}

#[derive(Clone, Debug)]
pub enum SshRemoteViewAction {
    ShowAddWizard,
    EditHost(String),
    CloseWizard,
    PreviousStep,
    NextStep,
    SelectConnectionMethod(SshRemoteConnectionMethod),
    SelectAuthMethod(SshRemoteAuthMethod),
    SelectInstallStrategy(SshRemoteAgentInstallStrategy),
    ToggleResource(SshRemoteResource),
    RequestDeleteHost(String),
    CancelDeleteConfirmation,
    ConfirmPendingDelete,
    ConnectHost(String),
    DisconnectHost(String),
    StartWizardDrag(Vector2F),
    DragWizard(Vector2F),
    EndWizardDrag,
}

#[derive(Clone, Debug)]
enum FormMode {
    Add,
    Edit(String),
}

#[derive(Clone, Debug)]
struct PendingDeleteHost {
    host_id: String,
    name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WizardStep {
    Method,
    Config,
    Resources,
    Install,
}

impl WizardStep {
    fn all() -> &'static [Self] {
        &[Self::Method, Self::Config, Self::Resources, Self::Install]
    }

    fn title(self) -> &'static str {
        match self {
            Self::Method => "Select method",
            Self::Config => "Connection",
            Self::Resources => "Resources",
            Self::Install => "Install",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Method => "Choose how Agentwarp should describe the SSH target.",
            Self::Config => "Fill SSH details and choose how remote resources are downloaded.",
            Self::Resources => "Pick optional agent CLIs. Required runtime packages stay included.",
            Self::Install => "Detect the remote machine and install missing runtime packages.",
        }
    }

    fn index(self) -> usize {
        Self::all()
            .iter()
            .position(|step| *step == self)
            .unwrap_or(0)
    }

    fn next(self) -> Self {
        match self {
            Self::Method => Self::Config,
            Self::Config => Self::Resources,
            Self::Resources => Self::Install,
            Self::Install => Self::Install,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Method => Self::Method,
            Self::Config => Self::Method,
            Self::Resources => Self::Config,
            Self::Install => Self::Resources,
        }
    }
}

#[derive(Clone, Debug)]
pub enum SshRemoteDirectoryPickerEvent {
    Selected(String),
    Cancelled,
}

#[derive(Clone, Debug)]
pub enum SshRemoteDirectoryPickerAction {
    Close,
    Refresh,
    GoUp,
    OpenPath(String),
    SelectPath(String),
    SelectCurrent,
    JumpToEditorPath,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SshRemoteDirectoryPickerStatus {
    Idle,
    Loading,
    Loaded,
    Failed(String),
}

#[derive(Default)]
struct SshRemoteDirectoryPickerMouseStates {
    close_button: MouseStateHandle,
    refresh_button: MouseStateHandle,
    up_button: MouseStateHandle,
    choose_button: MouseStateHandle,
    go_button: MouseStateHandle,
    cancel_button: MouseStateHandle,
    row_states: RefCell<HashMap<String, MouseStateHandle>>,
}

pub struct SshRemoteDirectoryPickerView {
    host: Option<SshRemoteHost>,
    current_path: String,
    entries: Vec<SshRemoteDirectoryEntry>,
    parent_path: Option<String>,
    status: SshRemoteDirectoryPickerStatus,
    path_editor: ViewHandle<EditorView>,
    scroll_state: ClippedScrollStateHandle,
    mouse_states: SshRemoteDirectoryPickerMouseStates,
}

impl SshRemoteDirectoryPickerView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let path_editor = ctx.add_typed_action_view(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let mut editor = EditorView::single_line(
                SingleLineEditorOptions {
                    text: TextOptions::ui_text(Some(12.), appearance),
                    select_all_on_focus: true,
                    clear_selections_on_blur: true,
                    propagate_and_no_op_vertical_navigation_keys:
                        PropagateAndNoOpNavigationKeys::Always,
                    propagate_horizontal_navigation_keys: PropagateHorizontalNavigationKeys::Always,
                    ..Default::default()
                },
                ctx,
            );
            editor.set_placeholder_text("/home/project", ctx);
            editor
        });
        ctx.subscribe_to_view(&path_editor, |me, _, event, ctx| match event {
            EditorEvent::Enter => me.load_editor_path(ctx),
            EditorEvent::Escape => ctx.emit(SshRemoteDirectoryPickerEvent::Cancelled),
            _ => {}
        });

        Self {
            host: None,
            current_path: ".".to_owned(),
            entries: Vec::new(),
            parent_path: None,
            status: SshRemoteDirectoryPickerStatus::Idle,
            path_editor,
            scroll_state: ClippedScrollStateHandle::default(),
            mouse_states: SshRemoteDirectoryPickerMouseStates::default(),
        }
    }

    pub fn open(
        &mut self,
        host: SshRemoteHost,
        initial_path: Option<String>,
        ctx: &mut ViewContext<Self>,
    ) {
        self.host = Some(host);
        self.current_path = initial_path
            .map(|path| path.trim().to_owned())
            .filter(|path| !path.is_empty())
            .unwrap_or_else(|| ".".to_owned());
        self.entries.clear();
        self.parent_path = None;
        self.status = SshRemoteDirectoryPickerStatus::Idle;
        self.set_editor_text(&self.current_path, ctx);
        self.load_path(self.current_path.clone(), ctx);
    }

    fn set_editor_text(&self, text: &str, ctx: &mut ViewContext<Self>) {
        self.path_editor.update(ctx, |editor, ctx| {
            editor.set_buffer_text(text, ctx);
        });
    }

    fn editor_path(&self, ctx: &AppContext) -> String {
        self.path_editor
            .as_ref(ctx)
            .buffer_text(ctx)
            .trim()
            .to_owned()
    }

    fn load_editor_path(&mut self, ctx: &mut ViewContext<Self>) {
        let path = self.editor_path(ctx);
        if !path.is_empty() {
            self.load_path(path, ctx);
        }
    }

    fn load_path(&mut self, path: String, ctx: &mut ViewContext<Self>) {
        let Some(host) = self.host.clone() else {
            return;
        };
        self.current_path = path.clone();
        self.set_editor_text(&path, ctx);
        self.status = SshRemoteDirectoryPickerStatus::Loading;
        self.entries.clear();
        self.parent_path = None;
        ctx.spawn(list_remote_directories(host, path), |view, result, ctx| {
            match result {
                Ok(listing) => {
                    view.current_path = listing.path;
                    view.parent_path = listing.parent_path;
                    view.entries = listing.entries;
                    view.status = SshRemoteDirectoryPickerStatus::Loaded;
                    view.set_editor_text(&view.current_path, ctx);
                }
                Err(error) => {
                    view.status = SshRemoteDirectoryPickerStatus::Failed(error);
                }
            }
            ctx.notify();
        });
        ctx.notify();
    }

    fn mouse_state(&self, key: impl Into<String>) -> MouseStateHandle {
        self.mouse_states
            .row_states
            .borrow_mut()
            .entry(key.into())
            .or_default()
            .clone()
    }

    fn render_icon(icon: Icon, color: impl Into<ThemeFill>, size: f32) -> Box<dyn Element> {
        ConstrainedBox::new(icon.to_warpui_icon(color.into()).finish())
            .with_width(size)
            .with_height(size)
            .finish()
    }

    fn render_icon_button(
        mouse_state: MouseStateHandle,
        icon: Icon,
        tooltip_text: &'static str,
        action: SshRemoteDirectoryPickerAction,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let ui_builder = appearance.ui_builder().clone();
        Hoverable::new(mouse_state, move |state| {
            let icon_color = if state.is_hovered() {
                theme.main_text_color(theme.background())
            } else {
                theme.sub_text_color(theme.background())
            };
            let mut button_container =
                Container::new(Align::new(Self::render_icon(icon, icon_color, 13.)).finish())
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));
            if state.is_hovered() {
                button_container = button_container.with_background(theme.surface_overlay_1());
            }
            let button = ConstrainedBox::new(button_container.finish())
                .with_width(ICON_BUTTON_SIZE)
                .with_height(ICON_BUTTON_SIZE)
                .finish();
            if state.is_hovered() {
                let tooltip = ui_builder
                    .tool_tip(tooltip_text.to_string())
                    .build()
                    .finish();
                let mut stack = Stack::new().with_child(button);
                stack.add_positioned_overlay_child(
                    tooltip,
                    OffsetPositioning::offset_from_parent(
                        vec2f(0., 4.),
                        ParentOffsetBounds::WindowByPosition,
                        ParentAnchor::BottomMiddle,
                        ChildAnchor::TopMiddle,
                    ),
                );
                stack.finish()
            } else {
                button
            }
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(action.clone());
        })
        .finish()
    }

    fn render_text_button(
        mouse_state: MouseStateHandle,
        label: &'static str,
        icon: Option<Icon>,
        action: SshRemoteDirectoryPickerAction,
        is_primary: bool,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        Hoverable::new(mouse_state, move |state| {
            let background = if is_primary {
                ThemeFill::Solid(theme.accent().into())
            } else if state.is_hovered() {
                theme.surface_overlay_2()
            } else {
                theme.surface_overlay_1()
            };
            let text_fill = if is_primary {
                theme.background().into_solid()
            } else {
                theme.main_text_color(theme.background()).into_solid()
            };

            let mut row = Flex::row()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(6.);
            if let Some(icon) = icon {
                row.add_child(Self::render_icon(icon, ThemeFill::Solid(text_fill), 13.));
            }
            row.add_child(
                Text::new_inline(label, appearance.ui_font_family(), 12.)
                    .with_color(text_fill)
                    .with_style(Properties::default().weight(Weight::Medium))
                    .finish(),
            );

            Container::new(row.finish())
                .with_horizontal_padding(10.)
                .with_vertical_padding(6.)
                .with_background(background)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(action.clone());
        })
        .finish()
    }

    fn render_entry(
        &self,
        entry: &SshRemoteDirectoryEntry,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let path = entry.path.clone();
        let mouse_state = self.mouse_state(format!("entry:{}", entry.path));
        let name = entry.name.clone();
        let is_dir = entry.is_dir;
        Hoverable::new(mouse_state, move |state| {
            let mut container = Container::new(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.)
                    .with_child(Self::render_icon(
                        if is_dir { Icon::Folder } else { Icon::FileCopy },
                        theme.sub_text_color(theme.background()),
                        14.,
                    ))
                    .with_child(
                        Shrinkable::new(
                            1.,
                            Text::new_inline(name.clone(), appearance.ui_font_family(), 12.)
                                .with_color(theme.main_text_color(theme.background()).into())
                                .with_clip(ClipConfig::ellipsis())
                                .finish(),
                        )
                        .finish(),
                    )
                    .finish(),
            )
            .with_horizontal_padding(9.)
            .with_vertical_padding(7.)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));
            if state.is_hovered() {
                container = container.with_background(theme.surface_overlay_1());
            }
            container.finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            if is_dir {
                ctx.dispatch_typed_action(SshRemoteDirectoryPickerAction::OpenPath(path.clone()));
            } else {
                ctx.dispatch_typed_action(SshRemoteDirectoryPickerAction::SelectPath(path.clone()));
            }
        })
        .finish()
    }
}

impl Entity for SshRemoteDirectoryPickerView {
    type Event = SshRemoteDirectoryPickerEvent;
}

impl TypedActionView for SshRemoteDirectoryPickerView {
    type Action = SshRemoteDirectoryPickerAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            SshRemoteDirectoryPickerAction::Close => {
                ctx.emit(SshRemoteDirectoryPickerEvent::Cancelled);
            }
            SshRemoteDirectoryPickerAction::Refresh => {
                self.load_path(self.current_path.clone(), ctx);
            }
            SshRemoteDirectoryPickerAction::GoUp => {
                if let Some(parent_path) = self.parent_path.clone() {
                    self.load_path(parent_path, ctx);
                }
            }
            SshRemoteDirectoryPickerAction::OpenPath(path) => {
                self.load_path(path.clone(), ctx);
            }
            SshRemoteDirectoryPickerAction::SelectPath(path) => {
                ctx.emit(SshRemoteDirectoryPickerEvent::Selected(path.clone()));
            }
            SshRemoteDirectoryPickerAction::SelectCurrent => {
                ctx.emit(SshRemoteDirectoryPickerEvent::Selected(
                    self.current_path.clone(),
                ));
            }
            SshRemoteDirectoryPickerAction::JumpToEditorPath => self.load_editor_path(ctx),
        }
    }
}

impl View for SshRemoteDirectoryPickerView {
    fn ui_name() -> &'static str {
        "SshRemoteDirectoryPickerView"
    }

    fn on_focus(&mut self, _focus_ctx: &FocusContext, ctx: &mut ViewContext<Self>) {
        ctx.focus(&self.path_editor);
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let host_name = self
            .host
            .as_ref()
            .map(|host| host.display_name().to_owned())
            .unwrap_or_else(|| "SSH remote".to_owned());

        let status_text = match &self.status {
            SshRemoteDirectoryPickerStatus::Idle => "Browse remote files".to_owned(),
            SshRemoteDirectoryPickerStatus::Loading => "Loading remote files...".to_owned(),
            SshRemoteDirectoryPickerStatus::Loaded => format!("{} items", self.entries.len()),
            SshRemoteDirectoryPickerStatus::Failed(error) => error.clone(),
        };
        let status_color = if matches!(self.status, SshRemoteDirectoryPickerStatus::Failed(_)) {
            ThemeFill::Solid(theme.ui_error_color())
        } else {
            theme.sub_text_color(theme.background())
        };

        let mut entries = Flex::column().with_spacing(2.);
        if matches!(self.status, SshRemoteDirectoryPickerStatus::Loading) {
            entries.add_child(
                Container::new(
                    Text::new_inline("Loading...", appearance.ui_font_family(), 12.)
                        .with_color(theme.sub_text_color(theme.background()).into())
                        .finish(),
                )
                .with_horizontal_padding(9.)
                .with_vertical_padding(7.)
                .finish(),
            );
        } else if self.entries.is_empty() {
            entries.add_child(
                Container::new(
                    Text::new_inline("No child items", appearance.ui_font_family(), 12.)
                        .with_color(theme.disabled_ui_text_color().into())
                        .finish(),
                )
                .with_horizontal_padding(9.)
                .with_vertical_padding(7.)
                .finish(),
            );
        } else {
            for entry in &self.entries {
                entries.add_child(self.render_entry(entry, appearance));
            }
        }

        Container::new(
            ConstrainedBox::new(
                Flex::column()
                    .with_spacing(12.)
                    .with_child(
                        Flex::row()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_child(
                                Flex::column()
                                    .with_spacing(3.)
                                    .with_child(
                                        Text::new_inline(
                                            "Remote file explorer",
                                            appearance.header_font_family(),
                                            16.,
                                        )
                                        .with_color(theme.main_text_color(theme.surface_1()).into())
                                        .with_style(Properties::default().weight(Weight::Semibold))
                                        .finish(),
                                    )
                                    .with_child(
                                        Text::new_inline(
                                            host_name,
                                            appearance.ui_font_family(),
                                            11.5,
                                        )
                                        .with_color(theme.sub_text_color(theme.surface_1()).into())
                                        .finish(),
                                    )
                                    .finish(),
                            )
                            .with_child(Self::render_icon_button(
                                self.mouse_states.close_button.clone(),
                                Icon::X,
                                "Close",
                                SshRemoteDirectoryPickerAction::Close,
                                appearance,
                            ))
                            .finish(),
                    )
                    .with_child(
                        Flex::row()
                            .with_spacing(8.)
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_child(Self::render_icon_button(
                                self.mouse_states.up_button.clone(),
                                Icon::ArrowUp,
                                "Parent directory",
                                SshRemoteDirectoryPickerAction::GoUp,
                                appearance,
                            ))
                            .with_child(
                                Shrinkable::new(
                                    1.,
                                    TextInput::new(
                                        self.path_editor.clone(),
                                        UiComponentStyles::default()
                                            .set_height(30.)
                                            .set_background(theme.surface_2().into())
                                            .set_border_color(theme.surface_3().into())
                                            .set_border_width(1.)
                                            .set_border_radius(CornerRadius::with_all(
                                                Radius::Pixels(4.),
                                            ))
                                            .set_padding(Coords::uniform(8.)),
                                    )
                                    .build()
                                    .finish(),
                                )
                                .finish(),
                            )
                            .with_child(Self::render_text_button(
                                self.mouse_states.go_button.clone(),
                                "Go",
                                Some(Icon::ArrowRight),
                                SshRemoteDirectoryPickerAction::JumpToEditorPath,
                                false,
                                appearance,
                            ))
                            .with_child(Self::render_icon_button(
                                self.mouse_states.refresh_button.clone(),
                                Icon::Refresh,
                                "Refresh",
                                SshRemoteDirectoryPickerAction::Refresh,
                                appearance,
                            ))
                            .finish(),
                    )
                    .with_child(
                        Text::new_inline(status_text, appearance.ui_font_family(), 11.)
                            .with_color(status_color.into())
                            .finish(),
                    )
                    .with_child(
                        ConstrainedBox::new(
                            Container::new(
                                ClippedScrollable::vertical(
                                    self.scroll_state.clone(),
                                    entries.finish(),
                                    ScrollbarWidth::Auto,
                                    theme.nonactive_ui_detail().into(),
                                    theme.active_ui_detail().into(),
                                    ElementFill::None,
                                )
                                .with_overlayed_scrollbar()
                                .finish(),
                            )
                            .with_background(theme.background())
                            .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
                            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                            .with_horizontal_padding(6.)
                            .with_vertical_padding(6.)
                            .finish(),
                        )
                        .with_height(330.)
                        .finish(),
                    )
                    .with_child(
                        Flex::row()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                            .with_child(Self::render_text_button(
                                self.mouse_states.cancel_button.clone(),
                                "Cancel",
                                Some(Icon::X),
                                SshRemoteDirectoryPickerAction::Close,
                                false,
                                appearance,
                            ))
                            .with_child(Self::render_text_button(
                                self.mouse_states.choose_button.clone(),
                                "Choose",
                                Some(Icon::Check),
                                SshRemoteDirectoryPickerAction::SelectCurrent,
                                true,
                                appearance,
                            ))
                            .finish(),
                    )
                    .finish(),
            )
            .with_width(620.)
            .finish(),
        )
        .with_background(theme.surface_1())
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
        .with_drop_shadow(DropShadow::default())
        .with_horizontal_padding(18.)
        .with_vertical_padding(18.)
        .finish()
    }
}

#[derive(Default)]
struct SshRemoteMouseStates {
    add_button: MouseStateHandle,
    empty_add_button: MouseStateHandle,
    close_button: MouseStateHandle,
    next_button: MouseStateHandle,
    back_button: MouseStateHandle,
    save_button: MouseStateHandle,
    wizard_drag_header: MouseStateHandle,
    row_states: RefCell<HashMap<String, MouseStateHandle>>,
    connect_states: RefCell<HashMap<String, MouseStateHandle>>,
    edit_states: RefCell<HashMap<String, MouseStateHandle>>,
    delete_states: RefCell<HashMap<String, MouseStateHandle>>,
    delete_prompt_cancel: MouseStateHandle,
    delete_prompt_delete: MouseStateHandle,
    delete_prompt_close: MouseStateHandle,
    option_states: RefCell<HashMap<String, MouseStateHandle>>,
}

pub struct SshRemoteView {
    window_id: WindowId,
    name_editor: ViewHandle<EditorView>,
    host_editor: ViewHandle<EditorView>,
    user_editor: ViewHandle<EditorView>,
    port_editor: ViewHandle<EditorView>,
    identity_file_editor: ViewHandle<EditorView>,
    password_editor: ViewHandle<EditorView>,
    remote_shell_editor: ViewHandle<EditorView>,
    remote_setup_dir_editor: ViewHandle<EditorView>,
    ssh_config_alias_editor: ViewHandle<EditorView>,
    wizard_mode: Option<FormMode>,
    wizard_step: WizardStep,
    wizard_error: Option<String>,
    connection_method: SshRemoteConnectionMethod,
    auth_method: SshRemoteAuthMethod,
    install_strategy: SshRemoteAgentInstallStrategy,
    selected_resources: Vec<SshRemoteResource>,
    install_status: SshRemoteInstallStatus,
    install_logs: Vec<String>,
    pending_delete_confirmation: Option<PendingDeleteHost>,
    scroll_state: ClippedScrollStateHandle,
    wizard_body_scroll_state: ClippedScrollStateHandle,
    wizard_offset: Vector2F,
    wizard_drag_start_origin: Option<Vector2F>,
    wizard_drag_start_offset: Vector2F,
    mouse_states: SshRemoteMouseStates,
}

impl SshRemoteView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let make_editor = |placeholder: &'static str, ctx: &mut ViewContext<Self>| {
            let editor = ctx.add_typed_action_view(move |ctx| {
                let appearance = Appearance::as_ref(ctx);
                let mut editor = EditorView::single_line(
                    SingleLineEditorOptions {
                        text: TextOptions::ui_text(Some(12.), appearance),
                        select_all_on_focus: true,
                        clear_selections_on_blur: true,
                        propagate_and_no_op_vertical_navigation_keys:
                            PropagateAndNoOpNavigationKeys::Always,
                        propagate_horizontal_navigation_keys:
                            PropagateHorizontalNavigationKeys::Always,
                        ..Default::default()
                    },
                    ctx,
                );
                editor.set_placeholder_text(placeholder, ctx);
                editor
            });
            ctx.subscribe_to_view(&editor, |me, _handle, event, ctx| match event {
                EditorEvent::Enter => me.advance_wizard(ctx),
                EditorEvent::Escape => me.close_wizard(ctx),
                EditorEvent::Edited(_) => {
                    if me.wizard_error.is_some() {
                        me.wizard_error = None;
                        ctx.notify();
                    }
                }
                _ => {}
            });
            editor
        };

        let view = Self {
            window_id: ctx.window_id(),
            name_editor: make_editor("Production", ctx),
            host_editor: make_editor("example.com or 10.0.0.8", ctx),
            user_editor: make_editor("root", ctx),
            port_editor: make_editor("22", ctx),
            identity_file_editor: make_editor("~/.ssh/id_ed25519", ctx),
            password_editor: make_editor("saved SSH password", ctx),
            remote_shell_editor: make_editor("/bin/bash, powershell, or leave blank", ctx),
            remote_setup_dir_editor: make_editor(DEFAULT_REMOTE_SETUP_DIR, ctx),
            ssh_config_alias_editor: make_editor("my-linux-box", ctx),
            wizard_mode: None,
            wizard_step: WizardStep::Method,
            wizard_error: None,
            connection_method: SshRemoteConnectionMethod::Manual,
            auth_method: SshRemoteAuthMethod::PasswordPrompt,
            install_strategy: SshRemoteAgentInstallStrategy::RemoteDownload,
            selected_resources: default_resources(),
            install_status: SshRemoteInstallStatus::Idle,
            install_logs: Vec::new(),
            pending_delete_confirmation: None,
            scroll_state: ClippedScrollStateHandle::default(),
            wizard_body_scroll_state: ClippedScrollStateHandle::default(),
            wizard_offset: Vector2F::zero(),
            wizard_drag_start_origin: None,
            wizard_drag_start_offset: Vector2F::zero(),
            mouse_states: SshRemoteMouseStates::default(),
        };

        ctx.subscribe_to_model(&SshRemoteModel::handle(ctx), |_, _, _, ctx| {
            ctx.notify();
        });

        view
    }

    pub fn focus_first_field(&mut self, _ctx: &mut ViewContext<Self>) {}

    fn reset_wizard_position(&mut self) {
        self.wizard_offset = Vector2F::zero();
        self.wizard_drag_start_origin = None;
        self.wizard_drag_start_offset = Vector2F::zero();
    }

    fn show_add_wizard(&mut self, ctx: &mut ViewContext<Self>) {
        self.pending_delete_confirmation = None;
        self.wizard_mode = Some(FormMode::Add);
        self.wizard_step = WizardStep::Method;
        self.wizard_error = None;
        self.reset_wizard_position();
        self.connection_method = SshRemoteConnectionMethod::Manual;
        self.auth_method = SshRemoteAuthMethod::PasswordPrompt;
        self.install_strategy = SshRemoteAgentInstallStrategy::RemoteDownload;
        self.selected_resources = default_resources();
        self.install_status = SshRemoteInstallStatus::Idle;
        self.install_logs.clear();
        self.set_wizard_values(None, ctx);
        ctx.notify();
    }

    fn edit_host(&mut self, host_id: &str, ctx: &mut ViewContext<Self>) {
        let Some(host) = SshRemoteModel::as_ref(ctx).host(host_id).cloned() else {
            return;
        };
        self.pending_delete_confirmation = None;
        self.wizard_mode = Some(FormMode::Edit(host_id.to_owned()));
        self.wizard_step = WizardStep::Config;
        self.wizard_error = None;
        self.reset_wizard_position();
        self.connection_method = host.connection_method;
        self.auth_method = host.auth_method;
        self.install_strategy = match &host.agent_install_strategy {
            SshRemoteAgentInstallStrategy::Prompt => SshRemoteAgentInstallStrategy::RemoteDownload,
            strategy => strategy.clone(),
        };
        self.selected_resources = host.selected_resources();
        self.install_status = SshRemoteInstallStatus::Idle;
        self.install_logs.clear();
        self.set_wizard_values(Some(&host), ctx);
        ctx.notify();
    }

    fn close_wizard(&mut self, ctx: &mut ViewContext<Self>) {
        if self.wizard_step == WizardStep::Install
            && self.install_status == SshRemoteInstallStatus::Running
        {
            self.wizard_error = Some(
                "Remote setup is still running. Wait for it to finish before closing.".to_owned(),
            );
            ctx.notify();
            return;
        }
        self.wizard_mode = None;
        self.wizard_error = None;
        self.wizard_drag_start_origin = None;
        ctx.notify();
    }

    fn start_wizard_drag(&mut self, origin: Vector2F) {
        self.wizard_drag_start_origin = Some(origin);
        self.wizard_drag_start_offset = self.wizard_offset;
    }

    fn drag_wizard(&mut self, origin: Vector2F, ctx: &mut ViewContext<Self>) {
        let Some(start_origin) = self.wizard_drag_start_origin else {
            self.start_wizard_drag(origin);
            return;
        };
        self.wizard_offset = self.wizard_drag_start_offset + (origin - start_origin);
        ctx.notify();
    }

    fn end_wizard_drag(&mut self) {
        self.wizard_drag_start_origin = None;
    }

    fn wizard_window_centering_offset(&self, app: &AppContext) -> Vector2F {
        let Some(window_bounds) = app.window_bounds(&self.window_id) else {
            return Vector2F::zero();
        };
        let Some(panel_bounds) = app.element_position_by_id_at_last_frame(
            self.window_id,
            super::SSH_REMOTE_PANEL_POSITION_ID,
        ) else {
            return Vector2F::zero();
        };

        vec2f(window_bounds.width() / 2. - panel_bounds.center().x(), 0.)
    }

    fn previous_step(&mut self, ctx: &mut ViewContext<Self>) {
        if self.wizard_step == WizardStep::Install
            && self.install_status == SshRemoteInstallStatus::Running
        {
            return;
        }
        self.wizard_step = self.wizard_step.previous();
        self.wizard_error = None;
        ctx.notify();
    }

    fn advance_wizard(&mut self, ctx: &mut ViewContext<Self>) {
        if self.wizard_mode.is_none() {
            return;
        }
        if !self.validate_step(self.wizard_step, ctx) {
            return;
        }
        if self.wizard_step == WizardStep::Install {
            match self.install_status {
                SshRemoteInstallStatus::Succeeded => self.close_wizard(ctx),
                SshRemoteInstallStatus::Failed | SshRemoteInstallStatus::Idle => {
                    self.start_remote_setup(ctx);
                }
                SshRemoteInstallStatus::Running => {}
            }
        } else if self.wizard_step == WizardStep::Resources {
            self.start_remote_setup(ctx);
        } else {
            self.wizard_step = self.wizard_step.next();
            self.wizard_error = None;
            ctx.notify();
        }
    }

    fn validate_step(&mut self, step: WizardStep, ctx: &mut ViewContext<Self>) -> bool {
        let error = match step {
            WizardStep::Method | WizardStep::Resources | WizardStep::Install => None,
            WizardStep::Config => {
                let setup_dir = self.editor_text(&self.remote_setup_dir_editor, ctx);
                let field_error = if setup_dir.trim().is_empty() {
                    Some("Remote setup directory is required.".to_owned())
                } else if self.connection_method == SshRemoteConnectionMethod::SshConfig {
                    let alias = self.editor_text(&self.ssh_config_alias_editor, ctx);
                    if alias.trim().is_empty() {
                        Some("SSH config alias is required for this connection method.".to_owned())
                    } else {
                        None
                    }
                } else {
                    let host = self.editor_text(&self.host_editor, ctx);
                    if host.trim().is_empty() {
                        Some("Host is required.".to_owned())
                    } else if self.auth_method == SshRemoteAuthMethod::PasswordPrompt
                        && self
                            .editor_text(&self.password_editor, ctx)
                            .trim()
                            .is_empty()
                    {
                        Some("Saved password is required for automatic SSH setup.".to_owned())
                    } else if self.auth_method == SshRemoteAuthMethod::PrivateKey
                        && self
                            .editor_text(&self.identity_file_editor, ctx)
                            .trim()
                            .is_empty()
                    {
                        Some("Identity file is required for private key mode.".to_owned())
                    } else {
                        let port = self.editor_text(&self.port_editor, ctx);
                        if port.trim().is_empty() || port.trim().parse::<u16>().is_ok() {
                            None
                        } else {
                            Some("Port must be a number from 1 to 65535.".to_owned())
                        }
                    }
                };

                field_error.or_else(|| self.validate_wizard_host(ctx))
            }
        };

        if let Some(error) = error {
            self.wizard_error = Some(error);
            ctx.notify();
            false
        } else {
            true
        }
    }

    fn validate_wizard_host(&self, ctx: &AppContext) -> Option<String> {
        let form_mode = self.wizard_mode.clone().unwrap_or(FormMode::Add);
        let host = self.build_host_from_wizard(form_mode, ctx);
        let resolved = match host.resolve_embedded_target() {
            Ok(resolved) => resolved,
            Err(error) => return Some(error),
        };

        #[cfg(not(target_family = "wasm"))]
        if let SshRemoteResolvedAuth::PrivateKey(identity_file) = &resolved.auth {
            if !identity_file.exists() {
                return Some(format!(
                    "Private key file was not found: {}",
                    identity_file.display()
                ));
            }
        }

        for existing in SshRemoteModel::as_ref(ctx).hosts() {
            if existing.id == host.id {
                continue;
            }
            let Ok(existing_target) = existing.resolve_embedded_target() else {
                continue;
            };
            if existing_target.host == resolved.host
                && existing_target.port == resolved.port
                && existing_target.user == resolved.user
            {
                return Some(format!(
                    "Remote host already exists as '{}'.",
                    existing.display_name()
                ));
            }
        }

        None
    }

    fn build_host_from_wizard(&self, form_mode: FormMode, ctx: &AppContext) -> SshRemoteHost {
        let port_value = self.editor_text(&self.port_editor, ctx);
        let port = if port_value.trim().is_empty() {
            None
        } else {
            port_value.trim().parse::<u16>().ok()
        };
        let host_value = self.editor_text(&self.host_editor, ctx);
        let alias_value = self.editor_text(&self.ssh_config_alias_editor, ctx);
        let name_value = self.editor_text(&self.name_editor, ctx);
        let fallback_name = if self.connection_method == SshRemoteConnectionMethod::SshConfig {
            alias_value.clone()
        } else {
            host_value.clone()
        };
        let id = match form_mode {
            FormMode::Add => Uuid::new_v4().to_string(),
            FormMode::Edit(id) => id,
        };
        SshRemoteHost {
            id,
            name: if name_value.trim().is_empty() {
                fallback_name
            } else {
                name_value
            },
            host: host_value,
            user: self.editor_text(&self.user_editor, ctx),
            port,
            identity_file: self.editor_text(&self.identity_file_editor, ctx),
            password: self.editor_text(&self.password_editor, ctx),
            remote_shell: self.editor_text(&self.remote_shell_editor, ctx),
            remote_setup_dir: self.editor_text(&self.remote_setup_dir_editor, ctx),
            agent_install_strategy: self.install_strategy.clone(),
            connection_method: self.connection_method,
            ssh_config_alias: alias_value,
            auth_method: self.auth_method,
            resources: normalize_resources(self.selected_resources.clone()),
        }
    }

    fn start_remote_setup(&mut self, ctx: &mut ViewContext<Self>) {
        if self.install_status == SshRemoteInstallStatus::Running {
            return;
        }
        let Some(form_mode) = self.wizard_mode.clone() else {
            return;
        };
        if !self.validate_step(WizardStep::Config, ctx) {
            self.wizard_step = WizardStep::Config;
            ctx.notify();
            return;
        }

        let host = self.build_host_from_wizard(form_mode, ctx);
        self.wizard_mode = Some(FormMode::Edit(host.id.clone()));
        SshRemoteModel::handle(ctx).update(ctx, |model, ctx| {
            model.upsert_host(host.clone(), ctx);
            model.set_active_host(Some(host.id.clone()), ctx);
        });

        self.wizard_step = WizardStep::Install;
        self.install_status = SshRemoteInstallStatus::Running;
        self.install_logs.clear();
        self.install_logs
            .push("Preparing SSH remote setup...".to_owned());
        self.wizard_error = None;

        let (tx, rx) = async_channel::unbounded::<SshRemoteInstallEvent>();
        ctx.spawn_stream_local(
            rx,
            |me, event, ctx| me.handle_install_event(event, ctx),
            |_, _| {},
        );
        ctx.background_executor()
            .spawn(run_remote_setup(host, tx))
            .detach();
        ctx.notify();
    }

    fn handle_install_event(&mut self, event: SshRemoteInstallEvent, ctx: &mut ViewContext<Self>) {
        match event {
            SshRemoteInstallEvent::Log(line) => {
                self.install_logs.push(line);
            }
            SshRemoteInstallEvent::Finished(Ok(())) => {
                self.install_status = SshRemoteInstallStatus::Succeeded;
                self.install_logs
                    .push("Remote environment is ready.".to_owned());
            }
            SshRemoteInstallEvent::Finished(Err(error)) => {
                self.install_status = SshRemoteInstallStatus::Failed;
                self.wizard_error = Some(error.clone());
                self.install_logs.push(format!("[error] {error}"));
            }
        }
        ctx.notify();
    }

    fn request_delete_host(&mut self, host_id: &str, ctx: &mut ViewContext<Self>) {
        let Some(host) = SshRemoteModel::as_ref(ctx).host(host_id) else {
            return;
        };
        self.pending_delete_confirmation = Some(PendingDeleteHost {
            host_id: host_id.to_owned(),
            name: host.display_name().to_owned(),
        });
        ctx.notify();
    }

    fn cancel_delete_confirmation(&mut self, ctx: &mut ViewContext<Self>) {
        self.pending_delete_confirmation = None;
        ctx.notify();
    }

    fn confirm_pending_delete(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(pending_delete) = self.pending_delete_confirmation.take() else {
            return;
        };
        self.delete_host(&pending_delete.host_id, ctx);
    }

    fn delete_host(&mut self, host_id: &str, ctx: &mut ViewContext<Self>) {
        SshRemoteModel::handle(ctx).update(ctx, |model, ctx| {
            model.delete_host(host_id, ctx);
        });
        if self
            .pending_delete_confirmation
            .as_ref()
            .is_some_and(|pending| pending.host_id == host_id)
        {
            self.pending_delete_confirmation = None;
        }
        if matches!(&self.wizard_mode, Some(FormMode::Edit(editing_id)) if editing_id == host_id) {
            self.wizard_mode = None;
        }
        ctx.notify();
    }

    fn toggle_resource(&mut self, resource: SshRemoteResource, ctx: &mut ViewContext<Self>) {
        if resource.is_required() {
            return;
        }

        if self.selected_resources.contains(&resource) {
            self.selected_resources
                .retain(|selected| *selected != resource);
        } else {
            self.selected_resources.push(resource);
        }
        self.selected_resources = normalize_resources(self.selected_resources.clone());
        ctx.notify();
    }

    fn set_wizard_values(&self, host: Option<&SshRemoteHost>, ctx: &mut ViewContext<Self>) {
        let values = host
            .map(|host| {
                (
                    host.name.as_str(),
                    host.host.as_str(),
                    host.user.as_str(),
                    host.port.map(|port| port.to_string()).unwrap_or_default(),
                    host.identity_file.as_str(),
                    host.password.as_str(),
                    host.remote_shell.as_str(),
                    host.remote_setup_dir.as_str(),
                    host.ssh_config_alias.as_str(),
                )
            })
            .unwrap_or((
                "",
                "",
                "",
                "22".to_owned(),
                "",
                "",
                "",
                DEFAULT_REMOTE_SETUP_DIR,
                "",
            ));

        self.set_editor_text(&self.name_editor, values.0, ctx);
        self.set_editor_text(&self.host_editor, values.1, ctx);
        self.set_editor_text(&self.user_editor, values.2, ctx);
        self.set_editor_text(&self.port_editor, &values.3, ctx);
        self.set_editor_text(&self.identity_file_editor, values.4, ctx);
        self.set_editor_text(&self.password_editor, values.5, ctx);
        self.set_editor_text(&self.remote_shell_editor, values.6, ctx);
        self.set_editor_text(&self.remote_setup_dir_editor, values.7, ctx);
        self.set_editor_text(&self.ssh_config_alias_editor, values.8, ctx);
    }

    fn editor_text(&self, editor: &ViewHandle<EditorView>, ctx: &AppContext) -> String {
        editor.as_ref(ctx).buffer_text(ctx).trim().to_owned()
    }

    fn set_editor_text(
        &self,
        editor: &ViewHandle<EditorView>,
        text: &str,
        ctx: &mut ViewContext<Self>,
    ) {
        editor.update(ctx, |editor, ctx| {
            editor.set_buffer_text(text, ctx);
        });
    }

    fn mouse_state(
        map: &RefCell<HashMap<String, MouseStateHandle>>,
        key: impl Into<String>,
    ) -> MouseStateHandle {
        let key = key.into();
        map.borrow_mut().entry(key).or_default().clone()
    }

    fn render_icon(icon: Icon, color: impl Into<ThemeFill>, size: f32) -> Box<dyn Element> {
        ConstrainedBox::new(icon.to_warpui_icon(color.into()).finish())
            .with_width(size)
            .with_height(size)
            .finish()
    }

    fn render_icon_button(
        mouse_state: MouseStateHandle,
        icon: Icon,
        tooltip_text: &'static str,
        action: SshRemoteViewAction,
        appearance: &Appearance,
        is_danger: bool,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let ui_builder = appearance.ui_builder().clone();

        Hoverable::new(mouse_state, move |state| {
            let icon_color = if is_danger && state.is_hovered() {
                ThemeFill::Solid(theme.ansi_fg_red())
            } else if state.is_hovered() {
                theme.main_text_color(theme.background())
            } else {
                theme.sub_text_color(theme.background())
            };
            let mut button =
                Container::new(Align::new(Self::render_icon(icon, icon_color, 13.)).finish())
                    .with_horizontal_padding(4.)
                    .with_vertical_padding(4.)
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

            if state.is_hovered() {
                button = button.with_background(theme.surface_overlay_1());
            }

            let button = ConstrainedBox::new(button.finish())
                .with_width(ICON_BUTTON_SIZE)
                .with_height(ICON_BUTTON_SIZE)
                .finish();

            if state.is_hovered() {
                let tooltip = ui_builder
                    .tool_tip(tooltip_text.to_string())
                    .build()
                    .finish();
                let mut stack = Stack::new().with_child(button);
                stack.add_positioned_overlay_child(
                    tooltip,
                    OffsetPositioning::offset_from_parent(
                        vec2f(0., 4.),
                        ParentOffsetBounds::WindowByPosition,
                        ParentAnchor::BottomMiddle,
                        ChildAnchor::TopMiddle,
                    ),
                );
                stack.finish()
            } else {
                button
            }
        })
        .with_cursor(Cursor::PointingHand)
        .on_mouse_down(move |ctx, _, _| {
            ctx.dispatch_typed_action(action.clone());
        })
        .finish()
    }

    fn render_text_button(
        mouse_state: MouseStateHandle,
        label: &'static str,
        icon: Option<Icon>,
        action: SshRemoteViewAction,
        is_primary: bool,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();

        Hoverable::new(mouse_state, move |state| {
            let background = if is_primary {
                ThemeFill::Solid(theme.accent().into())
            } else if state.is_hovered() {
                theme.surface_overlay_2()
            } else {
                theme.surface_overlay_1()
            };
            let text_fill = if is_primary {
                theme.background().into_solid()
            } else {
                theme.main_text_color(theme.background()).into_solid()
            };

            let mut row = Flex::row()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(6.);
            if let Some(icon) = icon {
                row.add_child(Self::render_icon(icon, ThemeFill::Solid(text_fill), 13.));
            }
            row.add_child(
                Text::new_inline(label, appearance.ui_font_family(), 12.)
                    .with_color(text_fill)
                    .with_style(Properties::default().weight(Weight::Medium))
                    .finish(),
            );

            Container::new(row.finish())
                .with_horizontal_padding(10.)
                .with_vertical_padding(6.)
                .with_background(background)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_mouse_down(move |ctx, _, _| {
            ctx.dispatch_typed_action(action.clone());
        })
        .finish()
    }

    fn render_danger_text_button(
        mouse_state: MouseStateHandle,
        label: &'static str,
        action: SshRemoteViewAction,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();

        Hoverable::new(mouse_state, move |state| {
            let background = if state.is_hovered() {
                ThemeFill::Solid(theme.ansi_fg_red())
            } else {
                theme.surface_overlay_1()
            };
            let text_fill = if state.is_hovered() {
                theme.background().into_solid()
            } else {
                theme.ui_error_color()
            };

            Container::new(
                Text::new_inline(label, appearance.ui_font_family(), 12.)
                    .with_color(text_fill)
                    .with_style(Properties::default().weight(Weight::Medium))
                    .finish(),
            )
            .with_horizontal_padding(10.)
            .with_vertical_padding(6.)
            .with_background(background)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
            .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_mouse_down(move |ctx, _, _| {
            ctx.dispatch_typed_action(action.clone());
        })
        .finish()
    }

    fn render_disabled_text_button(
        label: &'static str,
        icon: Option<Icon>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let text_fill = theme.sub_text_color(theme.background()).into_solid();

        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.);
        if let Some(icon) = icon {
            row.add_child(Self::render_icon(icon, ThemeFill::Solid(text_fill), 13.));
        }
        row.add_child(
            Text::new_inline(label, appearance.ui_font_family(), 12.)
                .with_color(text_fill)
                .with_style(Properties::default().weight(Weight::Medium))
                .finish(),
        );

        Container::new(row.finish())
            .with_horizontal_padding(10.)
            .with_vertical_padding(6.)
            .with_background(theme.surface_overlay_1())
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
            .finish()
    }

    fn render_input(
        &self,
        label: &'static str,
        editor: &ViewHandle<EditorView>,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        Flex::column()
            .with_spacing(5.)
            .with_child(
                Text::new_inline(label, appearance.ui_font_family(), 11.)
                    .with_color(theme.sub_text_color(theme.background()).into())
                    .with_style(Properties::default().weight(Weight::Medium))
                    .finish(),
            )
            .with_child(
                TextInput::new(
                    editor.clone(),
                    UiComponentStyles::default()
                        .set_height(30.)
                        .set_background(theme.surface_2().into())
                        .set_border_color(theme.surface_3().into())
                        .set_border_width(1.)
                        .set_border_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                        .set_padding(Coords::uniform(8.)),
                )
                .build()
                .finish(),
            )
            .finish()
    }

    fn render_option_card(
        &self,
        key: String,
        icon: Icon,
        title: &'static str,
        description: &'static str,
        selected: bool,
        disabled: bool,
        action: SshRemoteViewAction,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let mouse_state = Self::mouse_state(&self.mouse_states.option_states, key);

        Hoverable::new(mouse_state, move |state| {
            let background = if selected {
                theme.surface_overlay_1()
            } else if state.is_hovered() && !disabled {
                theme.surface_overlay_1()
            } else {
                ThemeFill::Solid(ColorU::transparent_black())
            };
            let icon_fill = if selected {
                theme.accent()
            } else {
                theme.sub_text_color(theme.background())
            };
            let trailing = if selected {
                Self::render_icon(Icon::Check, theme.accent(), 14.)
            } else {
                ConstrainedBox::new(Empty::new().finish())
                    .with_width(14.)
                    .with_height(14.)
                    .finish()
            };

            Container::new(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(10.)
                    .with_child(Self::render_icon(icon, icon_fill, 16.))
                    .with_child(
                        Shrinkable::new(
                            1.,
                            Flex::column()
                                .with_spacing(2.)
                                .with_child(
                                    Text::new_inline(title, appearance.ui_font_family(), 12.)
                                        .with_color(
                                            theme.main_text_color(theme.background()).into(),
                                        )
                                        .with_style(Properties::default().weight(Weight::Semibold))
                                        .finish(),
                                )
                                .with_child(
                                    Text::new_inline(
                                        description,
                                        appearance.ui_font_family(),
                                        10.5,
                                    )
                                    .with_color(theme.sub_text_color(theme.background()).into())
                                    .with_clip(ClipConfig::ellipsis())
                                    .finish(),
                                )
                                .finish(),
                        )
                        .finish(),
                    )
                    .with_child(trailing)
                    .finish(),
            )
            .with_background(background)
            .with_border(Border::bottom(1.).with_border_fill(if selected {
                theme.active_ui_detail()
            } else {
                theme.surface_3()
            }))
            .with_horizontal_padding(8.)
            .with_vertical_padding(8.)
            .finish()
        })
        .with_cursor(Cursor::PointingHand)
        .on_click(move |ctx, _, _| {
            if !disabled {
                ctx.dispatch_typed_action(action.clone());
            }
        })
        .finish()
    }

    fn fixed_width(child: Box<dyn Element>, width: f32) -> Box<dyn Element> {
        ConstrainedBox::new(child).with_width(width).finish()
    }

    fn render_wizard_step_pill(
        &self,
        step: WizardStep,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let is_active = self.wizard_step == step;
        let is_complete = step.index() < self.wizard_step.index();
        let accent = if is_active || is_complete {
            theme.accent()
        } else {
            theme.sub_text_color(theme.background())
        };
        let label_color = if is_active {
            theme.main_text_color(theme.background())
        } else {
            theme.sub_text_color(theme.background())
        };
        let marker = if is_complete {
            Self::render_icon(Icon::Check, accent, 11.)
        } else {
            Container::new(
                Text::new_inline(
                    format!("{}", step.index() + 1),
                    appearance.ui_font_family(),
                    10.5,
                )
                .with_color(accent.into_solid())
                .with_style(Properties::default().weight(Weight::Semibold))
                .finish(),
            )
            .finish()
        };

        let mut container = Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(6.)
                .with_child(
                    ConstrainedBox::new(Container::new(Align::new(marker).finish()).finish())
                        .with_width(20.)
                        .with_height(20.)
                        .finish(),
                )
                .with_child(
                    Shrinkable::new(
                        1.,
                        Text::new_inline(step.title(), appearance.ui_font_family(), 11.)
                            .with_color(label_color.into())
                            .with_style(Properties::default().weight(if is_active {
                                Weight::Semibold
                            } else {
                                Weight::Medium
                            }))
                            .with_clip(ClipConfig::ellipsis())
                            .finish(),
                    )
                    .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(6.)
        .with_vertical_padding(5.)
        .with_border(Border::bottom(1.).with_border_fill(if is_active {
            theme.active_ui_detail()
        } else {
            theme.surface_3()
        }))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(0.)));

        if is_active {
            container = container.with_background(theme.surface_overlay_1());
        } else {
            container = container.with_background(ThemeFill::Solid(ColorU::transparent_black()));
        }

        container.finish()
    }

    fn render_wizard_step_bar(&self, appearance: &Appearance) -> Box<dyn Element> {
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.);
        for step in WizardStep::all() {
            row.add_child(
                Shrinkable::new(1., self.render_wizard_step_pill(*step, appearance)).finish(),
            );
        }
        row.finish()
    }

    fn render_method_step(&self, appearance: &Appearance) -> Box<dyn Element> {
        Flex::column()
            .with_spacing(10.)
            .with_child(self.render_option_card(
                "method:manual".to_owned(),
                SshRemoteConnectionMethod::Manual.icon(),
                SshRemoteConnectionMethod::Manual.label(),
                SshRemoteConnectionMethod::Manual.description(),
                self.connection_method == SshRemoteConnectionMethod::Manual,
                false,
                SshRemoteViewAction::SelectConnectionMethod(SshRemoteConnectionMethod::Manual),
                appearance,
            ))
            .with_child(self.render_option_card(
                "method:ssh-config".to_owned(),
                SshRemoteConnectionMethod::SshConfig.icon(),
                SshRemoteConnectionMethod::SshConfig.label(),
                SshRemoteConnectionMethod::SshConfig.description(),
                self.connection_method == SshRemoteConnectionMethod::SshConfig,
                false,
                SshRemoteViewAction::SelectConnectionMethod(SshRemoteConnectionMethod::SshConfig),
                appearance,
            ))
            .finish()
    }

    fn render_install_strategy_controls(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        Flex::column()
            .with_spacing(8.)
            .with_child(
                Text::new_inline("Resource download method", appearance.ui_font_family(), 11.)
                    .with_color(theme.sub_text_color(theme.background()).into())
                    .with_style(Properties::default().weight(Weight::Medium))
                    .finish(),
            )
            .with_child(
                Flex::column()
                    .with_spacing(2.)
                    .with_child(self.render_option_card(
                        "strategy:upload".to_owned(),
                        SshRemoteAgentInstallStrategy::LocalUpload.icon(),
                        SshRemoteAgentInstallStrategy::LocalUpload.as_label(),
                        SshRemoteAgentInstallStrategy::LocalUpload.description(),
                        self.install_strategy == SshRemoteAgentInstallStrategy::LocalUpload,
                        false,
                        SshRemoteViewAction::SelectInstallStrategy(
                            SshRemoteAgentInstallStrategy::LocalUpload,
                        ),
                        appearance,
                    ))
                    .with_child(self.render_option_card(
                        "strategy:remote".to_owned(),
                        SshRemoteAgentInstallStrategy::RemoteDownload.icon(),
                        SshRemoteAgentInstallStrategy::RemoteDownload.as_label(),
                        SshRemoteAgentInstallStrategy::RemoteDownload.description(),
                        self.install_strategy == SshRemoteAgentInstallStrategy::RemoteDownload,
                        false,
                        SshRemoteViewAction::SelectInstallStrategy(
                            SshRemoteAgentInstallStrategy::RemoteDownload,
                        ),
                        appearance,
                    ))
                    .finish(),
            )
            .with_child(
                Text::new_inline(
                    "Remote download is faster when the server can reach the package CDN; local upload works for restricted networks.",
                    appearance.ui_font_family(),
                    10.5,
                )
                .with_color(theme.sub_text_color(theme.background()).into())
                .finish(),
            )
            .finish()
    }

    fn render_config_step(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let mut content = Flex::column().with_spacing(12.);

        content.add_child(self.render_input("Environment name", &self.name_editor, appearance));
        if self.connection_method == SshRemoteConnectionMethod::SshConfig {
            content.add_child(self.render_input(
                "SSH config alias",
                &self.ssh_config_alias_editor,
                appearance,
            ));
            content.add_child(
                Text::new_inline(
                    "Alias mode reads HostName, User, Port, and IdentityFile from ~/.ssh/config before using the built-in SSH transport.",
                    appearance.ui_font_family(),
                    11.,
                )
                .with_color(theme.sub_text_color(theme.background()).into())
                .finish(),
            );
        } else {
            content.add_child(
                Flex::row()
                    .with_spacing(12.)
                    .with_child(
                        Shrinkable::new(
                            1.,
                            self.render_input("Host", &self.host_editor, appearance),
                        )
                        .finish(),
                    )
                    .with_child(
                        ConstrainedBox::new(self.render_input(
                            "Port",
                            &self.port_editor,
                            appearance,
                        ))
                        .with_width(118.)
                        .finish(),
                    )
                    .finish(),
            );
            content.add_child(self.render_input("User", &self.user_editor, appearance));
        }
        content.add_child(
            Flex::column()
                .with_spacing(7.)
                .with_child(
                    Text::new_inline("Authentication", appearance.ui_font_family(), 11.)
                        .with_color(theme.sub_text_color(theme.background()).into())
                        .with_style(Properties::default().weight(Weight::Medium))
                        .finish(),
                )
                .with_child(
                    Flex::column()
                        .with_spacing(2.)
                        .with_child(self.render_option_card(
                            "auth:password".to_owned(),
                            SshRemoteAuthMethod::PasswordPrompt.icon(),
                            SshRemoteAuthMethod::PasswordPrompt.label(),
                            SshRemoteAuthMethod::PasswordPrompt.description(),
                            self.auth_method == SshRemoteAuthMethod::PasswordPrompt,
                            false,
                            SshRemoteViewAction::SelectAuthMethod(
                                SshRemoteAuthMethod::PasswordPrompt,
                            ),
                            appearance,
                        ))
                        .with_child(self.render_option_card(
                            "auth:key".to_owned(),
                            SshRemoteAuthMethod::PrivateKey.icon(),
                            SshRemoteAuthMethod::PrivateKey.label(),
                            SshRemoteAuthMethod::PrivateKey.description(),
                            self.auth_method == SshRemoteAuthMethod::PrivateKey,
                            false,
                            SshRemoteViewAction::SelectAuthMethod(SshRemoteAuthMethod::PrivateKey),
                            appearance,
                        ))
                        .finish(),
                )
                .finish(),
        );
        if self.auth_method == SshRemoteAuthMethod::PrivateKey {
            content.add_child(self.render_input(
                "Identity file",
                &self.identity_file_editor,
                appearance,
            ));
        } else {
            content.add_child(self.render_input("Password", &self.password_editor, appearance));
        }
        content.add_child(self.render_install_strategy_controls(appearance));
        content.add_child(self.render_input(
            "Remote setup directory",
            &self.remote_setup_dir_editor,
            appearance,
        ));
        content.add_child(self.render_input("Remote shell", &self.remote_shell_editor, appearance));

        content.finish()
    }

    fn render_resources_step(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        let mut resources = Flex::column().with_spacing(8.);
        resources.add_child(
            Text::new_inline("Agent CLIs", appearance.ui_font_family(), 12.)
                .with_color(theme.main_text_color(theme.background()).into())
                .with_style(Properties::default().weight(Weight::Semibold))
                .finish(),
        );
        for resource in SshRemoteResource::selectable() {
            let selected = self.selected_resources.contains(resource);
            resources.add_child(self.render_option_card(
                format!("resource:{resource:?}"),
                resource.icon(),
                resource.label(),
                resource.description(),
                selected,
                resource.is_required(),
                SshRemoteViewAction::ToggleResource(*resource),
                appearance,
            ));
        }

        let mut required_runtime = Flex::column().with_spacing(8.).with_child(
            Text::new_inline("Required runtime", appearance.ui_font_family(), 12.)
                .with_color(theme.main_text_color(theme.background()).into())
                .with_style(Properties::default().weight(Weight::Semibold))
                .finish(),
        );
        for resource in SshRemoteResource::required() {
            required_runtime.add_child(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.)
                    .with_child(Self::render_icon(
                        resource.icon(),
                        theme.sub_text_color(theme.background()),
                        13.,
                    ))
                    .with_child(
                        Text::new_inline(resource.label(), appearance.ui_font_family(), 11.)
                            .with_color(theme.main_text_color(theme.background()).into())
                            .with_clip(ClipConfig::ellipsis())
                            .finish(),
                    )
                    .finish(),
            );
        }
        resources.add_child(
            Container::new(required_runtime.finish())
                .with_background(theme.surface_2())
                .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                .with_horizontal_padding(12.)
                .with_vertical_padding(12.)
                .finish(),
        );

        let mut selected_list = Flex::column().with_spacing(8.).with_child(
            Text::new_inline("Selected packages", appearance.ui_font_family(), 12.)
                .with_color(theme.main_text_color(theme.background()).into())
                .with_style(Properties::default().weight(Weight::Semibold))
                .finish(),
        );
        let selected_resources = normalize_resources(self.selected_resources.clone());
        for resource in &selected_resources {
            selected_list.add_child(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.)
                    .with_child(Self::render_icon(
                        resource.icon(),
                        theme.sub_text_color(theme.background()),
                        13.,
                    ))
                    .with_child(
                        Text::new_inline(resource.label(), appearance.ui_font_family(), 11.)
                            .with_color(theme.main_text_color(theme.background()).into())
                            .with_clip(ClipConfig::ellipsis())
                            .finish(),
                    )
                    .finish(),
            );
        }

        Flex::row()
            .with_spacing(14.)
            .with_child(Shrinkable::new(1., resources.finish()).finish())
            .with_child(
                ConstrainedBox::new(
                    Container::new(selected_list.finish())
                        .with_background(theme.surface_2())
                        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
                        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                        .with_horizontal_padding(12.)
                        .with_vertical_padding(12.)
                        .finish(),
                )
                .with_width(RESOURCE_SUMMARY_WIDTH)
                .finish(),
            )
            .finish()
    }

    fn render_summary_row(
        &self,
        label: &'static str,
        value: String,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(12.)
            .with_child(
                Text::new_inline(label, appearance.ui_font_family(), 11.)
                    .with_color(theme.sub_text_color(theme.background()).into())
                    .finish(),
            )
            .with_child(
                Shrinkable::new(
                    1.,
                    Text::new_inline(value, appearance.ui_font_family(), 11.)
                        .with_color(theme.main_text_color(theme.background()).into())
                        .with_clip(ClipConfig::ellipsis())
                        .finish(),
                )
                .finish(),
            )
            .finish()
    }

    fn render_install_step(&self, appearance: &Appearance, app: &AppContext) -> Box<dyn Element> {
        let theme = appearance.theme();
        let name = self.editor_text(&self.name_editor, app);
        let target = if self.connection_method == SshRemoteConnectionMethod::SshConfig {
            self.editor_text(&self.ssh_config_alias_editor, app)
        } else {
            let user = self.editor_text(&self.user_editor, app);
            let host = self.editor_text(&self.host_editor, app);
            if user.is_empty() {
                host
            } else {
                format!("{user}@{host}")
            }
        };
        let resources = self
            .selected_resources
            .iter()
            .map(|resource| resource.label())
            .collect::<Vec<_>>()
            .join(", ");
        let auth_label = if self.auth_method == SshRemoteAuthMethod::PasswordPrompt
            && !self.editor_text(&self.password_editor, app).is_empty()
        {
            "Password prompt (saved)".to_owned()
        } else {
            self.auth_method.label().to_owned()
        };
        let status_text = match self.install_status {
            SshRemoteInstallStatus::Idle => "Ready to install",
            SshRemoteInstallStatus::Running => "Installing...",
            SshRemoteInstallStatus::Succeeded => "Ready",
            SshRemoteInstallStatus::Failed => "Failed",
        };
        let status_icon = match self.install_status {
            SshRemoteInstallStatus::Succeeded => Icon::Check,
            SshRemoteInstallStatus::Failed => Icon::X,
            SshRemoteInstallStatus::Idle | SshRemoteInstallStatus::Running => Icon::Refresh,
        };
        let mut log_lines = Flex::column().with_spacing(6.);
        for line in &self.install_logs {
            log_lines.add_child(
                Text::new_inline(line.clone(), appearance.monospace_font_family(), 11.)
                    .with_color(theme.main_text_color(theme.background()).into())
                    .finish(),
            );
        }

        Flex::column()
            .with_spacing(12.)
            .with_child(
                Container::new(
                    Flex::row()
                        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(
                            Flex::column()
                                .with_spacing(3.)
                                .with_child(
                                    Text::new_inline(
                                        "Remote setup",
                                        appearance.ui_font_family(),
                                        12.,
                                    )
                                    .with_color(theme.main_text_color(theme.background()).into())
                                    .with_style(Properties::default().weight(Weight::Semibold))
                                    .finish(),
                                )
                                .with_child(
                                    Text::new_inline(
                                        format!(
                                            "{} - {}",
                                            if name.is_empty() {
                                                target.clone()
                                            } else {
                                                name.clone()
                                            },
                                            self.editor_text(&self.remote_setup_dir_editor, app)
                                        ),
                                        appearance.ui_font_family(),
                                        11.,
                                    )
                                    .with_color(theme.sub_text_color(theme.background()).into())
                                    .with_clip(ClipConfig::ellipsis())
                                    .finish(),
                                )
                                .finish(),
                        )
                        .with_child(
                            Flex::row()
                                .with_spacing(7.)
                                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                .with_child(Self::render_icon(status_icon, theme.accent(), 13.))
                                .with_child(
                                    Text::new_inline(status_text, appearance.ui_font_family(), 11.)
                                        .with_color(
                                            theme.main_text_color(theme.background()).into(),
                                        )
                                        .with_style(Properties::default().weight(Weight::Medium))
                                        .finish(),
                                )
                                .finish(),
                        )
                        .finish(),
                )
                .with_background(theme.surface_2())
                .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                .with_horizontal_padding(12.)
                .with_vertical_padding(10.)
                .finish(),
            )
            .with_child(
                Container::new(
                    Flex::column()
                        .with_spacing(10.)
                        .with_child(self.render_summary_row("Target", target, appearance))
                        .with_child(self.render_summary_row(
                            "Authentication",
                            auth_label,
                            appearance,
                        ))
                        .with_child(self.render_summary_row(
                            "Download",
                            self.install_strategy.as_label().to_owned(),
                            appearance,
                        ))
                        .with_child(self.render_summary_row("Resources", resources, appearance))
                        .finish(),
                )
                .with_background(theme.surface_2())
                .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                .with_horizontal_padding(12.)
                .with_vertical_padding(10.)
                .finish(),
            )
            .with_child(
                Container::new(log_lines.finish())
                    .with_background(theme.background())
                    .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                    .with_horizontal_padding(12.)
                    .with_vertical_padding(12.)
                    .finish(),
            )
            .finish()
    }

    fn render_wizard_body(&self, appearance: &Appearance, app: &AppContext) -> Box<dyn Element> {
        match self.wizard_step {
            WizardStep::Method => self.render_method_step(appearance),
            WizardStep::Config => self.render_config_step(appearance),
            WizardStep::Resources => self.render_resources_step(appearance),
            WizardStep::Install => self.render_install_step(appearance, app),
        }
    }

    fn render_wizard_header(
        &self,
        title: &'static str,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let description = self.wizard_step.description();
        let close_button = Self::render_icon_button(
            self.mouse_states.close_button.clone(),
            Icon::X,
            "Close",
            SshRemoteViewAction::CloseWizard,
            appearance,
            false,
        );

        let title_area =
            Hoverable::new(self.mouse_states.wizard_drag_header.clone(), move |state| {
                let mut container = Container::new(
                    Flex::column()
                        .with_spacing(3.)
                        .with_child(
                            Text::new_inline(title, appearance.header_font_family(), 16.)
                                .with_color(theme.main_text_color(theme.surface_1()).into())
                                .with_style(Properties::default().weight(Weight::Semibold))
                                .finish(),
                        )
                        .with_child(
                            Text::new_inline(description, appearance.ui_font_family(), 11.5)
                                .with_color(theme.sub_text_color(theme.surface_1()).into())
                                .finish(),
                        )
                        .finish(),
                )
                .with_horizontal_padding(4.)
                .with_vertical_padding(3.)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

                if state.is_hovered() {
                    container = container.with_background(theme.surface_overlay_1());
                }

                container.finish()
            })
            .with_cursor(Cursor::OpenHand)
            .finish();

        let draggable_title_hit_area = ConstrainedBox::new(
            Container::new(title_area)
                .with_background_color(ColorU::transparent_black())
                .finish(),
        )
        .with_width(WIZARD_BODY_WIDTH - ICON_BUTTON_SIZE - 12.)
        .with_height(WIZARD_HEADER_HEIGHT - 10.)
        .finish();

        let draggable_title = EventHandler::new(draggable_title_hit_area)
            .with_always_handle()
            .on_left_mouse_down(|ctx, _, position| {
                ctx.dispatch_typed_action(SshRemoteViewAction::StartWizardDrag(position));
                DispatchEventResult::StopPropagation
            })
            .on_mouse_dragged(|ctx, _, position| {
                ctx.dispatch_typed_action(SshRemoteViewAction::DragWizard(position));
                DispatchEventResult::StopPropagation
            })
            .on_left_mouse_up(|ctx, _, _| {
                ctx.dispatch_typed_action(SshRemoteViewAction::EndWizardDrag);
                DispatchEventResult::StopPropagation
            })
            .finish();

        ConstrainedBox::new(
            Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(draggable_title)
                    .with_child(close_button)
                    .finish(),
            )
            .with_padding_bottom(10.)
            .finish(),
        )
        .with_height(WIZARD_HEADER_HEIGHT)
        .finish()
    }

    fn render_wizard_modal(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let title = match self.wizard_mode {
            Some(FormMode::Add) => "Add SSH remote",
            Some(FormMode::Edit(_)) => "Edit SSH remote",
            None => "",
        };

        let mut right_panel = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_spacing(12.)
            .with_child(self.render_wizard_header(title, appearance))
            .with_child(self.render_wizard_step_bar(appearance))
            .with_child(
                ConstrainedBox::new(
                    Container::new(
                        ClippedScrollable::vertical(
                            self.wizard_body_scroll_state.clone(),
                            Self::fixed_width(
                                self.render_wizard_body(appearance, app),
                                WIZARD_BODY_CONTENT_WIDTH,
                            ),
                            ScrollbarWidth::Auto,
                            theme.nonactive_ui_detail().into(),
                            theme.active_ui_detail().into(),
                            ElementFill::None,
                        )
                        .with_overlayed_scrollbar()
                        .finish(),
                    )
                    .with_horizontal_padding(WIZARD_BODY_INNER_PADDING)
                    .with_vertical_padding(WIZARD_BODY_INNER_PADDING)
                    .finish(),
                )
                .with_width(WIZARD_BODY_WIDTH)
                .with_height(378.)
                .finish(),
            );

        if let Some(error) = &self.wizard_error {
            right_panel.add_child(
                Text::new_inline(error.clone(), appearance.ui_font_family(), 11.)
                    .with_color(theme.ui_error_color().into())
                    .finish(),
            );
        }

        right_panel.add_child(
            Flex::row()
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_main_axis_size(MainAxisSize::Max)
                .with_child(if self.wizard_step == WizardStep::Method {
                    ConstrainedBox::new(Empty::new().finish())
                        .with_width(1.)
                        .with_height(1.)
                        .finish()
                } else {
                    Self::render_text_button(
                        self.mouse_states.back_button.clone(),
                        "Back",
                        Some(Icon::ArrowLeft),
                        SshRemoteViewAction::PreviousStep,
                        false,
                        appearance,
                    )
                })
                .with_child(match self.wizard_step {
                    WizardStep::Install
                        if self.install_status == SshRemoteInstallStatus::Running =>
                    {
                        Self::render_disabled_text_button(
                            "Installing",
                            Some(Icon::Refresh),
                            appearance,
                        )
                    }
                    WizardStep::Install
                        if self.install_status == SshRemoteInstallStatus::Succeeded =>
                    {
                        Self::render_text_button(
                            self.mouse_states.save_button.clone(),
                            "Done",
                            Some(Icon::Check),
                            SshRemoteViewAction::NextStep,
                            true,
                            appearance,
                        )
                    }
                    WizardStep::Install => Self::render_text_button(
                        self.mouse_states.next_button.clone(),
                        "Retry",
                        Some(Icon::Refresh),
                        SshRemoteViewAction::NextStep,
                        true,
                        appearance,
                    ),
                    WizardStep::Resources => Self::render_text_button(
                        self.mouse_states.next_button.clone(),
                        "Install",
                        Some(Icon::Download),
                        SshRemoteViewAction::NextStep,
                        true,
                        appearance,
                    ),
                    _ => Self::render_text_button(
                        self.mouse_states.next_button.clone(),
                        "Next",
                        Some(Icon::ArrowRight),
                        SshRemoteViewAction::NextStep,
                        true,
                        appearance,
                    ),
                })
                .finish(),
        );

        Container::new(
            ConstrainedBox::new(
                Container::new(right_panel.finish())
                    .with_background(theme.surface_1())
                    .with_horizontal_padding(WIZARD_RIGHT_HORIZONTAL_PADDING)
                    .with_vertical_padding(18.)
                    .finish(),
            )
            .with_width(WIZARD_WIDTH)
            .with_height(WIZARD_HEIGHT)
            .finish(),
        )
        .with_background(theme.surface_1())
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
        .with_drop_shadow(DropShadow::default())
        .finish()
    }

    fn render_host_row(
        &self,
        host: &SshRemoteHost,
        connection_status: SshRemoteConnectionStatus,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let row_key = host.id.clone();
        let host_id = host.id.clone();
        let select_host_id = host.id.clone();
        let row_state = Self::mouse_state(&self.mouse_states.row_states, format!("row-{row_key}"));
        let connect_state = Self::mouse_state(
            &self.mouse_states.connect_states,
            format!("connect-{row_key}"),
        );
        let edit_state =
            Self::mouse_state(&self.mouse_states.edit_states, format!("edit-{row_key}"));
        let delete_state = Self::mouse_state(
            &self.mouse_states.delete_states,
            format!("delete-{row_key}"),
        );

        let name = host.display_name().to_owned();
        let user_host = host.user_host();
        let resource_count = host.selected_resources().len();
        let install_strategy = host.agent_install_strategy.as_label().to_owned();
        let is_active = connection_status == SshRemoteConnectionStatus::Active;
        let is_connecting = connection_status == SshRemoteConnectionStatus::Connecting;
        let is_failed = matches!(connection_status, SshRemoteConnectionStatus::Failed(_));
        let status_detail = match &connection_status {
            SshRemoteConnectionStatus::Connecting => Some("Connecting to remote...".to_owned()),
            SshRemoteConnectionStatus::Failed(message) => {
                Some(format!("Connection failed - {message}"))
            }
            _ => None,
        };
        let icon = if is_active {
            Icon::CloudFilled
        } else {
            Icon::Cloud
        };
        let row_action = if is_active {
            SshRemoteViewAction::DisconnectHost(select_host_id.clone())
        } else {
            SshRemoteViewAction::ConnectHost(select_host_id.clone())
        };

        let mut row = Hoverable::new(row_state, move |state| {
            let mut actions = Flex::row()
                .with_spacing(2.)
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            if state.is_hovered() && !is_connecting {
                let (button_label, button_icon, button_action) = if is_active {
                    (
                        "Disconnect",
                        Some(Icon::X),
                        SshRemoteViewAction::DisconnectHost(host_id.clone()),
                    )
                } else {
                    (
                        if is_failed { "Retry" } else { "Connect" },
                        Some(Icon::Terminal),
                        SshRemoteViewAction::ConnectHost(host_id.clone()),
                    )
                };
                actions.add_child(Self::render_text_button(
                    connect_state.clone(),
                    button_label,
                    button_icon,
                    button_action,
                    false,
                    appearance,
                ));
                actions.add_child(Self::render_icon_button(
                    edit_state.clone(),
                    Icon::Edit,
                    "Edit",
                    SshRemoteViewAction::EditHost(host_id.clone()),
                    appearance,
                    false,
                ));
                actions.add_child(Self::render_icon_button(
                    delete_state.clone(),
                    Icon::Trash,
                    "Delete",
                    SshRemoteViewAction::RequestDeleteHost(host_id.clone()),
                    appearance,
                    true,
                ));
            } else if is_connecting {
                actions.add_child(
                    Text::new_inline("Connecting", appearance.ui_font_family(), 10.5)
                        .with_color(theme.accent().into_solid())
                        .with_style(Properties::default().weight(Weight::Medium))
                        .finish(),
                );
            } else if is_active {
                actions.add_child(
                    Text::new_inline("Active", appearance.ui_font_family(), 10.5)
                        .with_color(theme.accent().into_solid())
                        .with_style(Properties::default().weight(Weight::Medium))
                        .finish(),
                );
            } else if is_failed {
                actions.add_child(
                    Text::new_inline("Failed", appearance.ui_font_family(), 10.5)
                        .with_color(theme.ui_error_color().into())
                        .with_style(Properties::default().weight(Weight::Medium))
                        .finish(),
                );
            } else {
                actions.add_child(
                    ConstrainedBox::new(Empty::new().finish())
                        .with_width(ICON_BUTTON_SIZE)
                        .with_height(ICON_BUTTON_SIZE)
                        .finish(),
                );
            }

            let mut container = Container::new(
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.)
                    .with_child(Self::render_icon(
                        icon,
                        if is_active {
                            theme.accent()
                        } else {
                            theme.sub_text_color(theme.background())
                        },
                        15.,
                    ))
                    .with_child(
                        Shrinkable::new(
                            1.,
                            Flex::column()
                                .with_spacing(2.)
                                .with_child(
                                    Text::new_inline(
                                        name.clone(),
                                        appearance.ui_font_family(),
                                        12.,
                                    )
                                    .with_color(theme.main_text_color(theme.background()).into())
                                    .with_style(Properties::default().weight(Weight::Semibold))
                                    .with_clip(ClipConfig::ellipsis())
                                    .finish(),
                                )
                                .with_child(
                                    Text::new_inline(
                                        status_detail.clone().unwrap_or_else(|| {
                                            format!(
                                                "{} - {} resources - {}",
                                                user_host, resource_count, install_strategy
                                            )
                                        }),
                                        appearance.ui_font_family(),
                                        10.5,
                                    )
                                    .with_color(theme.sub_text_color(theme.background()).into())
                                    .with_clip(ClipConfig::ellipsis())
                                    .finish(),
                                )
                                .finish(),
                        )
                        .finish(),
                    )
                    .with_child(actions.finish())
                    .finish(),
            )
            .with_horizontal_padding(8.)
            .with_vertical_padding(7.)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));

            if is_active {
                container = container
                    .with_background(theme.surface_overlay_1())
                    .with_border(Border::all(1.).with_border_fill(theme.active_ui_detail()));
            } else if state.is_hovered() {
                container = container.with_background(theme.surface_overlay_1());
            }

            container.finish()
        })
        .with_defer_events_to_children();

        if !is_connecting {
            row = row
                .with_cursor(Cursor::PointingHand)
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(row_action.clone());
                });
        }

        row.finish()
    }

    fn render_delete_confirmation_prompt(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let pending_delete = self.pending_delete_confirmation.as_ref()?;
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let body = format!(
            "\"{}\" will be removed from SSH remotes.",
            pending_delete.name
        );

        let prompt = Container::new(
            Flex::column()
                .with_spacing(10.)
                .with_child(
                    Flex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(
                            Text::new_inline(
                                "Delete SSH remote?",
                                appearance.ui_font_family(),
                                12.,
                            )
                            .with_color(theme.main_text_color(theme.background()).into())
                            .with_style(Properties::default().weight(Weight::Semibold))
                            .finish(),
                        )
                        .with_child(Self::render_icon_button(
                            self.mouse_states.delete_prompt_close.clone(),
                            Icon::X,
                            "Close",
                            SshRemoteViewAction::CancelDeleteConfirmation,
                            appearance,
                            false,
                        ))
                        .finish(),
                )
                .with_child(
                    Text::new(body, appearance.ui_font_family(), 11.)
                        .with_color(theme.sub_text_color(theme.background()).into_solid())
                        .finish(),
                )
                .with_child(
                    Flex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_main_axis_alignment(MainAxisAlignment::End)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_spacing(6.)
                        .with_child(Self::render_text_button(
                            self.mouse_states.delete_prompt_cancel.clone(),
                            "Cancel",
                            None,
                            SshRemoteViewAction::CancelDeleteConfirmation,
                            false,
                            appearance,
                        ))
                        .with_child(Self::render_danger_text_button(
                            self.mouse_states.delete_prompt_delete.clone(),
                            "Delete",
                            SshRemoteViewAction::ConfirmPendingDelete,
                            appearance,
                        ))
                        .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(12.)
        .with_vertical_padding(12.)
        .with_background(theme.surface_2())
        .with_border(Border::all(1.).with_border_fill(theme.surface_3()))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
        .finish();

        Some(
            ConstrainedBox::new(prompt)
                .with_width(DELETE_CONFIRMATION_PROMPT_WIDTH)
                .finish(),
        )
    }

    fn render_empty_state(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        Container::new(
            Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(10.)
                .with_child(Self::render_icon(
                    Icon::Cloud,
                    theme.sub_text_color(theme.background()),
                    20.,
                ))
                .with_child(
                    Text::new_inline("No SSH remotes", appearance.ui_font_family(), 12.)
                        .with_color(theme.main_text_color(theme.background()).into())
                        .with_style(Properties::default().weight(Weight::Semibold))
                        .finish(),
                )
                .with_child(
                    Text::new_inline(
                        "Add a machine to launch terminals and agent sessions over SSH.",
                        appearance.ui_font_family(),
                        11.,
                    )
                    .with_color(theme.sub_text_color(theme.background()).into())
                    .finish(),
                )
                .with_child(Self::render_text_button(
                    self.mouse_states.empty_add_button.clone(),
                    "Add remote",
                    Some(Icon::Plus),
                    SshRemoteViewAction::ShowAddWizard,
                    false,
                    appearance,
                ))
                .finish(),
        )
        .with_horizontal_padding(12.)
        .with_vertical_padding(18.)
        .finish()
    }
}

impl Entity for SshRemoteView {
    type Event = SshRemoteViewEvent;
}

impl TypedActionView for SshRemoteView {
    type Action = SshRemoteViewAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            SshRemoteViewAction::ShowAddWizard => self.show_add_wizard(ctx),
            SshRemoteViewAction::EditHost(host_id) => self.edit_host(host_id, ctx),
            SshRemoteViewAction::CloseWizard => self.close_wizard(ctx),
            SshRemoteViewAction::PreviousStep => self.previous_step(ctx),
            SshRemoteViewAction::NextStep => self.advance_wizard(ctx),
            SshRemoteViewAction::SelectConnectionMethod(method) => {
                self.connection_method = *method;
                self.wizard_error = None;
                ctx.notify();
            }
            SshRemoteViewAction::SelectAuthMethod(method) => {
                self.auth_method = *method;
                ctx.notify();
            }
            SshRemoteViewAction::SelectInstallStrategy(strategy) => {
                self.install_strategy = strategy.clone();
                ctx.notify();
            }
            SshRemoteViewAction::ToggleResource(resource) => self.toggle_resource(*resource, ctx),
            SshRemoteViewAction::RequestDeleteHost(host_id) => {
                self.request_delete_host(host_id, ctx);
            }
            SshRemoteViewAction::CancelDeleteConfirmation => self.cancel_delete_confirmation(ctx),
            SshRemoteViewAction::ConfirmPendingDelete => self.confirm_pending_delete(ctx),
            SshRemoteViewAction::ConnectHost(host_id) => {
                self.pending_delete_confirmation = None;
                ctx.emit(SshRemoteViewEvent::ConnectHost(host_id.clone()));
            }
            SshRemoteViewAction::DisconnectHost(host_id) => {
                self.pending_delete_confirmation = None;
                ctx.emit(SshRemoteViewEvent::DisconnectHost(host_id.clone()));
            }
            SshRemoteViewAction::StartWizardDrag(origin) => self.start_wizard_drag(*origin),
            SshRemoteViewAction::DragWizard(origin) => self.drag_wizard(*origin, ctx),
            SshRemoteViewAction::EndWizardDrag => self.end_wizard_drag(),
        }
    }
}

impl View for SshRemoteView {
    fn ui_name() -> &'static str {
        "SshRemoteView"
    }

    fn on_focus(&mut self, _focus_ctx: &FocusContext, _ctx: &mut ViewContext<Self>) {}

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let model = SshRemoteModel::as_ref(app);
        let hosts = model.hosts().to_vec();

        let mut content = Flex::column().with_spacing(8.);
        content.add_child(
            Container::new(
                Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_child(
                        Text::new_inline("SSH remotes", appearance.ui_font_family(), 12.)
                            .with_color(theme.sub_text_color(theme.background()).into())
                            .with_style(Properties::default().weight(Weight::Medium))
                            .finish(),
                    )
                    .with_child(Self::render_icon_button(
                        self.mouse_states.add_button.clone(),
                        Icon::Plus,
                        "Add remote",
                        SshRemoteViewAction::ShowAddWizard,
                        appearance,
                        false,
                    ))
                    .finish(),
            )
            .with_horizontal_padding(SIDEBAR_HORIZONTAL_PADDING)
            .with_padding_top(8.)
            .with_padding_bottom(4.)
            .finish(),
        );

        if hosts.is_empty() {
            content.add_child(self.render_empty_state(appearance));
        } else {
            for host in hosts {
                let connection_status = model.connection_status(&host.id);
                content.add_child(
                    Container::new(self.render_host_row(&host, connection_status, appearance))
                        .with_horizontal_padding(SIDEBAR_HORIZONTAL_PADDING)
                        .finish(),
                );
            }
        }

        let panel = ClippedScrollable::vertical(
            self.scroll_state.clone(),
            content.finish(),
            ScrollbarWidth::Auto,
            theme.nonactive_ui_detail().into(),
            theme.active_ui_detail().into(),
            ElementFill::None,
        )
        .with_overlayed_scrollbar()
        .finish();

        let delete_prompt = self.render_delete_confirmation_prompt(app);

        if self.wizard_mode.is_none() {
            if let Some(prompt) = delete_prompt {
                let mut stack = Stack::new().with_child(panel);
                stack.add_positioned_child(
                    prompt,
                    OffsetPositioning::offset_from_parent(
                        vec2f(
                            SIDEBAR_HORIZONTAL_PADDING,
                            DELETE_CONFIRMATION_PROMPT_OFFSET,
                        ),
                        ParentOffsetBounds::ParentByPosition,
                        ParentAnchor::TopLeft,
                        ChildAnchor::TopLeft,
                    ),
                );
                return stack.finish();
            }
            return panel;
        }

        let mut stack = Stack::new().with_child(panel);
        let scrim = ConstrainedBox::new(
            Container::new(Empty::new().finish())
                .with_background_color(ColorU::new(0, 0, 0, 150))
                .finish(),
        )
        .with_width(10000.)
        .with_height(10000.)
        .finish();
        stack.add_positioned_overlay_child(
            scrim,
            OffsetPositioning::offset_from_parent(
                vec2f(0., 0.),
                ParentOffsetBounds::WindowByPosition,
                ParentAnchor::Center,
                ChildAnchor::Center,
            ),
        );
        stack.add_positioned_overlay_child(
            self.render_wizard_modal(app),
            OffsetPositioning::offset_from_parent(
                self.wizard_window_centering_offset(app) + self.wizard_offset,
                ParentOffsetBounds::WindowByPosition,
                ParentAnchor::Center,
                ChildAnchor::Center,
            ),
        );
        stack.finish()
    }
}
