use std::env;
use std::fs::OpenOptions;
use std::io::{self, Read as _, Write as _};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use signal_hook::consts::signal::{SIGINT, SIGQUIT};
#[cfg(unix)]
use signal_hook::iterator::Signals;

const WARP_BOOTSTRAP_DONE_MARKER: &[u8] = b"eval \"$WARP_BOOTSTRAP_VAR\"; unset WARP_BOOTSTRAP_VAR";
const WARP_PASTE_PREFIX_MARKER: &[u8] = b"\x1bi";
const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";

#[derive(Debug)]
enum Auth {
    Password(String),
    PrivateKey(PathBuf),
}

#[derive(Debug)]
struct Config {
    host: String,
    port: u16,
    user: String,
    auth: Auth,
    setup_dir: Option<String>,
    cwd: Option<String>,
}

fn env_required(name: &str) -> io::Result<String> {
    env::var(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, format!("{name} is required")))
}

fn expand_tilde_path(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn config_from_env() -> io::Result<Config> {
    let host = env_required("AGENTWARP_SSH_HOST")?;
    let port = env::var("AGENTWARP_SSH_PORT")
        .ok()
        .and_then(|port| port.parse::<u16>().ok())
        .unwrap_or(22);
    let user = env::var("AGENTWARP_SSH_USER")
        .ok()
        .filter(|user| !user.trim().is_empty())
        .or_else(|| env::var("USER").ok())
        .or_else(|| env::var("USERNAME").ok())
        .unwrap_or_else(|| "root".to_owned());

    let auth = match env::var("AGENTWARP_SSH_AUTH").as_deref() {
        Ok("password") => Auth::Password(env_required("AGENTWARP_SSH_PASSWORD")?),
        Ok("private_key") => Auth::PrivateKey(expand_tilde_path(&env_required(
            "AGENTWARP_SSH_IDENTITY_FILE",
        )?)),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "AGENTWARP_SSH_AUTH must be password or private_key",
            ));
        }
    };

    let setup_dir = env::var("AGENTWARP_SSH_REMOTE_SETUP_DIR")
        .ok()
        .map(|dir| dir.trim().to_owned())
        .filter(|dir| !dir.is_empty());
    let cwd = env::var("AGENTWARP_SSH_REMOTE_CWD")
        .ok()
        .map(|dir| dir.trim().to_owned())
        .filter(|dir| !dir.is_empty());

    Ok(Config {
        host,
        port,
        user,
        auth,
        setup_dir,
        cwd,
    })
}

fn ssh_error(message: impl AsRef<str>, error: impl std::fmt::Display) -> io::Error {
    io::Error::new(
        io::ErrorKind::Other,
        format!("{}: {error}", message.as_ref()),
    )
}

fn is_would_block(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::WouldBlock
        || error.raw_os_error() == Some(libc::EAGAIN)
        || error.raw_os_error() == Some(libc::EWOULDBLOCK)
}

fn debug_log_line(line: impl AsRef<str>) {
    let Ok(path) = env::var("AGENTWARP_SSH_DEBUG_LOG") else {
        return;
    };
    let Ok(mut log) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let _ = writeln!(log, "{}", line.as_ref());
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

fn remote_forwarded_env_exports() -> String {
    let Ok(raw_env) = env::var("AGENTWARP_SSH_REMOTE_ENV_JSON") else {
        return String::new();
    };
    let Ok(env_vars) = serde_json::from_str::<std::collections::HashMap<String, String>>(&raw_env)
    else {
        debug_log_line("failed to parse AGENTWARP_SSH_REMOTE_ENV_JSON");
        return String::new();
    };

    let mut exports = String::new();
    for (key, value) in env_vars {
        if key
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
            && !key.is_empty()
        {
            exports.push_str("export ");
            exports.push_str(&key);
            exports.push('=');
            exports.push_str(&shell_quote(&value));
            exports.push_str("; ");
        }
    }
    exports
}

fn remote_claude_settings_sync_command() -> &'static str {
    "if command -v node >/dev/null 2>&1 && { [ -n \"${ANTHROPIC_AUTH_TOKEN:-}\" ] || [ -n \"${ANTHROPIC_API_KEY:-}\" ] || [ -n \"${ANTHROPIC_BASE_URL:-}\" ]; }; then node -e 'const fs=require(\"fs\"),os=require(\"os\"),path=require(\"path\");const keys=[\"ANTHROPIC_AUTH_TOKEN\",\"ANTHROPIC_API_KEY\",\"ANTHROPIC_BASE_URL\",\"ANTHROPIC_MODEL\",\"ANTHROPIC_DEFAULT_HAIKU_MODEL\",\"ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME\",\"ANTHROPIC_DEFAULT_SONNET_MODEL\",\"ANTHROPIC_DEFAULT_SONNET_MODEL_NAME\",\"ANTHROPIC_DEFAULT_OPUS_MODEL\",\"ANTHROPIC_DEFAULT_OPUS_MODEL_NAME\"];const dir=path.join(os.homedir(),\".claude\");const file=path.join(dir,\"settings.json\");let settings={};try{settings=JSON.parse(fs.readFileSync(file,\"utf8\"));}catch{}if(!settings||typeof settings!==\"object\"||Array.isArray(settings))settings={};settings.skipIntroduction=true;settings.skipDangerousModePermissionPrompt=true;const existingEnv=settings.env&&typeof settings.env===\"object\"&&!Array.isArray(settings.env)?settings.env:{};const nextEnv={...existingEnv};for(const key of keys){if(process.env[key])nextEnv[key]=process.env[key];}if(Object.keys(nextEnv).length)settings.env=nextEnv;fs.mkdirSync(dir,{recursive:true});fs.writeFileSync(file,JSON.stringify(settings,null,2)+\"\\n\");' >/dev/null 2>&1 || true; fi; "
}

fn remote_codex_settings_sync_command(project_cwd: Option<&str>) -> String {
    let project_cwd_env = project_cwd
        .map(|cwd| format!("CODEX_PROJECT_CWD={} ", shell_quote(cwd.trim())))
        .unwrap_or_default();

    format!(
        r#"if command -v node >/dev/null 2>&1 && {{ [ -n "${{OPENAI_API_KEY:-}}" ] || [ -n "${{OPENAI_BASE_URL:-}}" ] || [ -n "${{OPENAI_MODEL:-}}" ]; }}; then {project_cwd_env}node <<'AGENTWARP_CODEX_SETTINGS' >/dev/null 2>&1 || true
const fs = require("fs");
const os = require("os");
const path = require("path");
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
const apiKey = (process.env.OPENAI_API_KEY || "").trim();
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
const baseUrl = (process.env.OPENAI_BASE_URL || "").trim();
if (baseUrl) config = upsertTop(config, "openai_base_url", tomlString(baseUrl));
config = upsertTop(config, "check_for_update_on_startup", "false");
const model = (process.env.OPENAI_MODEL || "").trim();
if (model && model !== "default") {{
  config = upsertTop(config, "model", tomlString(model));
  config = upsertSection(config, "[notice.model_migrations]", tomlString(model), tomlString("gpt-5.4"));
}}
const cwd = (process.env.CODEX_PROJECT_CWD || "").trim();
if (cwd) {{
  config = upsertSection(config, `[projects.${{tomlString(cwd)}}]`, "trust_level", tomlString("trusted"));
}}
fs.writeFileSync(configPath, config);
AGENTWARP_CODEX_SETTINGS
fi; "#
    )
}

fn configured_remote_shell() -> Option<String> {
    let shell = env::var("AGENTWARP_SSH_REMOTE_SHELL").ok()?;
    let shell = shell.trim();
    (!shell.is_empty()).then(|| shell.to_owned())
}

fn parse_echo_shell_word(input: &str) -> Option<String> {
    #[derive(Copy, Clone, PartialEq, Eq)]
    enum QuoteState {
        Unquoted,
        Single,
        Double,
    }

    let mut state = QuoteState::Unquoted;
    let mut output = String::new();
    let mut chars = input.trim_start().chars().peekable();
    while let Some(ch) = chars.next() {
        match state {
            QuoteState::Unquoted => match ch {
                '\'' => state = QuoteState::Single,
                '"' => state = QuoteState::Double,
                ')' => return Some(output),
                ch if ch.is_whitespace() && !output.is_empty() => {
                    while matches!(chars.peek(), Some(next) if next.is_whitespace()) {
                        chars.next();
                    }
                    return matches!(chars.next(), Some(')')).then_some(output);
                }
                ch => output.push(ch),
            },
            QuoteState::Single => {
                if ch == '\'' {
                    state = QuoteState::Unquoted;
                } else {
                    output.push(ch);
                }
            }
            QuoteState::Double => match ch {
                '"' => state = QuoteState::Unquoted,
                '\\' => {
                    if let Some(next) = chars.next() {
                        output.push(next);
                    }
                }
                ch => output.push(ch),
            },
        }
    }
    (state == QuoteState::Unquoted && !output.is_empty()).then_some(output)
}

fn warp_bash_rcfile_script_from_args() -> Option<String> {
    let command = env::args().nth(2)?;
    let marker = "--rcfile <(echo ";
    let start = command.find(marker)? + marker.len();
    parse_echo_shell_word(&command[start..])
}

fn remote_safe_bash_rcfile_script(script: &str) -> String {
    script.to_owned()
}

fn remote_join(dir: &str, file_name: &str) -> String {
    format!("{}/{}", dir.trim_end_matches('/'), file_name)
}

fn remote_shell_command(config: &Config, remote_rcfile_path: Option<&str>) -> Option<String> {
    let has_warp_shell_args = env::args().nth(1).is_some();
    let shell =
        configured_remote_shell().or_else(|| has_warp_shell_args.then(|| "bash".to_owned()));
    let Some(shell) = shell else {
        return None;
    };
    let lower = shell.to_ascii_lowercase();
    let remote_env_exports = remote_forwarded_env_exports();
    let launch = if let Some(remote_rcfile_path) = remote_rcfile_path {
        format!(
            "exec -a bash {} --rcfile {}",
            shell_quote(&shell),
            shell_quote(remote_rcfile_path)
        )
    } else if lower.contains("powershell") || lower.contains("pwsh") {
        format!("exec {} -NoLogo", shell_quote(&shell))
    } else {
        format!("exec {} -l", shell_quote(&shell))
    };
    Some(match &config.setup_dir {
        Some(setup_dir) if !lower.contains("powershell") && !lower.contains("pwsh") => {
            let setup_dir = setup_dir.trim();
            let cwd = config.cwd.as_deref().unwrap_or(setup_dir).trim();
            let quoted_setup_dir = shell_quote(setup_dir);
            let quoted_cwd = shell_quote(cwd);
            let env_file = shell_quote(remote_join(setup_dir, "agentwarp-env.sh").as_str());
            let claude_settings_sync = remote_claude_settings_sync_command();
            let codex_settings_sync = remote_codex_settings_sync_command(config.cwd.as_deref());
            format!(
                "export AGENTWARP_REMOTE_ROOT={quoted_setup_dir}; \
                 {remote_env_exports}\
                 export PATH={quoted_setup_dir}/bin:{quoted_setup_dir}/node/bin:{quoted_setup_dir}/npm-global/bin:$PATH; \
                 export npm_config_prefix={quoted_setup_dir}/npm-global; \
                 export NPM_CONFIG_PREFIX={quoted_setup_dir}/npm-global; \
                 {claude_settings_sync}\
                 {codex_settings_sync}\
                 [ -r {env_file} ] && . {env_file}; \
                 cd {quoted_cwd} 2>/dev/null || cd {quoted_setup_dir} 2>/dev/null || cd; {launch}"
            )
        }
        _ => launch,
    })
}

fn json_escape(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            _ => out.push(ch),
        }
    }
    out
}

fn shell_name() -> &'static str {
    let shell = env::var("AGENTWARP_SSH_REMOTE_SHELL")
        .unwrap_or_else(|_| "bash".to_owned())
        .to_ascii_lowercase();
    if shell.contains("zsh") {
        "zsh"
    } else if shell.contains("fish") {
        "fish"
    } else if shell.contains("pwsh") || shell.contains("powershell") {
        "powershell"
    } else {
        "bash"
    }
}

fn emit_dcs_payload(payload: &str) -> io::Result<()> {
    let hex = payload
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut stdout = io::stdout();
    write!(stdout, "\x1bP$d{hex}\x1b\\")?;
    stdout.flush()
}

fn new_warp_session_id() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn emit_init_shell(config: &Config, session_id: u64) -> io::Result<()> {
    let payload = format!(
        r#"{{"hook":"InitShell","value":{{"session_id":{session_id},"shell":"{}","user":"{}","hostname":"{}"}}}}"#,
        json_escape(shell_name()),
        json_escape(&config.user),
        json_escape(&config.host),
    );
    emit_dcs_payload(&payload)
}

fn emit_precmd(config: &Config, session_id: u64) -> io::Result<()> {
    let pwd = config.setup_dir.as_deref().unwrap_or("/");
    let payload = format!(
        r#"{{"hook":"Precmd","value":{{"pwd":"{}","ps1":"$ ","ps1_is_encoded":false,"honor_ps1":false,"git_head":"","git_branch":"","virtual_env":"","conda_env":"","session_id":{session_id}}}}}"#,
        json_escape(pwd),
    );
    emit_dcs_payload(&payload)
}

fn emit_bootstrapped() -> io::Result<()> {
    let shell = json_escape(shell_name());
    // The normal Warp rcfile emits this after shell bootstrap. The embedded SSH
    // bridge cannot safely forward that rcfile through another shell quoting layer,
    // so it marks the session ready before handing control to the remote shell.
    let payload = format!(
        r#"{{"hook":"Bootstrapped","value":{{"histfile":"","shell":"{shell}","home_dir":"","path":"","cdpath":"","editor":"","aliases":"","abbreviations":"","function_names":"","env_var_names":"","builtins":"","keywords":"","shell_version":"","shell_options":"","rcfiles_start_time":"","rcfiles_end_time":"","shell_plugins":"","vi_mode_enabled":"","os_category":"Linux","linux_distribution":"","wsl_name":"","shell_path":""}}}}"#
    );
    emit_dcs_payload(&payload)
}

fn emit_warp_bootstrap(config: &Config) -> io::Result<()> {
    if env::args().nth(1).is_none() {
        return Ok(());
    }
    let session_id = new_warp_session_id();
    emit_init_shell(config, session_id)?;
    emit_bootstrapped()?;
    emit_precmd(config, session_id)
}

struct StdinGuard {
    original_flags: Option<i32>,
    #[cfg(unix)]
    original_termios: Option<libc::termios>,
}

impl StdinGuard {
    fn install() -> Self {
        let original_flags = unsafe {
            let flags = libc::fcntl(libc::STDIN_FILENO, libc::F_GETFL, 0);
            if flags >= 0 {
                let _ = libc::fcntl(libc::STDIN_FILENO, libc::F_SETFL, flags | libc::O_NONBLOCK);
                Some(flags)
            } else {
                None
            }
        };

        #[cfg(unix)]
        let original_termios = install_raw_stdin_mode();

        Self {
            original_flags,
            #[cfg(unix)]
            original_termios,
        }
    }
}

impl Drop for StdinGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(original_termios) = self.original_termios.as_ref() {
            unsafe {
                let _ = libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, original_termios);
            }
        }

        if let Some(original_flags) = self.original_flags {
            unsafe {
                let _ = libc::fcntl(libc::STDIN_FILENO, libc::F_SETFL, original_flags);
            }
        }
    }
}

#[cfg(unix)]
fn install_raw_stdin_mode() -> Option<libc::termios> {
    unsafe {
        if libc::isatty(libc::STDIN_FILENO) != 1 {
            return None;
        }

        let mut original = std::mem::zeroed::<libc::termios>();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut original) != 0 {
            return None;
        }

        let mut raw = original;
        raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        raw.c_oflag &= !libc::OPOST;
        raw.c_cflag |= libc::CS8;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;

        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) != 0 {
            return None;
        }

        Some(original)
    }
}

#[cfg(unix)]
fn setup_signal_forwarding() -> Option<mpsc::Receiver<u8>> {
    let mut signals = Signals::new([SIGINT, SIGQUIT]).ok()?;
    let (sender, receiver) = mpsc::channel();

    std::thread::Builder::new()
        .name("agentwarp-ssh-signal-forwarder".to_owned())
        .spawn(move || {
            for signal in signals.forever() {
                let byte = match signal {
                    SIGINT => 0x03,
                    SIGQUIT => 0x1c,
                    _ => continue,
                };
                if sender.send(byte).is_err() {
                    break;
                }
            }
        })
        .ok()?;

    Some(receiver)
}

#[cfg(not(unix))]
fn setup_signal_forwarding() -> Option<mpsc::Receiver<u8>> {
    None
}

fn connect(config: &Config) -> io::Result<ssh2::Session> {
    let mut addresses = (config.host.as_str(), config.port).to_socket_addrs()?;
    let address = addresses.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            format!("could not resolve {}:{}", config.host, config.port),
        )
    })?;
    let tcp = TcpStream::connect_timeout(&address, Duration::from_secs(20))?;
    tcp.set_nodelay(true).ok();

    let mut session =
        ssh2::Session::new().map_err(|err| ssh_error("failed to create ssh session", err))?;
    session.set_tcp_stream(tcp);
    session
        .handshake()
        .map_err(|err| ssh_error("ssh handshake failed", err))?;
    match &config.auth {
        Auth::Password(password) => session
            .userauth_password(&config.user, password)
            .map_err(|err| ssh_error("ssh password authentication failed", err))?,
        Auth::PrivateKey(path) => session
            .userauth_pubkey_file(&config.user, None, Path::new(path), None)
            .map_err(|err| ssh_error("ssh private key authentication failed", err))?,
    }
    if !session.authenticated() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "ssh authentication did not complete",
        ));
    }
    Ok(session)
}

fn run_remote_command(session: &ssh2::Session, command: &str) -> io::Result<()> {
    let mut channel = session
        .channel_session()
        .map_err(|err| ssh_error("failed to open ssh command channel", err))?;
    channel
        .exec(command)
        .map_err(|err| ssh_error("failed to run remote command", err))?;
    let mut stdout = String::new();
    let mut stderr = String::new();
    let _ = channel.read_to_string(&mut stdout);
    let _ = channel.stderr().read_to_string(&mut stderr);
    channel
        .wait_close()
        .map_err(|err| ssh_error("failed to close remote command channel", err))?;
    let status = channel
        .exit_status()
        .map_err(|err| ssh_error("failed to read remote command exit status", err))?;
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "remote command exited with status {status}: {}",
                stderr.trim()
            ),
        ))
    }
}

fn upload_remote_bash_rcfile(
    session: &ssh2::Session,
    config: &Config,
    script: &str,
) -> io::Result<String> {
    let setup_dir = config.setup_dir.as_deref().unwrap_or("/tmp/agentwarp");
    run_remote_command(
        session,
        &format!(
            "mkdir -p {} && chmod 700 {}",
            shell_quote(setup_dir),
            shell_quote(setup_dir)
        ),
    )?;

    let remote_path = remote_join(setup_dir, "agentwarp-bash-rcfile.sh");
    let sftp = session
        .sftp()
        .map_err(|err| ssh_error("failed to open ssh sftp session", err))?;
    let mut file = sftp
        .create(Path::new(&remote_path))
        .map_err(|err| ssh_error("failed to create remote bash rcfile", err))?;
    let script = remote_safe_bash_rcfile_script(script);
    file.write_all(script.as_bytes())?;
    file.flush()?;
    drop(file);

    run_remote_command(
        session,
        &format!("chmod 600 {}", shell_quote(remote_path.as_str())),
    )?;
    debug_log_line(format!(
        "uploaded remote bash rcfile to {} ({} bytes)",
        remote_path,
        script.len()
    ));
    Ok(remote_path)
}

fn upload_remote_text(
    session: &ssh2::Session,
    config: &Config,
    file_name: &str,
    contents: &[u8],
) -> io::Result<String> {
    let setup_dir = config.setup_dir.as_deref().unwrap_or("/tmp/agentwarp");
    let remote_path = remote_join(setup_dir, file_name);
    session.set_blocking(true);
    let result = (|| {
        let sftp = session
            .sftp()
            .map_err(|err| ssh_error("failed to open ssh sftp session", err))?;
        let mut file = sftp
            .create(Path::new(&remote_path))
            .map_err(|err| ssh_error("failed to create remote file", err))?;
        file.write_all(contents)?;
        file.flush()?;
        Ok(remote_path)
    })();
    session.set_blocking(false);
    result
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn terminal_size() -> (u32, u32, u32, u32) {
    unsafe {
        let mut size: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) == 0
            && size.ws_col > 0
            && size.ws_row > 0
        {
            return (
                size.ws_col as u32,
                size.ws_row as u32,
                size.ws_xpixel as u32,
                size.ws_ypixel as u32,
            );
        }
    }
    (80, 24, 0, 0)
}

fn write_all_to_channel(channel: &mut ssh2::Channel, bytes: &[u8]) -> io::Result<()> {
    let mut written = 0;
    while written < bytes.len() {
        match channel.write(&bytes[written..]) {
            Ok(0) => break,
            Ok(n) => written += n,
            Err(err) if is_would_block(&err) => std::thread::sleep(Duration::from_millis(5)),
            Err(err) => return Err(ssh_error("failed to write to ssh pty", err)),
        }
    }
    Ok(())
}

fn write_all_to_stdout(stdout: &mut io::Stdout, bytes: &[u8]) -> io::Result<()> {
    let mut written = 0;
    while written < bytes.len() {
        match stdout.write(&bytes[written..]) {
            Ok(0) => break,
            Ok(n) => written += n,
            Err(err) if is_would_block(&err) => std::thread::sleep(Duration::from_millis(5)),
            Err(err) => return Err(err),
        }
    }
    loop {
        match stdout.flush() {
            Ok(()) => return Ok(()),
            Err(err) if is_would_block(&err) => std::thread::sleep(Duration::from_millis(5)),
            Err(err) => return Err(err),
        }
    }
}

#[derive(Default)]
struct TerminalInputSanitizer {
    pending: Vec<u8>,
}

impl TerminalInputSanitizer {
    fn sanitize(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut input = Vec::with_capacity(self.pending.len() + bytes.len());
        input.append(&mut self.pending);
        input.extend_from_slice(bytes);

        let mut output = Vec::with_capacity(input.len());
        let mut index = 0;
        while index < input.len() {
            if starts_with_at(&input, index, WARP_PASTE_PREFIX_MARKER) {
                let after_prefix = &input[index + WARP_PASTE_PREFIX_MARKER.len()..];
                if is_paste_wrapper_after_prefix(after_prefix) {
                    index += WARP_PASTE_PREFIX_MARKER.len();
                    continue;
                }
            }
            if starts_with_at(&input, index, BRACKETED_PASTE_START) {
                index += BRACKETED_PASTE_START.len();
                continue;
            }
            if starts_with_at(&input, index, BRACKETED_PASTE_END) {
                index += BRACKETED_PASTE_END.len();
                continue;
            }
            if input[index] == 0x10 {
                let after_dle = &input[index + 1..];
                if starts_with_sequence(after_dle, WARP_PASTE_PREFIX_MARKER)
                    && is_paste_wrapper_after_prefix(&after_dle[WARP_PASTE_PREFIX_MARKER.len()..])
                {
                    index += 1;
                    continue;
                }
                if starts_with_sequence(after_dle, BRACKETED_PASTE_START) {
                    index += 1;
                    continue;
                }
                if might_be_dle_paste_sequence(after_dle) {
                    self.pending.extend_from_slice(&input[index..]);
                    break;
                }
            }
            if might_be_partial_bracketed_paste_sequence(&input[index..]) {
                self.pending.extend_from_slice(&input[index..]);
                break;
            }

            output.push(input[index]);
            index += 1;
        }

        output
    }

    #[cfg(test)]
    fn finish(mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }
}

fn starts_with_at(bytes: &[u8], index: usize, needle: &[u8]) -> bool {
    bytes
        .get(index..index.saturating_add(needle.len()))
        .is_some_and(|slice| slice == needle)
}

fn starts_with_sequence(bytes: &[u8], sequence: &[u8]) -> bool {
    bytes
        .get(..sequence.len())
        .is_some_and(|slice| slice == sequence)
}

fn is_paste_wrapper_after_prefix(bytes: &[u8]) -> bool {
    starts_with_sequence(bytes, BRACKETED_PASTE_START)
        || might_be_partial_bracketed_paste_sequence(bytes)
        || bytes.first() == Some(&0x10) && {
            let after_dle = &bytes[1..];
            after_dle.is_empty()
                || starts_with_sequence(after_dle, BRACKETED_PASTE_START)
                || might_be_partial_bracketed_paste_sequence(after_dle)
        }
}

fn might_be_dle_paste_sequence(bytes: &[u8]) -> bool {
    bytes.is_empty()
        || bytes.len() <= WARP_PASTE_PREFIX_MARKER.len()
            && WARP_PASTE_PREFIX_MARKER.starts_with(bytes)
        || might_be_partial_bracketed_paste_sequence(bytes)
}

fn might_be_partial_bracketed_paste_sequence(bytes: &[u8]) -> bool {
    bytes.len() >= 2
        && ((bytes.len() < BRACKETED_PASTE_START.len() && BRACKETED_PASTE_START.starts_with(bytes))
            || (bytes.len() < BRACKETED_PASTE_END.len() && BRACKETED_PASTE_END.starts_with(bytes)))
}

fn bridge(
    config: &Config,
    session: &ssh2::Session,
    mut channel: ssh2::Channel,
    capture_initial_bootstrap: bool,
) -> io::Result<i32> {
    let _stdin_guard = StdinGuard::install();
    let signal_rx = setup_signal_forwarding();
    let mut stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut debug_log = env::var("AGENTWARP_SSH_DEBUG_LOG")
        .ok()
        .and_then(|path| OpenOptions::new().create(true).append(true).open(path).ok());
    let mut input = [0_u8; 8192];
    let mut output = [0_u8; 8192];
    let mut last_resize = Instant::now();
    let mut last_size = terminal_size();
    let mut capture_initial_bootstrap = capture_initial_bootstrap;
    let mut bootstrap_buffer = Vec::new();
    let mut input_sanitizer = TerminalInputSanitizer::default();

    loop {
        let mut progressed = false;

        if let Some(signal_rx) = signal_rx.as_ref() {
            while let Ok(byte) = signal_rx.try_recv() {
                progressed = true;
                write_all_to_channel(&mut channel, &[byte])?;
            }
        }

        match stdin.read(&mut input) {
            Ok(0) => {}
            Ok(count) => {
                progressed = true;
                if let Some(log) = debug_log.as_mut() {
                    let _ = writeln!(
                        log,
                        "\n>>> stdin {} bytes\n{}",
                        count,
                        String::from_utf8_lossy(&input[..count])
                    );
                }

                if capture_initial_bootstrap {
                    bootstrap_buffer.extend_from_slice(&input[..count]);
                    if find_bytes(&bootstrap_buffer, WARP_BOOTSTRAP_DONE_MARKER).is_some() {
                        let remote_bootstrap_path = upload_remote_text(
                            session,
                            config,
                            "agentwarp-bash-bootstrap.sh",
                            &bootstrap_buffer,
                        )?;
                        debug_log_line(format!(
                            "captured and uploaded warp bash bootstrap to {} ({} bytes)",
                            remote_bootstrap_path,
                            bootstrap_buffer.len()
                        ));
                        let source_command =
                            format!(" source {}\n", shell_quote(remote_bootstrap_path.as_str()));
                        write_all_to_channel(&mut channel, source_command.as_bytes())?;
                        bootstrap_buffer.clear();
                        capture_initial_bootstrap = false;
                    }
                    continue;
                }

                let sanitized = input_sanitizer.sanitize(&input[..count]);
                if !sanitized.is_empty() {
                    write_all_to_channel(&mut channel, &sanitized)?;
                }
            }
            Err(err) if is_would_block(&err) => {}
            Err(err) => return Err(err),
        }

        loop {
            match channel.read(&mut output) {
                Ok(0) => break,
                Ok(count) => {
                    progressed = true;
                    if let Some(log) = debug_log.as_mut() {
                        let _ = writeln!(
                            log,
                            "\n<<< stdout {} bytes\n{}",
                            count,
                            String::from_utf8_lossy(&output[..count])
                        );
                    }
                    write_all_to_stdout(&mut stdout, &output[..count])?;
                }
                Err(err) if is_would_block(&err) => break,
                Err(err) => return Err(ssh_error("failed to read from ssh pty", err)),
            }
        }

        if last_resize.elapsed() >= Duration::from_millis(250) {
            last_resize = Instant::now();
            let size = terminal_size();
            if size != last_size {
                last_size = size;
                let _ = channel.request_pty_size(size.0, size.1, Some(size.2), Some(size.3));
            }
        }

        if channel.eof() {
            debug_log_line("ssh pty channel reached EOF");
            break;
        }
        if !progressed {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    session.set_blocking(true);
    channel
        .wait_close()
        .map_err(|err| ssh_error("failed to close ssh pty", err))?;
    let exit_status = channel
        .exit_status()
        .map_err(|err| ssh_error("failed to read ssh exit status", err))?;
    debug_log_line(format!("ssh pty channel exit status: {exit_status}"));
    Ok(exit_status)
}

fn run() -> io::Result<i32> {
    let config = config_from_env()?;
    let args = env::args().collect::<Vec<_>>();
    debug_log_line(format!(
        "agentwarp-ssh-pty start: args_count={} arg1={:?} arg2_prefix={:?} setup_dir={:?}",
        args.len(),
        args.get(1),
        args.get(2)
            .map(|arg| arg.chars().take(240).collect::<String>()),
        config.setup_dir
    ));
    let session = connect(&config)?;
    debug_log_line(format!(
        "authenticated embedded ssh session to {}@{}:{}",
        config.user, config.host, config.port
    ));
    let rcfile_script = warp_bash_rcfile_script_from_args().filter(|_| shell_name() == "bash");
    debug_log_line(format!(
        "warp bash rcfile extracted: {}",
        rcfile_script
            .as_ref()
            .map(|script| script.len().to_string())
            .unwrap_or_else(|| "no".to_owned())
    ));
    let remote_rcfile_path = rcfile_script
        .map(|script| upload_remote_bash_rcfile(&session, &config, &script))
        .transpose()?;
    let mut channel = session
        .channel_session()
        .map_err(|err| ssh_error("failed to open ssh channel", err))?;
    let (cols, rows, width_px, height_px) = terminal_size();
    channel
        .request_pty(
            "xterm-256color",
            None,
            Some((cols, rows, width_px, height_px)),
        )
        .map_err(|err| ssh_error("failed to request ssh pty", err))?;
    if let Some(command) = remote_shell_command(&config, remote_rcfile_path.as_deref()) {
        debug_log_line(format!("starting remote shell command: {command}"));
        channel
            .exec(&command)
            .map_err(|err| ssh_error("failed to start configured remote shell", err))?;
    } else {
        channel
            .shell()
            .map_err(|err| ssh_error("failed to start remote shell", err))?;
    }
    if remote_rcfile_path.is_none() {
        emit_warp_bootstrap(&config)?;
    }
    session.set_blocking(false);
    bridge(&config, &session, channel, remote_rcfile_path.is_some())
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            debug_log_line(format!("agentwarp embedded ssh pty failed: {error}"));
            eprintln!("agentwarp embedded ssh pty failed: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalInputSanitizer;

    #[test]
    fn sanitizer_strips_warp_paste_wrappers() {
        let mut sanitizer = TerminalInputSanitizer::default();

        assert_eq!(
            sanitizer.sanitize(b"\x1bi\x10\x1b[200~ls\x1b[201~\r"),
            b"ls\r"
        );
        assert!(sanitizer.finish().is_empty());
    }

    #[test]
    fn sanitizer_handles_split_bracketed_paste_sequences() {
        let mut sanitizer = TerminalInputSanitizer::default();

        assert_eq!(sanitizer.sanitize(b"\x1bi\x10\x1b[20"), b"");
        assert_eq!(sanitizer.sanitize(b"0~pwd\x1b[201"), b"pwd");
        assert_eq!(sanitizer.sanitize(b"~\n"), b"\n");
        assert!(sanitizer.finish().is_empty());
    }

    #[test]
    fn sanitizer_strips_dle_before_warp_paste_prefix() {
        let mut sanitizer = TerminalInputSanitizer::default();

        assert_eq!(
            sanitizer.sanitize(b"\x10\x1bi\x1b[200~echo ok; exit\x1b[201~\n"),
            b"echo ok; exit\n"
        );
        assert!(sanitizer.finish().is_empty());
    }

    #[test]
    fn sanitizer_preserves_alt_i_without_paste_wrapper() {
        let mut sanitizer = TerminalInputSanitizer::default();

        assert_eq!(sanitizer.sanitize(b"\x1bi"), b"\x1bi");
        assert!(sanitizer.finish().is_empty());
    }

    #[test]
    fn sanitizer_preserves_escape_key() {
        let mut sanitizer = TerminalInputSanitizer::default();

        assert_eq!(sanitizer.sanitize(b"\x1b"), b"\x1b");
        assert!(sanitizer.finish().is_empty());
    }

    #[test]
    fn sanitizer_preserves_split_arrow_sequence() {
        let mut sanitizer = TerminalInputSanitizer::default();

        assert_eq!(sanitizer.sanitize(b"\x1b["), b"");
        assert_eq!(sanitizer.sanitize(b"A"), b"\x1b[A");
        assert!(sanitizer.finish().is_empty());
    }

    #[test]
    fn sanitizer_preserves_alt_i_followed_by_arrow_sequence() {
        let mut sanitizer = TerminalInputSanitizer::default();

        assert_eq!(sanitizer.sanitize(b"\x1bi\x1b[A"), b"\x1bi\x1b[A");
        assert!(sanitizer.finish().is_empty());
    }
}
