use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context as _};
use dunce::canonicalize;
use futures_util::{SinkExt, StreamExt};
use html5ever::tendril::TendrilSink;
use html5ever::{parse_document, Attribute};
use itertools::Itertools;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use pathfinder_color::ColorU;
use pathfinder_geometry::vector::Vector2F;
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use serde_json::Value;
use url::Url;
use warp_core::features::FeatureFlag;
use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::Icon;
use warp_util::path::LineAndColumnArg;
use warpui::clipboard::ClipboardContent;
use warpui::elements::{
    resizable_state_handle, Align, Border, ChildAnchor, ChildView, Clipped,
    ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, DragBarSide, Element, EventContext, Flex, Hoverable, LayoutContext,
    LiveElement, MainAxisAlignment, MainAxisSize, MouseStateHandle, PaintContext, ParentElement,
    Point, PositionedElementAnchor, Radius, Resizable, ResizableStateHandle, ScrollbarWidth,
    Shrinkable, Text,
};
use warpui::event::DispatchedEvent;
use warpui::fonts::{Properties, Weight};
use warpui::keymap::EditableBinding;
use warpui::platform::Cursor;
use warpui::r#async::Timer;
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::{
    AfterLayoutContext, AppContext, Entity, EntityId, ModelHandle, SingletonEntity, SizeConstraint,
    TypedActionView, View, ViewContext, ViewHandle, WeakViewHandle, WindowId,
};
use websocket::{Message, WebSocket, WebsocketMessage as _};

use crate::ai::agent::AgentReviewCommentBatch;
use crate::appearance::{Appearance, AppearanceEvent};
use crate::code::buffer_location::LocalOrRemotePath;
#[cfg(feature = "local_fs")]
use crate::code::file_tree::{FileTreeEvent, FileTreeView};
use crate::code_review::code_review_header::HEADER_BUTTON_PADDING;
#[cfg(feature = "local_fs")]
use crate::code_review::code_review_view::CodeReviewAction;
use crate::code_review::code_review_view::{
    render_file_navigation_button, CodeReviewCommentDebugState, CodeReviewView,
    CodeReviewViewEvent, CONTENT_LEFT_MARGIN, CONTENT_RIGHT_MARGIN,
};
use crate::code_review::diff_state::DiffStateModel;
use crate::code_review::telemetry_event::CodeReviewContextDestination;
use crate::drive::panel::{MAX_SIDEBAR_WIDTH_RATIO, MIN_SIDEBAR_WIDTH};
use crate::editor::{EditorOptions, EditorView, Event as EditorEvent, TextOptions};
use crate::pane_group::pane::view::header::components::HEADER_EDGE_PADDING;
use crate::pane_group::pane::view::header::PANE_HEADER_HEIGHT;
use crate::pane_group::{
    Event as PaneGroupEvent, PaneGroup, WorkingDirectoriesEvent, WorkingDirectoriesModel,
};
use crate::settings::{AISettings, AISettingsChangedEvent};
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::terminal::input::MenuPositioning;
use crate::terminal::resizable_data::{ModalType, ResizableData};
use crate::terminal::view::TerminalView;
use crate::terminal::CLIAgent;
use crate::ui_components::buttons::icon_button_with_color;
use crate::ui_components::icons;
use crate::util::bindings::{keybinding_name_to_display_string, CustomAction};
#[cfg(feature = "local_fs")]
use crate::util::openable_file_type::FileTarget;
use crate::util::path::{display_name_with_host, display_path_with_host};
use crate::view_components::action_button::{ActionButton, PaneHeaderTheme};
#[cfg(feature = "local_fs")]
use crate::view_components::action_button::{NakedTheme, TooltipAlignment};
use crate::view_components::{Dropdown, DropdownItem};
use crate::workspace::view::TOGGLE_RIGHT_PANEL_BINDING_NAME;
use crate::workspace::WorkspaceAction;

const BROWSER_MAX_ELEMENT_CANDIDATES: usize = 80;
const BROWSER_MAX_ELEMENT_TEXT_CHARS: usize = 500;
const BROWSER_MAX_CONTEXT_CHARS: usize = 4_000;
const BROWSER_HOST_REPAINT_INTERVAL: Duration = Duration::from_millis(250);
const BROWSER_ELEMENT_PICKER_POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BrowserHostRect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[cfg(all(target_os = "linux", not(target_family = "wasm")))]
struct ExternalBrowserHost {
    child: Option<Child>,
    window_id: Option<String>,
    reparented_to: Option<String>,
    current_url: Option<String>,
    class_name: String,
    profile_dir: PathBuf,
    cdp_port: u16,
    last_rect: Option<BrowserHostRect>,
    last_sync_at: Option<Instant>,
    decorations_disabled: bool,
    hidden: bool,
}

#[cfg(all(target_os = "linux", not(target_family = "wasm")))]
impl ExternalBrowserHost {
    fn new() -> Self {
        let process_id = std::process::id();
        Self {
            child: None,
            window_id: None,
            reparented_to: None,
            current_url: None,
            class_name: format!("WarpRightPanelBrowser{process_id}"),
            profile_dir: std::env::temp_dir()
                .join(format!("warp-right-panel-browser-{process_id}")),
            cdp_port: 40_000 + (process_id % 20_000) as u16,
            last_rect: None,
            last_sync_at: None,
            decorations_disabled: false,
            hidden: false,
        }
    }

    fn show(&mut self, url: &str, parent_window_id: Option<String>, rect: BrowserHostRect) {
        if rect.width < 8 || rect.height < 8 {
            self.hide();
            return;
        }

        self.ensure_launched(url);

        let should_sync = self.last_rect != Some(rect)
            || self.hidden
            || self
                .last_sync_at
                .is_none_or(|last_sync| last_sync.elapsed() >= BROWSER_HOST_REPAINT_INTERVAL);
        if !should_sync {
            return;
        }

        if self.window_id.is_none() {
            self.window_id = self.find_window_id();
        }

        let Some(window_id) = self.window_id.clone() else {
            return;
        };

        if !self.decorations_disabled {
            let _ = Command::new("xprop")
                .args([
                    "-id",
                    &window_id,
                    "-f",
                    "_MOTIF_WM_HINTS",
                    "32c",
                    "-set",
                    "_MOTIF_WM_HINTS",
                    "0x2, 0x0, 0x0, 0x0, 0x0",
                ])
                .status();
            self.decorations_disabled = true;
        }

        if let Some(parent_window_id) = parent_window_id {
            if self.reparented_to.as_deref() != Some(parent_window_id.as_str()) {
                let _ = Command::new("xdotool")
                    .args(["windowreparent", &window_id, &parent_window_id])
                    .status();
                self.reparented_to = Some(parent_window_id);
                self.last_rect = None;
            }
        }

        let _ = Command::new("xdotool")
            .args([
                "windowmap",
                &window_id,
                "windowmove",
                &window_id,
                &rect.x.to_string(),
                &rect.y.to_string(),
                "windowsize",
                &window_id,
                &rect.width.to_string(),
                &rect.height.to_string(),
                "windowraise",
                &window_id,
            ])
            .status();

        self.hidden = false;
        self.last_rect = Some(rect);
        self.last_sync_at = Some(Instant::now());
    }

    fn hide(&mut self) {
        if self.hidden {
            return;
        }

        if self.window_id.is_none() {
            self.window_id = self.find_window_id();
        }

        if let Some(window_id) = &self.window_id {
            let _ = Command::new("xdotool")
                .args(["windowunmap", window_id])
                .status();
        }

        self.hidden = true;
        self.last_rect = None;
    }

    fn terminate(&mut self) {
        if self.window_id.is_none() {
            self.window_id = self.find_window_id();
        }

        if let Some(window_id) = &self.window_id {
            let _ = Command::new("xdotool")
                .args(["windowclose", window_id])
                .status();
        }

        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }

        self.child = None;
        self.window_id = None;
        self.reparented_to = None;
        self.current_url = None;
        self.last_rect = None;
        self.last_sync_at = None;
        self.decorations_disabled = false;
        self.hidden = true;
    }

    fn ensure_launched(&mut self, url: &str) {
        let child_exited = self
            .child
            .as_mut()
            .is_some_and(|child| child.try_wait().ok().flatten().is_some());

        if child_exited {
            self.child = None;
            self.window_id = None;
            self.reparented_to = None;
            self.current_url = None;
            self.decorations_disabled = false;
        }

        if self.child.is_some() && self.current_url.as_deref() == Some(url) {
            return;
        }

        if self.child.is_some() {
            self.terminate();
        }

        let _ = std::fs::create_dir_all(&self.profile_dir);
        let user_data_dir = format!("--user-data-dir={}", self.profile_dir.display());
        let class_arg = format!("--class={}", self.class_name);
        let app_arg = format!("--app={url}");
        let remote_debugging_address = "--remote-debugging-address=127.0.0.1";
        let remote_debugging_port = format!("--remote-debugging-port={}", self.cdp_port);
        match Command::new("chromium")
            .args([
                user_data_dir.as_str(),
                remote_debugging_address,
                remote_debugging_port.as_str(),
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-default-apps",
                "--disable-session-crashed-bubble",
                "--disable-infobars",
                class_arg.as_str(),
                app_arg.as_str(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => {
                self.child = Some(child);
                self.current_url = Some(url.to_string());
                self.window_id = None;
                self.reparented_to = None;
                self.decorations_disabled = false;
                self.hidden = false;
                self.last_sync_at = None;
            }
            Err(err) => {
                log::warn!("failed to launch right-panel browser host: {err}");
                self.child = None;
                self.current_url = None;
            }
        }
    }

    fn find_window_id(&self) -> Option<String> {
        let output = Command::new("xdotool")
            .args(["search", "--onlyvisible", "--class", &self.class_name])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.trim().is_empty())
            .last()
            .map(|line| line.trim().to_string())
    }
}

#[cfg(all(target_os = "linux", not(target_family = "wasm")))]
impl Drop for ExternalBrowserHost {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(all(target_os = "linux", not(target_family = "wasm")))]
fn external_browser_host() -> &'static Mutex<ExternalBrowserHost> {
    static HOST: OnceLock<Mutex<ExternalBrowserHost>> = OnceLock::new();
    HOST.get_or_init(|| Mutex::new(ExternalBrowserHost::new()))
}

fn show_external_browser_host(url: &str, parent_window_id: Option<String>, rect: BrowserHostRect) {
    #[cfg(all(target_os = "linux", not(target_family = "wasm")))]
    if let Ok(mut host) = external_browser_host().lock() {
        host.show(url, parent_window_id, rect);
    }

    #[cfg(not(all(target_os = "linux", not(target_family = "wasm"))))]
    let _ = (url, parent_window_id, rect);
}

fn hide_external_browser_host() {
    #[cfg(all(target_os = "linux", not(target_family = "wasm")))]
    if let Ok(mut host) = external_browser_host().lock() {
        host.hide();
    }
}

fn external_browser_debugging_port() -> Option<u16> {
    #[cfg(all(target_os = "linux", not(target_family = "wasm")))]
    if let Ok(host) = external_browser_host().lock() {
        return host.child.as_ref().map(|_| host.cdp_port);
    }

    None
}

#[derive(Debug, Deserialize)]
struct BrowserCdpTarget {
    #[serde(rename = "type")]
    target_type: String,
    url: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    websocket_debugger_url: Option<String>,
}

async fn browser_cdp_page_ws_url(port: u16) -> anyhow::Result<String> {
    let targets = reqwest::get(format!("http://127.0.0.1:{port}/json"))
        .await?
        .json::<Vec<BrowserCdpTarget>>()
        .await?;

    targets
        .into_iter()
        .find(|target| {
            target.target_type == "page"
                && !target.url.starts_with("devtools://")
                && target.websocket_debugger_url.is_some()
        })
        .and_then(|target| target.websocket_debugger_url)
        .ok_or_else(|| anyhow!("No debuggable browser page found"))
}

async fn browser_cdp_evaluate(port: u16, expression: String) -> anyhow::Result<Value> {
    let ws_url = browser_cdp_page_ws_url(port).await?;
    let socket = WebSocket::connect(&ws_url, None::<&str>).await?;
    let (mut sink, mut stream) = socket.split().await;
    let request_id = 1_u64;
    let request = serde_json::json!({
        "id": request_id,
        "method": "Runtime.evaluate",
        "params": {
            "expression": expression,
            "awaitPromise": false,
            "returnByValue": true,
            "userGesture": true,
        }
    });

    sink.send(Message::new(request.to_string())).await?;
    while let Some(message) = stream.next().await {
        let message = message?;
        let Some(text) = message.text() else {
            continue;
        };
        let response: Value = serde_json::from_str(text)?;
        if response.get("id").and_then(Value::as_u64) == Some(request_id) {
            if let Some(error) = response.get("error") {
                return Err(anyhow!("CDP Runtime.evaluate failed: {error}"));
            }
            return Ok(response);
        }
    }

    Err(anyhow!(
        "CDP connection closed before Runtime.evaluate returned"
    ))
}

async fn browser_enable_element_picker(port: u16) -> anyhow::Result<()> {
    browser_cdp_evaluate(port, BROWSER_ELEMENT_PICKER_SCRIPT.to_string())
        .await
        .map(|_| ())
        .context("failed to enable browser element picker")
}

async fn browser_disable_element_picker(port: u16) -> anyhow::Result<()> {
    browser_cdp_evaluate(
        port,
        r#"(() => {
            if (window.__warpDisableElementPicker) {
                window.__warpDisableElementPicker();
            }
            window.__warpSelectedElementContext = null;
            return true;
        })()"#
            .to_string(),
    )
    .await
    .map(|_| ())
    .context("failed to disable browser element picker")
}

async fn browser_take_selected_element(port: u16) -> anyhow::Result<Option<String>> {
    let response = browser_cdp_evaluate(
        port,
        r#"(() => {
            const selected = window.__warpSelectedElementContext || null;
            if (selected) {
                window.__warpSelectedElementContext = null;
            }
            return selected;
        })()"#
            .to_string(),
    )
    .await
    .context("failed to read selected browser element")?;

    Ok(response
        .pointer("/result/result/value")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned))
}

const BROWSER_ELEMENT_PICKER_SCRIPT: &str = r#"(() => {
    if (window.__warpDisableElementPicker) {
        window.__warpDisableElementPicker();
    }

    window.__warpSelectedElementContext = null;

    const overlay = document.createElement('div');
    overlay.style.cssText = [
        'position: fixed',
        'z-index: 2147483646',
        'pointer-events: none',
        'border: 2px solid #315EFB',
        'background: rgba(49, 94, 251, 0.10)',
        'box-sizing: border-box',
        'display: none'
    ].join(';');

    const label = document.createElement('div');
    label.style.cssText = [
        'position: fixed',
        'z-index: 2147483647',
        'pointer-events: none',
        'max-width: 420px',
        'padding: 8px 10px',
        'border-radius: 8px',
        'background: #202839',
        'color: white',
        'font: 12px Arial, sans-serif',
        'box-shadow: 0 12px 40px rgba(0,0,0,0.30)',
        'display: none',
        'white-space: pre-wrap'
    ].join(';');

    document.documentElement.appendChild(overlay);
    document.documentElement.appendChild(label);

    function normalizeText(text) {
        return String(text || '').replace(/\s+/g, ' ').trim();
    }

    function truncate(text, max) {
        text = String(text || '');
        return text.length > max ? text.slice(0, max - 3) + '...' : text;
    }

    function cssEscape(value) {
        if (window.CSS && CSS.escape) {
            return CSS.escape(value);
        }
        return String(value).replace(/[^a-zA-Z0-9_-]/g, ch => '\\' + ch);
    }

    function selectorFor(element) {
        if (!element || element.nodeType !== Node.ELEMENT_NODE) {
            return '';
        }
        if (element.id) {
            return '#' + cssEscape(element.id);
        }

        const parts = [];
        let current = element;
        while (current && current.nodeType === Node.ELEMENT_NODE && parts.length < 6) {
            let part = current.localName;
            if (current.classList && current.classList.length > 0) {
                part += '.' + Array.from(current.classList)
                    .slice(0, 3)
                    .map(cssEscape)
                    .join('.');
            }
            const parent = current.parentElement;
            if (parent) {
                const siblings = Array.from(parent.children)
                    .filter(child => child.localName === current.localName);
                if (siblings.length > 1) {
                    part += `:nth-of-type(${siblings.indexOf(current) + 1})`;
                }
            }
            parts.unshift(part);
            current = parent;
        }
        return parts.join(' > ');
    }

    function attributesFor(element) {
        const names = ['id', 'class', 'role', 'aria-label', 'href', 'src', 'type', 'name', 'placeholder'];
        return names
            .map(name => {
                const value = element.getAttribute && element.getAttribute(name);
                return value ? `${name}="${truncate(normalizeText(value), 180)}"` : null;
            })
            .filter(Boolean)
            .join(', ');
    }

    function describe(element) {
        const rect = element.getBoundingClientRect();
        const tag = element.localName || element.tagName.toLowerCase();
        const selector = selectorFor(element);
        const attrs = attributesFor(element) || 'none';
        const text = truncate(normalizeText(element.innerText || element.textContent || element.getAttribute('aria-label') || ''), 3000);
        const html = truncate(element.outerHTML || '', 1200);
        return [
            'Web page element',
            `Page: ${document.title || 'Untitled page'}`,
            `URL: ${location.href}`,
            `Selector: ${selector}`,
            `Tag: <${tag}>`,
            `Bounds: ${Math.round(rect.width)}x${Math.round(rect.height)} at (${Math.round(rect.left)}, ${Math.round(rect.top)})`,
            `Attributes: ${attrs}`,
            '',
            'Text:',
            text || '(no visible text)',
            '',
            'HTML:',
            html
        ].join('\n');
    }

    function show(element) {
        if (!element || element === overlay || element === label) {
            return;
        }
        const rect = element.getBoundingClientRect();
        overlay.style.display = 'block';
        overlay.style.left = `${rect.left}px`;
        overlay.style.top = `${rect.top}px`;
        overlay.style.width = `${rect.width}px`;
        overlay.style.height = `${rect.height}px`;

        label.style.display = 'block';
        label.textContent = `${element.localName || element.tagName.toLowerCase()}  ${Math.round(rect.width)}x${Math.round(rect.height)}\n${selectorFor(element)}`;
        const labelX = Math.min(Math.max(8, rect.left), window.innerWidth - 430);
        const labelY = rect.top > 72 ? rect.top - 64 : rect.bottom + 8;
        label.style.left = `${labelX}px`;
        label.style.top = `${Math.max(8, labelY)}px`;
    }

    function cleanup() {
        document.removeEventListener('mousemove', onMove, true);
        document.removeEventListener('click', onClick, true);
        document.removeEventListener('keydown', onKeyDown, true);
        overlay.remove();
        label.remove();
        window.__warpDisableElementPicker = null;
    }

    function onMove(event) {
        show(event.target);
    }

    function onClick(event) {
        event.preventDefault();
        event.stopPropagation();
        event.stopImmediatePropagation();
        window.__warpSelectedElementContext = describe(event.target);
        cleanup();
        return false;
    }

    function onKeyDown(event) {
        if (event.key === 'Escape') {
            event.preventDefault();
            event.stopPropagation();
            cleanup();
        }
    }

    window.__warpDisableElementPicker = cleanup;
    document.addEventListener('mousemove', onMove, true);
    document.addEventListener('click', onClick, true);
    document.addEventListener('keydown', onKeyDown, true);
    return true;
})()"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RightPanelMode {
    CodeReview,
    Files,
    Browser,
}

impl RightPanelMode {
    fn label(self) -> &'static str {
        match self {
            Self::CodeReview => "Review",
            Self::Files => "Files",
            Self::Browser => "Browser",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::CodeReview => Icon::Diff,
            Self::Files => Icon::FileCopy,
            Self::Browser => Icon::Globe,
        }
    }
}

#[derive(Default)]
struct ModeButtonMouseStates {
    code_review: MouseStateHandle,
    files: MouseStateHandle,
    browser: MouseStateHandle,
}

impl ModeButtonMouseStates {
    fn handle(&self, mode: RightPanelMode) -> MouseStateHandle {
        match mode {
            RightPanelMode::CodeReview => self.code_review.clone(),
            RightPanelMode::Files => self.files.clone(),
            RightPanelMode::Browser => self.browser.clone(),
        }
    }
}

fn normalize_browser_url(raw_url: &str) -> Option<String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.contains("://") || trimmed.starts_with("about:") {
        return Some(trimmed.to_string());
    }

    if trimmed.starts_with('/') {
        return Some(format!("file://{trimmed}"));
    }

    if trimmed.starts_with("localhost")
        || trimmed.starts_with("127.")
        || trimmed.starts_with("0.0.0.0")
        || trimmed.starts_with("[::1]")
    {
        return Some(format!("http://{trimmed}"));
    }

    Some(format!("https://{trimmed}"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BrowserElementCandidate {
    tag: String,
    selector: String,
    text: String,
    attributes: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BrowserDocument {
    url: String,
    title: Option<String>,
    elements: Vec<BrowserElementCandidate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BrowserLoadState {
    Blank,
    Loading { url: String },
    Loaded(BrowserDocument),
    Error { url: String, message: String },
}

impl Default for BrowserLoadState {
    fn default() -> Self {
        Self::Blank
    }
}

struct ExternalBrowserSurfaceElement {
    window_id: WindowId,
    url: Option<String>,
    background: ColorU,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ExternalBrowserSurfaceElement {
    fn new(window_id: WindowId, url: Option<String>, background: ColorU) -> Self {
        Self {
            window_id,
            url,
            background,
            size: None,
            origin: None,
        }
    }
}

impl Element for ExternalBrowserSurfaceElement {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = constraint.max;
        self.size = Some(size);
        size
    }

    fn after_layout(&mut self, _ctx: &mut AfterLayoutContext, _app: &AppContext) {}

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        let size = self.size.unwrap_or(Vector2F::zero());
        ctx.scene
            .draw_rect_with_hit_recording(pathfinder_geometry::rect::RectF::new(origin, size))
            .with_background(self.background);

        let Some(url) = self.url.as_deref() else {
            hide_external_browser_host();
            return;
        };

        let Some(window) = app.windows().platform_window(self.window_id) else {
            hide_external_browser_host();
            return;
        };

        let scale = window.backing_scale_factor();
        let parent_window_id = window.x11_window_id().map(|id| format!("{id:#x}"));
        let (x, y) = if parent_window_id.is_some() {
            (origin.x(), origin.y())
        } else {
            let window_origin = window.origin();
            (
                window_origin.x() + origin.x(),
                window_origin.y() + origin.y(),
            )
        };
        let rect = BrowserHostRect {
            x: (x * scale).round() as i32,
            y: (y * scale).round() as i32,
            width: (size.x() * scale).round().max(1.) as u32,
            height: (size.y() * scale).round().max(1.) as u32,
        };
        show_external_browser_host(url, parent_window_id, rect);
        ctx.repaint_after(BROWSER_HOST_REPAINT_INTERVAL);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        _event: &DispatchedEvent,
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        false
    }
}

fn normalize_browser_text(text: &str) -> String {
    text.split_whitespace().join(" ")
}

fn truncate_browser_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut truncated = text
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

fn browser_attr_value(attrs: &[Attribute], name: &str) -> Option<String> {
    attrs
        .iter()
        .find_map(|attr| (attr.name.local.as_ref() == name).then(|| attr.value.to_string()))
}

fn browser_attr_summary(attrs: &[Attribute]) -> Vec<(String, String)> {
    [
        "id",
        "class",
        "role",
        "aria-label",
        "href",
        "src",
        "type",
        "name",
        "placeholder",
    ]
    .into_iter()
    .filter_map(|name| {
        browser_attr_value(attrs, name).map(|value| {
            (
                name.to_string(),
                truncate_browser_text(&normalize_browser_text(&value), 160),
            )
        })
    })
    .collect()
}

fn browser_selector_for(tag: &str, attrs: &[Attribute], fallback_index: usize) -> String {
    if let Some(id) = browser_attr_value(attrs, "id") {
        let id = id.trim();
        if !id.is_empty() {
            return format!("#{id}");
        }
    }

    if let Some(class_names) = browser_attr_value(attrs, "class") {
        let classes = class_names
            .split_whitespace()
            .take(3)
            .map(|class_name| format!(".{class_name}"))
            .join("");
        if !classes.is_empty() {
            return format!("{tag}{classes}");
        }
    }

    format!("{tag}:nth-candidate({fallback_index})")
}

fn browser_element_text(handle: &Handle) -> String {
    fn collect_text(handle: &Handle, output: &mut String) {
        match &handle.data {
            NodeData::Text { contents } => {
                output.push_str(&contents.borrow());
                output.push(' ');
            }
            NodeData::Element { name, .. } => {
                let tag = name.local.to_string();
                if matches!(
                    tag.as_str(),
                    "script" | "style" | "noscript" | "svg" | "head"
                ) {
                    return;
                }
                for child in handle.children.borrow().iter() {
                    collect_text(child, output);
                }
            }
            _ => {
                for child in handle.children.borrow().iter() {
                    collect_text(child, output);
                }
            }
        }
    }

    let mut text = String::new();
    collect_text(handle, &mut text);
    normalize_browser_text(&text)
}

fn browser_element_is_candidate(tag: &str, attrs: &[Attribute], text: &str) -> bool {
    if text.is_empty() {
        return false;
    }

    if matches!(
        tag,
        "a" | "button"
            | "input"
            | "textarea"
            | "select"
            | "label"
            | "summary"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "p"
            | "li"
            | "pre"
            | "code"
            | "blockquote"
    ) {
        return true;
    }

    browser_attr_value(attrs, "role").is_some()
        || browser_attr_value(attrs, "aria-label").is_some()
        || browser_attr_value(attrs, "data-testid").is_some()
}

fn parse_browser_document(url: String, html: &str) -> BrowserDocument {
    fn walk(
        handle: &Handle,
        title: &mut Option<String>,
        elements: &mut Vec<BrowserElementCandidate>,
    ) {
        let NodeData::Element { name, attrs, .. } = &handle.data else {
            for child in handle.children.borrow().iter() {
                walk(child, title, elements);
            }
            return;
        };

        let tag = name.local.to_string();
        if matches!(tag.as_str(), "script" | "style" | "noscript" | "svg") {
            return;
        }

        if tag == "title" {
            let text = browser_element_text(handle);
            if !text.is_empty() {
                *title = Some(truncate_browser_text(&text, 160));
            }
        }

        if elements.len() < BROWSER_MAX_ELEMENT_CANDIDATES {
            let attrs = attrs.borrow();
            let mut text = browser_element_text(handle);

            if text.is_empty() {
                text = browser_attr_value(&attrs, "aria-label")
                    .or_else(|| browser_attr_value(&attrs, "placeholder"))
                    .or_else(|| browser_attr_value(&attrs, "value"))
                    .map(|value| normalize_browser_text(&value))
                    .unwrap_or_default();
            }

            if browser_element_is_candidate(&tag, &attrs, &text) {
                let candidate_index = elements.len() + 1;
                elements.push(BrowserElementCandidate {
                    tag: tag.clone(),
                    selector: browser_selector_for(&tag, &attrs, candidate_index),
                    text: truncate_browser_text(&text, BROWSER_MAX_ELEMENT_TEXT_CHARS),
                    attributes: browser_attr_summary(&attrs),
                });
            }
        }

        for child in handle.children.borrow().iter() {
            walk(child, title, elements);
        }
    }

    let dom = parse_document(RcDom::default(), Default::default()).one(html.to_string());
    let mut title = None;
    let mut elements = Vec::new();
    walk(&dom.document, &mut title, &mut elements);

    BrowserDocument {
        url,
        title,
        elements,
    }
}

async fn load_browser_document(url: String) -> Result<BrowserDocument, String> {
    let parsed_url = Url::parse(&url).map_err(|err| format!("Invalid URL: {err}"))?;
    let html = match parsed_url.scheme() {
        "http" | "https" => {
            let client = http_client::Client::new();
            let response = client
                .get(url.clone())
                .timeout(Duration::from_secs(15))
                .send()
                .await
                .map_err(|err| format!("Request failed: {err}"))?;

            let status = response.status();
            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            if !status.is_success() {
                return Err(format!("HTTP {status}"));
            }
            if !content_type.is_empty()
                && !content_type.contains("text/html")
                && !content_type.contains("application/xhtml")
                && !content_type.contains("text/plain")
            {
                return Err(format!("Unsupported content type: {content_type}"));
            }

            response
                .text()
                .await
                .map_err(|err| format!("Could not read response: {err}"))?
        }
        "file" => {
            let path = parsed_url
                .to_file_path()
                .map_err(|_| "Invalid file URL".to_string())?;
            async_fs::read_to_string(path)
                .await
                .map_err(|err| format!("Could not read file: {err}"))?
        }
        "about" => String::new(),
        scheme => return Err(format!("Unsupported URL scheme: {scheme}")),
    };

    Ok(parse_browser_document(url, &html))
}

fn deduplicate_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen_paths = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen_paths.insert(path.clone()))
        .collect()
}

/// Describes which agent destination is available for sending review comments.
#[derive(Clone, Debug, PartialEq)]
pub enum ReviewDestination {
    /// No terminal is available to receive comments.
    None,
    /// A Warp agent terminal is available (input box visible, not executing).
    Warp,
    /// A CLI agent (e.g. Claude Code, Gemini) is running in a terminal.
    Cli(CLIAgent),
}

/// Result of attempting to submit review comments to a terminal.
pub enum ReviewSubmissionResult {
    Success {
        comment_count: usize,
        file_count: usize,
        destination: CodeReviewContextDestination,
    },
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReviewTerminalUnavailableReason {
    NoSelectedRepo,
    SessionPathUnavailable,
    SessionOutsideSelectedRepo,
    AIDisabled,
    TerminalExecuting,
    InputBoxNotVisible,
}

impl ReviewTerminalUnavailableReason {
    fn label(&self) -> &'static str {
        match self {
            Self::NoSelectedRepo => "no repo is selected for code review",
            Self::SessionPathUnavailable => "session cwd is unavailable or not local",
            Self::SessionOutsideSelectedRepo => "session cwd is not inside selected repo",
            Self::AIDisabled => "AI is disabled for Warp review destinations",
            Self::TerminalExecuting => "terminal is currently executing a command",
            Self::InputBoxNotVisible => "terminal input box is not visible",
        }
    }
}

#[derive(Debug)]
struct ReviewTerminalStatus {
    active_session_path: Option<PathBuf>,
    current_repo_path: Option<LocalOrRemotePath>,
    active_cli_agent: Option<String>,
    is_executing: bool,
    is_input_box_visible: bool,
    unavailable_reasons: Vec<ReviewTerminalUnavailableReason>,
}
impl ReviewTerminalStatus {
    fn is_available(&self) -> bool {
        self.unavailable_reasons.is_empty()
    }
}

struct CodeReviewState {
    dropdown: ViewHandle<Dropdown<RightPanelAction>>,
    available_repos: Vec<LocalOrRemotePath>,
    /// The repository path of the focused terminal
    focused_repo_path: Option<LocalOrRemotePath>,
    /// The repository path of the repository selected in the dropdown
    selected_repo_path: Option<LocalOrRemotePath>,
    /// Avoid showing the jump-to-repo button if the focused repo has not changed
    did_focused_repo_change: bool,
}

#[cfg(feature = "local_fs")]
struct CodeReviewSessionEnv {
    is_remote: bool,
    is_wsl: bool,
}

/// Resolve the repo-switcher dropdown's text color from the current theme.
/// Kept as a free function so the construction site and the
/// `AppearanceEvent::ThemeChanged` subscription compute the exact same color.
fn repo_dropdown_font_color(appearance: &Appearance) -> ColorU {
    appearance
        .theme()
        .sub_text_color(appearance.theme().background())
        .into_solid()
}

impl CodeReviewState {
    pub fn new(ctx: &mut ViewContext<RightPanelView>) -> Self {
        CodeReviewState {
            dropdown: ctx.add_typed_action_view(|ctx| {
                let (font_color, ui_font_size) = {
                    let appearance = Appearance::as_ref(ctx);
                    (
                        repo_dropdown_font_color(appearance),
                        appearance.ui_font_size(),
                    )
                };
                let mut dropdown = Dropdown::new(ctx);
                dropdown.set_menu_position(
                    PositionedElementAnchor::BottomRight,
                    ChildAnchor::TopRight,
                    ctx,
                );
                dropdown.set_main_axis_size(MainAxisSize::Min, ctx);
                dropdown.set_font_color(font_color, ctx);
                dropdown.set_font_size(ui_font_size, ctx);
                dropdown.set_vertical_margin(0., ctx);
                dropdown.set_top_bar_height(warp_core::ui::icons::ICON_DIMENSIONS, ctx);
                dropdown.set_padding(HEADER_BUTTON_PADDING, ctx);

                // The font color above is derived from the active theme and
                // cached inside the dropdown. Without this subscription, the
                // cached value goes stale across light/dark switches and the
                // header label becomes unreadable on the new background
                // (e.g. white-on-white in light mode after starting in dark).
                ctx.subscribe_to_model(&Appearance::handle(ctx), |dropdown, _, event, ctx| {
                    if matches!(event, AppearanceEvent::ThemeChanged) {
                        let font_color = repo_dropdown_font_color(Appearance::as_ref(ctx));
                        dropdown.set_font_color(font_color, ctx);
                    }
                });

                dropdown
            }),
            available_repos: vec![],
            selected_repo_path: None,
            focused_repo_path: None,
            did_focused_repo_change: false,
        }
    }

    #[cfg(not(feature = "local_fs"))]
    fn set_available_repos(
        &mut self,
        _repos: Vec<LocalOrRemotePath>,
        _ctx: &mut ViewContext<RightPanelView>,
    ) {
    }

    #[cfg(feature = "local_fs")]
    fn set_available_repos(
        &mut self,
        repos: Vec<LocalOrRemotePath>,
        ctx: &mut ViewContext<RightPanelView>,
    ) {
        let should_clear = self
            .selected_repo_path
            .as_ref()
            .map(|p| !repos.contains(p))
            .unwrap_or(false);
        if should_clear {
            self.selected_repo_path = None;
        }
        self.available_repos = repos;

        self.update_repo_dropdown(ctx);

        // Auto-select first repo if we have one and no selection yet
        if self.selected_repo_path.is_none() {
            if let Some(first_repo) = self.available_repos.first() {
                self.set_selected_repo(first_repo.clone(), ctx);
            }
        }
    }

    #[cfg(not(feature = "local_fs"))]
    pub fn set_selected_repo(
        &mut self,
        _repo_path: LocalOrRemotePath,
        _ctx: &mut ViewContext<RightPanelView>,
    ) {
    }

    #[cfg(feature = "local_fs")]
    pub fn set_selected_repo(
        &mut self,
        repo_path: LocalOrRemotePath,
        ctx: &mut ViewContext<RightPanelView>,
    ) {
        self.set_selected_repo_internal(repo_path, true, ctx);
    }

    pub fn set_focused_repo(
        &mut self,
        repo_path: Option<LocalOrRemotePath>,
        ctx: &mut ViewContext<RightPanelView>,
    ) {
        self.did_focused_repo_change = true;
        self.focused_repo_path = repo_path;
        ctx.notify();
    }

    /// Internal method to set the selected repo with control over whether to update the dropdown.
    /// When `update_dropdown` is false, we skip updating the dropdown (useful when the change
    /// is coming from the dropdown itself to avoid circular updates).
    #[cfg(feature = "local_fs")]
    fn set_selected_repo_internal(
        &mut self,
        repo_path: LocalOrRemotePath,
        update_dropdown: bool,
        ctx: &mut ViewContext<RightPanelView>,
    ) {
        if repo_path.is_remote() && !FeatureFlag::RemoteCodeReview.is_enabled() {
            return;
        }
        if self.selected_repo_path.as_ref() == Some(&repo_path) {
            return;
        }

        self.did_focused_repo_change = false;
        self.selected_repo_path = Some(repo_path.clone());

        // Only update the dropdown if requested (not when selection came from dropdown itself)
        if update_dropdown {
            self.update_repo_dropdown(ctx);
        }

        ctx.notify();
    }

    #[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
    fn get_repo_display_name(
        &self,
        repo_path: &LocalOrRemotePath,
        ctx: &AppContext,
    ) -> Option<String> {
        let name = display_name_with_host(repo_path, ctx);
        (!name.is_empty()).then_some(name)
    }

    #[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
    fn update_repo_dropdown(&mut self, ctx: &mut ViewContext<RightPanelView>) {
        // Collect data before borrowing mutably
        let (items, selected_display_name) = {
            let items: Vec<DropdownItem<RightPanelAction>> = self
                .available_repos
                .iter()
                .map(|repo_path| {
                    let display_name = self
                        .get_repo_display_name(repo_path, ctx)
                        .unwrap_or_else(|| "Unknown".to_string());
                    DropdownItem::new(
                        display_name,
                        RightPanelAction::SelectRepo {
                            repo_path: repo_path.clone(),
                            from_dropdown: true,
                        },
                    )
                })
                .collect();

            let selected_display_name = self
                .selected_repo_path
                .as_ref()
                .and_then(|selected| self.get_repo_display_name(selected, ctx));

            (items, selected_display_name)
        };

        // Now update the dropdown
        if !items.is_empty() {
            self.dropdown.update(ctx, |dropdown, ctx| {
                dropdown.set_items(items, ctx);
                if let Some(display_name) = selected_display_name {
                    dropdown.set_selected_by_name(display_name, ctx);
                }
            });
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
pub enum RightPanelAction {
    ToggleFileSidebar,
    SetMode(RightPanelMode),
    SelectRepo {
        repo_path: LocalOrRemotePath,
        from_dropdown: bool,
    },
    OpenRepository,
    OpenBrowserCurrentUrl,
    OpenBrowserExternal,
    CopyBrowserUrl,
    RefreshBrowserUrl,
    BrowserBack,
    BrowserForward,
    ToggleBrowserElementPicker,
    AttachBrowserElement {
        index: usize,
    },
    ToggleMaximize,
}

#[derive(Clone, Debug)]
#[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
pub enum RightPanelEvent {
    ToggleMaximize,
    #[cfg(feature = "local_fs")]
    FileTree(RightPanelFileTreeEvent),
    #[cfg(feature = "local_fs")]
    OpenFileFromFileManager {
        location: LocalOrRemotePath,
        target: FileTarget,
        line_col: Option<LineAndColumnArg>,
    },
    #[cfg(feature = "local_fs")]
    OpenFileWithTarget {
        path: PathBuf,
        target: FileTarget,
        line_col: Option<LineAndColumnArg>,
    },
    OpenFileInNewTab {
        path: LocalOrRemotePath,
        line_and_column: Option<LineAndColumnArg>,
    },
    AttachBrowserSelectionAsContext {
        text: String,
    },
    #[cfg(not(target_family = "wasm"))]
    OpenLspLogs {
        log_path: PathBuf,
    },
}

#[cfg(feature = "local_fs")]
#[derive(Clone, Debug)]
pub enum RightPanelFileTreeEvent {
    FileRenamed {
        old_path: PathBuf,
        new_path: PathBuf,
    },
    FileDeleted {
        path: PathBuf,
    },
    AttachPathAsContext {
        path: PathBuf,
    },
    CDToDirectory {
        path: PathBuf,
    },
    OpenDirectoryInNewTab {
        path: PathBuf,
    },
}

pub struct RightPanelView {
    window_id: WindowId,
    resizable_state_handle: ResizableStateHandle,
    close_button_mouse_state: MouseStateHandle,
    file_navigation_button_mouse_state: MouseStateHandle,
    mode_button_mouse_states: ModeButtonMouseStates,
    #[cfg(feature = "local_fs")]
    open_repository_button: ViewHandle<ActionButton>,
    browser_url_editor: ViewHandle<EditorView>,
    browser_back_button: ViewHandle<ActionButton>,
    browser_forward_button: ViewHandle<ActionButton>,
    browser_open_button: ViewHandle<ActionButton>,
    browser_copy_button: ViewHandle<ActionButton>,
    browser_refresh_button: ViewHandle<ActionButton>,
    browser_element_picker_button: ViewHandle<ActionButton>,
    pub active_pane_group: Option<ViewHandle<PaneGroup>>,
    #[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
    working_directories_model: ModelHandle<WorkingDirectoriesModel>,
    maximize_button: ViewHandle<ActionButton>,
    code_review_state: Option<CodeReviewState>,
    #[cfg(feature = "local_fs")]
    code_review_session_env: Option<CodeReviewSessionEnv>,
    #[cfg(feature = "local_fs")]
    file_tree_views: HashMap<EntityId, ViewHandle<FileTreeView>>,
    active_mode: RightPanelMode,
    browser_current_url: Option<String>,
    browser_history: Vec<String>,
    browser_history_index: Option<usize>,
    browser_load_state: BrowserLoadState,
    browser_element_picker_enabled: bool,
    browser_element_mouse_states: Vec<MouseStateHandle>,
    browser_scroll_state: ClippedScrollStateHandle,
    is_agent_management_view_open: bool,
    panel_position: super::PanelPosition,
}

impl RightPanelView {
    pub fn init(app: &mut AppContext) {
        use warpui::keymap::macros::*;

        app.register_editable_bindings([EditableBinding::new(
            "workspace:toggle_maximize_code_review_panel",
            "Toggle Maximize Code Review Panel",
            RightPanelAction::ToggleMaximize,
        )
        .with_enabled(|| cfg!(feature = "local_fs"))
        .with_context_predicate(id!("RightPanelView"))
        .with_custom_action(CustomAction::ToggleMaximizePane)]);
    }

    pub fn new(
        working_directories_model: ModelHandle<WorkingDirectoriesModel>,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let resizable_data_handle = ResizableData::handle(ctx);
        let resizable_state_handle = match resizable_data_handle
            .as_ref(ctx)
            .get_handle(ctx.window_id(), ModalType::RightPanelWidth)
        {
            Some(handle) => handle,
            None => {
                log::error!("Couldn't retrieve Right panel resizable state handle.");
                resizable_state_handle(600.0)
            }
        };

        let code_review_state = if cfg!(feature = "local_fs") {
            Some(CodeReviewState::new(ctx))
        } else {
            None
        };

        ctx.subscribe_to_model(&working_directories_model, move |me, _, event, ctx| {
            me.handle_working_directories_event(event, ctx)
        });

        // Recompute terminal availability when CLI agent sessions start or end.
        ctx.subscribe_to_model(&CLIAgentSessionsModel::handle(ctx), |me, _, _, ctx| {
            me.recompute_terminal_availability(ctx);
        });

        // Recompute terminal availability when AI is toggled on or off, so the
        // send button and tooltip update immediately.
        ctx.subscribe_to_model(&AISettings::handle(ctx), |me, _, event, ctx| {
            if matches!(event, AISettingsChangedEvent::IsAnyAIEnabled { .. }) {
                me.recompute_terminal_availability(ctx);
            }
        });

        let maximize_button = ctx.add_typed_action_view(|ctx| {
            let mut button = ActionButton::new("", PaneHeaderTheme)
                .with_icon(Icon::Maximize)
                .with_tooltip("Maximize")
                .with_tooltip_positioning_provider(Arc::new(MenuPositioning::BelowInputBox))
                .on_click(|ctx| ctx.dispatch_typed_action(RightPanelAction::ToggleMaximize));

            if let Some(keybinding_label) = keybinding_name_to_display_string(
                "workspace:toggle_maximize_code_review_panel",
                ctx,
            ) {
                button = button.with_tooltip_sublabel(keybinding_label);
            }

            button
        });

        let browser_url_editor = ctx.add_typed_action_view(|ctx| {
            let appearance = Appearance::as_ref(ctx);
            let options = EditorOptions {
                text: TextOptions::ui_text(Some(13.), appearance),
                single_line: true,
                select_all_on_focus: true,
                clear_selections_on_blur: true,
                ..Default::default()
            };
            let mut editor = EditorView::new(options, ctx);
            editor.set_placeholder_text("localhost:3000 or https://example.com", ctx);
            editor
        });
        ctx.subscribe_to_view(&browser_url_editor, |me, _handle, event, ctx| {
            me.handle_browser_url_editor_event(event, ctx);
        });

        let browser_back_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("", PaneHeaderTheme)
                .with_icon(Icon::ChevronLeft)
                .with_tooltip("Back")
                .with_tooltip_positioning_provider(Arc::new(MenuPositioning::BelowInputBox))
                .on_click(|ctx| ctx.dispatch_typed_action(RightPanelAction::BrowserBack))
        });

        let browser_forward_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("", PaneHeaderTheme)
                .with_icon(Icon::ChevronRight)
                .with_tooltip("Forward")
                .with_tooltip_positioning_provider(Arc::new(MenuPositioning::BelowInputBox))
                .on_click(|ctx| ctx.dispatch_typed_action(RightPanelAction::BrowserForward))
        });

        let browser_open_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("", PaneHeaderTheme)
                .with_icon(Icon::LinkExternal)
                .with_tooltip("Open externally")
                .with_tooltip_positioning_provider(Arc::new(MenuPositioning::BelowInputBox))
                .on_click(|ctx| ctx.dispatch_typed_action(RightPanelAction::OpenBrowserExternal))
        });

        let browser_copy_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("", PaneHeaderTheme)
                .with_icon(Icon::Copy)
                .with_tooltip("Copy URL")
                .with_tooltip_positioning_provider(Arc::new(MenuPositioning::BelowInputBox))
                .on_click(|ctx| ctx.dispatch_typed_action(RightPanelAction::CopyBrowserUrl))
        });

        let browser_refresh_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("", PaneHeaderTheme)
                .with_icon(Icon::RefreshCcw)
                .with_tooltip("Reopen URL")
                .with_tooltip_positioning_provider(Arc::new(MenuPositioning::BelowInputBox))
                .on_click(|ctx| ctx.dispatch_typed_action(RightPanelAction::RefreshBrowserUrl))
        });

        let browser_element_picker_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("", PaneHeaderTheme)
                .with_icon(Icon::CornersOfBox)
                .with_tooltip("Select page element")
                .with_tooltip_positioning_provider(Arc::new(MenuPositioning::BelowInputBox))
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(RightPanelAction::ToggleBrowserElementPicker)
                })
        });

        #[cfg(feature = "local_fs")]
        let open_repository_button = ctx.add_typed_action_view(|_| {
            ActionButton::new("Open repository", NakedTheme)
                .with_size(crate::view_components::action_button::ButtonSize::Small)
                .with_tooltip("Navigate to a repo and initialize it for coding")
                .with_tooltip_alignment(TooltipAlignment::Center)
                .on_click(|ctx| ctx.dispatch_typed_action(RightPanelAction::OpenRepository))
        });

        Self {
            window_id: ctx.window_id(),
            resizable_state_handle,
            close_button_mouse_state: Default::default(),
            file_navigation_button_mouse_state: Default::default(),
            mode_button_mouse_states: Default::default(),
            #[cfg(feature = "local_fs")]
            open_repository_button,
            browser_url_editor,
            browser_back_button,
            browser_forward_button,
            browser_open_button,
            browser_copy_button,
            browser_refresh_button,
            browser_element_picker_button,
            active_pane_group: None,
            working_directories_model,
            maximize_button,
            code_review_state,
            #[cfg(feature = "local_fs")]
            code_review_session_env: None,
            #[cfg(feature = "local_fs")]
            file_tree_views: HashMap::new(),
            active_mode: RightPanelMode::CodeReview,
            browser_current_url: None,
            browser_history: Vec::new(),
            browser_history_index: None,
            browser_load_state: BrowserLoadState::Blank,
            browser_element_picker_enabled: false,
            browser_element_mouse_states: Vec::new(),
            browser_scroll_state: ClippedScrollStateHandle::new(),
            is_agent_management_view_open: false,
            panel_position: super::PanelPosition::Right,
        }
    }

    pub fn set_agent_management_view_open(&mut self, is_open: bool, ctx: &mut ViewContext<Self>) {
        self.is_agent_management_view_open = is_open;
        ctx.notify();
    }

    pub fn set_panel_position(
        &mut self,
        position: super::PanelPosition,
        ctx: &mut ViewContext<Self>,
    ) {
        self.panel_position = position;
        ctx.notify();
    }

    fn set_active_mode(&mut self, mode: RightPanelMode, ctx: &mut ViewContext<Self>) {
        let was_files = self.active_mode == RightPanelMode::Files;
        self.active_mode = mode;
        if mode != RightPanelMode::Browser {
            self.browser_element_picker_enabled = false;
            if let Some(port) = external_browser_debugging_port() {
                ctx.spawn(browser_disable_element_picker(port), |_, result, _| {
                    if let Err(err) = result {
                        log::warn!("failed to disable browser element picker: {err:?}");
                    }
                });
            }
            hide_external_browser_host();
        }

        #[cfg(feature = "local_fs")]
        {
            if was_files && mode != RightPanelMode::Files {
                self.set_active_file_tree_visible(false, ctx);
            }

            if mode == RightPanelMode::Files {
                self.update_file_tree_for_active_pane_group(ctx);
                self.focus_active_file_tree(ctx);
            } else if mode == RightPanelMode::CodeReview {
                if let Some(repo_path) = self.selected_repo_path().cloned() {
                    self.ensure_code_review_view_exists(&repo_path, ctx);
                }
                self.recompute_terminal_availability(ctx);
            }
        }

        ctx.notify();
    }

    fn current_browser_url_from_editor(&self, app: &AppContext) -> Option<String> {
        let editor_text = self
            .browser_url_editor
            .read(app, |editor, ctx| editor.buffer_text(ctx));
        normalize_browser_url(&editor_text).or_else(|| self.browser_current_url.clone())
    }

    fn can_browser_go_back(&self) -> bool {
        self.browser_history_index.is_some_and(|index| index > 0)
    }

    fn can_browser_go_forward(&self) -> bool {
        self.browser_history_index
            .is_some_and(|index| index + 1 < self.browser_history.len())
    }

    fn open_browser_url(&mut self, url: String, ctx: &mut ViewContext<Self>) {
        let Some(url) = normalize_browser_url(&url) else {
            return;
        };

        let should_push_history = match self.browser_history_index {
            Some(index) => self
                .browser_history
                .get(index)
                .map_or(true, |current| current != &url),
            None => true,
        };

        if should_push_history {
            if let Some(index) = self.browser_history_index {
                self.browser_history.truncate(index + 1);
            }
            self.browser_history.push(url.clone());
            self.browser_history_index = Some(self.browser_history.len() - 1);
        }

        self.load_browser_url(url, ctx);
    }

    fn load_browser_url(&mut self, url: String, ctx: &mut ViewContext<Self>) {
        self.browser_current_url = Some(url.clone());
        self.browser_element_picker_enabled = false;
        self.browser_url_editor.update(ctx, |editor, ctx| {
            editor.set_buffer_text(&url, ctx);
        });

        if url == "about:blank" {
            self.browser_load_state = BrowserLoadState::Blank;
            self.browser_element_mouse_states.clear();
            hide_external_browser_host();
            ctx.notify();
            return;
        }

        self.browser_load_state = BrowserLoadState::Loading { url: url.clone() };
        self.browser_element_mouse_states.clear();
        ctx.notify();

        ctx.spawn(
            load_browser_document(url.clone()),
            move |me, result, ctx| {
                if me.browser_current_url.as_deref() != Some(url.as_str()) {
                    return;
                }

                match result {
                    Ok(document) => {
                        me.browser_element_mouse_states
                            .resize_with(document.elements.len(), MouseStateHandle::default);
                        me.browser_load_state = BrowserLoadState::Loaded(document);
                    }
                    Err(message) => {
                        me.browser_load_state = BrowserLoadState::Error {
                            url: url.clone(),
                            message,
                        };
                    }
                }
                ctx.notify();
            },
        );
    }

    fn navigate_browser_history(&mut self, offset: isize, ctx: &mut ViewContext<Self>) {
        let Some(index) = self.browser_history_index else {
            return;
        };
        let next_index = index as isize + offset;
        if next_index < 0 || next_index as usize >= self.browser_history.len() {
            return;
        }

        let next_index = next_index as usize;
        let Some(url) = self.browser_history.get(next_index).cloned() else {
            return;
        };
        self.browser_history_index = Some(next_index);
        self.load_browser_url(url, ctx);
    }

    fn open_browser_external(&self, ctx: &mut ViewContext<Self>) {
        if let Some(url) = self.current_browser_url_from_editor(ctx) {
            ctx.open_url(&url);
        }
    }

    fn format_browser_element_context(
        document: &BrowserDocument,
        element: &BrowserElementCandidate,
    ) -> String {
        let title = document.title.as_deref().unwrap_or("Untitled page");
        let attributes = if element.attributes.is_empty() {
            "Attributes: none".to_string()
        } else {
            format!(
                "Attributes: {}",
                element
                    .attributes
                    .iter()
                    .map(|(name, value)| format!("{name}=\"{value}\""))
                    .join(", ")
            )
        };

        truncate_browser_text(
            &format!(
                "Web page element\nPage: {title}\nURL: {}\nSelector: {}\nTag: <{}>\n{}\nText:\n{}",
                document.url, element.selector, element.tag, attributes, element.text
            ),
            BROWSER_MAX_CONTEXT_CHARS,
        )
    }

    fn attach_browser_element_as_context(&mut self, index: usize, ctx: &mut ViewContext<Self>) {
        let Some(text) = (match &self.browser_load_state {
            BrowserLoadState::Loaded(document) => document
                .elements
                .get(index)
                .map(|element| Self::format_browser_element_context(document, element)),
            _ => None,
        }) else {
            return;
        };

        self.browser_element_picker_enabled = false;
        ctx.emit(RightPanelEvent::AttachBrowserSelectionAsContext { text });
        ctx.notify();
    }

    fn toggle_browser_element_picker(&mut self, ctx: &mut ViewContext<Self>) {
        if self.browser_element_picker_enabled {
            self.browser_element_picker_enabled = false;
            if let Some(port) = external_browser_debugging_port() {
                ctx.spawn(browser_disable_element_picker(port), |_, result, _| {
                    if let Err(err) = result {
                        log::warn!("failed to disable browser element picker: {err:?}");
                    }
                });
            }
            ctx.notify();
            return;
        }

        let Some(port) = external_browser_debugging_port() else {
            log::warn!("browser element picker requested before browser host was ready");
            return;
        };

        self.browser_element_picker_enabled = true;
        ctx.notify();
        ctx.spawn(
            browser_enable_element_picker(port),
            |me, result, ctx| match result {
                Ok(()) => {
                    me.poll_browser_element_selection(ctx);
                }
                Err(err) => {
                    log::warn!("failed to enable browser element picker: {err:?}");
                    me.browser_element_picker_enabled = false;
                    ctx.notify();
                }
            },
        );
    }

    fn poll_browser_element_selection(&mut self, ctx: &mut ViewContext<Self>) {
        if !self.browser_element_picker_enabled {
            return;
        }

        let Some(port) = external_browser_debugging_port() else {
            self.browser_element_picker_enabled = false;
            ctx.notify();
            return;
        };

        ctx.spawn(
            async move {
                Timer::after(BROWSER_ELEMENT_PICKER_POLL_INTERVAL).await;
                browser_take_selected_element(port).await
            },
            |me, result, ctx| {
                if !me.browser_element_picker_enabled {
                    return;
                }

                match result {
                    Ok(Some(text)) => {
                        me.browser_element_picker_enabled = false;
                        ctx.emit(RightPanelEvent::AttachBrowserSelectionAsContext { text });
                        ctx.notify();
                    }
                    Ok(None) => {
                        me.poll_browser_element_selection(ctx);
                    }
                    Err(err) => {
                        log::warn!("failed to poll selected browser element: {err:?}");
                        me.poll_browser_element_selection(ctx);
                    }
                }
            },
        );
    }

    fn handle_browser_url_editor_event(
        &mut self,
        event: &EditorEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        if matches!(event, EditorEvent::Enter) {
            self.handle_action(&RightPanelAction::OpenBrowserCurrentUrl, ctx);
        }
    }

    #[cfg(feature = "local_fs")]
    pub fn update_session_env(
        &mut self,
        is_remote: bool,
        is_wsl: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        self.code_review_session_env = Some(CodeReviewSessionEnv { is_remote, is_wsl });
        ctx.notify();
    }

    pub fn selected_repo_path(&self) -> Option<&LocalOrRemotePath> {
        self.code_review_state
            .as_ref()
            .and_then(|s| s.selected_repo_path.as_ref())
    }

    #[cfg(feature = "local_fs")]
    pub fn update_selected_repo(
        &mut self,
        repo_path: LocalOrRemotePath,
        ctx: &mut ViewContext<Self>,
    ) {
        self.handle_action(
            &RightPanelAction::SelectRepo {
                repo_path,
                from_dropdown: false,
            },
            ctx,
        );
    }

    fn handle_working_directories_event(
        &mut self,
        event: &WorkingDirectoriesEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            #[cfg(feature = "local_fs")]
            WorkingDirectoriesEvent::DirectoriesChanged {
                pane_group_id,
                directories: _,
            } => {
                let Some(active_pane_group) = &self.active_pane_group else {
                    return;
                };
                if active_pane_group.id() != *pane_group_id {
                    return;
                }
                self.update_file_tree_for_active_pane_group(ctx);
            }
            WorkingDirectoriesEvent::RepositoriesChanged {
                pane_group_id,
                repositories,
            } => {
                let Some(active_pane_group) = &self.active_pane_group else {
                    return;
                };
                if active_pane_group.id() != *pane_group_id {
                    return;
                }
                let old_selected = self
                    .code_review_state
                    .as_ref()
                    .and_then(|s| s.selected_repo_path.clone());

                if let Some(state) = self.code_review_state.as_mut() {
                    state.set_available_repos(repositories.clone(), ctx);
                }

                let new_selected = self
                    .code_review_state
                    .as_ref()
                    .and_then(|s| s.selected_repo_path.clone());

                // Only close the old view if the selection actually changed.
                if old_selected != new_selected {
                    if let Some(old_path) = &old_selected {
                        self.close_code_review_view(*pane_group_id, old_path, ctx);
                    }
                }

                if let Some(path) = &new_selected {
                    self.ensure_code_review_view_exists(path, ctx);
                }

                self.recompute_terminal_availability(ctx);
                ctx.notify();
            }
            WorkingDirectoriesEvent::FocusedRepoChanged {
                pane_group_id,
                repository_terminal_map: _,
                focused_repo,
            } => {
                let Some(active_pane_group) = &self.active_pane_group else {
                    return;
                };
                if active_pane_group.id() != *pane_group_id {
                    return;
                }

                // When the focused terminal changes repos (via CD or pane focus),
                // update the dropdown to match the focused terminal's repo
                if let Some(state) = self.code_review_state.as_mut() {
                    state.set_focused_repo(focused_repo.clone(), ctx);
                }

                self.recompute_terminal_availability(ctx);
                ctx.notify();
            }
        }
    }

    #[cfg(feature = "local_fs")]
    fn handle_file_tree_event(&mut self, event: &FileTreeEvent, ctx: &mut ViewContext<Self>) {
        match event {
            FileTreeEvent::FileRenamed { old_path, new_path } => {
                ctx.emit(RightPanelEvent::FileTree(
                    RightPanelFileTreeEvent::FileRenamed {
                        old_path: old_path.clone(),
                        new_path: new_path.clone(),
                    },
                ));
            }
            FileTreeEvent::FileDeleted { path } => {
                ctx.emit(RightPanelEvent::FileTree(
                    RightPanelFileTreeEvent::FileDeleted { path: path.clone() },
                ));
            }
            FileTreeEvent::AttachAsContext { path } => {
                ctx.emit(RightPanelEvent::FileTree(
                    RightPanelFileTreeEvent::AttachPathAsContext { path: path.clone() },
                ));
            }
            FileTreeEvent::OpenFile {
                path,
                target,
                line_col,
            } => {
                ctx.emit(RightPanelEvent::OpenFileFromFileManager {
                    location: path.clone(),
                    target: target.clone(),
                    line_col: *line_col,
                });
            }
            FileTreeEvent::CDToDirectory { path } => {
                ctx.emit(RightPanelEvent::FileTree(
                    RightPanelFileTreeEvent::CDToDirectory { path: path.clone() },
                ));
            }
            FileTreeEvent::OpenDirectoryInNewTab { path } => {
                ctx.emit(RightPanelEvent::FileTree(
                    RightPanelFileTreeEvent::OpenDirectoryInNewTab { path: path.clone() },
                ));
            }
        }
    }

    #[cfg(feature = "local_fs")]
    fn get_or_create_file_tree_view_for_pane_group(
        &mut self,
        pane_group_id: EntityId,
        ctx: &mut ViewContext<Self>,
    ) -> ViewHandle<FileTreeView> {
        if let Some(view) = self.file_tree_views.get(&pane_group_id) {
            return view.clone();
        }

        let file_tree_view = ctx.add_typed_action_view(FileTreeView::new);
        ctx.subscribe_to_view(&file_tree_view, |me, _, event, ctx| {
            me.handle_file_tree_event(event, ctx);
        });
        self.file_tree_views
            .insert(pane_group_id, file_tree_view.clone());
        file_tree_view
    }

    #[cfg(feature = "local_fs")]
    fn update_file_tree_for_active_pane_group(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(pane_group) = self.active_pane_group.clone() else {
            return;
        };
        let pane_group_id = pane_group.id();
        let active_directories = self.working_directories_model.read(ctx, |model, _| {
            model
                .most_recent_directories_for_pane_group(pane_group_id)
                .map(|dirs| dirs.collect_vec())
                .unwrap_or_default()
        });
        let has_terminal_session = active_directories
            .iter()
            .any(|dir| dir.terminal_id.is_some());
        let local_paths = active_directories
            .iter()
            .filter_map(|dir| dir.path.to_local_path().map(Path::to_path_buf))
            .collect_vec();
        let local_directories = deduplicate_paths(local_paths);
        let remote_repos = active_directories
            .iter()
            .filter_map(|dir| match &dir.path {
                LocalOrRemotePath::Remote(remote_path) => {
                    Some(repo_metadata::RemoteRepositoryIdentifier::new(
                        remote_path.host_id.clone(),
                        remote_path.path.clone(),
                    ))
                }
                LocalOrRemotePath::Local(_) => None,
            })
            .collect_vec();
        let active_file_model = pane_group.as_ref(ctx).active_file_model().clone();
        let is_visible = self.file_tree_should_be_active(ctx);
        let file_tree_view = self.get_or_create_file_tree_view_for_pane_group(pane_group_id, ctx);

        file_tree_view.update(ctx, |view, ctx| {
            view.set_root_directories(local_directories, ctx);
            view.set_remote_root_directories(&remote_repos, ctx);
            view.set_has_terminal_session(has_terminal_session, ctx);
            view.set_active_file_model(active_file_model, ctx);
            view.set_is_active(is_visible, ctx);

            if is_visible {
                view.auto_expand_to_most_recent_directory(ctx);
            }
        });
    }

    #[cfg(feature = "local_fs")]
    pub(crate) fn set_active_file_tree_visible(
        &self,
        is_visible: bool,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(pane_group) = &self.active_pane_group else {
            return;
        };
        if let Some(file_tree_view) = self.file_tree_views.get(&pane_group.id()) {
            file_tree_view.update(ctx, |view, ctx| {
                view.set_is_active(is_visible, ctx);
            });
        }
    }

    #[cfg(feature = "local_fs")]
    fn file_tree_should_be_active(&self, app: &AppContext) -> bool {
        self.active_mode == RightPanelMode::Files
            && self
                .active_pane_group
                .as_ref()
                .is_some_and(|pane_group| pane_group.as_ref(app).right_panel_open)
    }

    #[cfg(feature = "local_fs")]
    fn active_file_tree_view(&self) -> Option<ViewHandle<FileTreeView>> {
        let pane_group_id = self.active_pane_group.as_ref()?.id();
        self.file_tree_views.get(&pane_group_id).cloned()
    }

    #[cfg(feature = "local_fs")]
    fn focus_active_file_tree(&self, ctx: &mut ViewContext<Self>) {
        if let Some(file_tree_view) = self.active_file_tree_view() {
            file_tree_view.update(ctx, |view, ctx| {
                view.on_left_panel_focused(ctx);
            });
            ctx.focus(&file_tree_view);
        }
    }

    pub fn set_active_pane_group(
        &mut self,
        pane_group: ViewHandle<PaneGroup>,
        working_directories_model: &ModelHandle<WorkingDirectoriesModel>,
        ctx: &mut ViewContext<Self>,
    ) {
        let pane_group_id = pane_group.id();

        // Subscribe to pane group events so we can recompute terminal
        // availability when terminal state changes (e.g. command
        // starts/finishes).
        ctx.subscribe_to_view(&pane_group, |me, _, event, ctx| {
            if matches!(event, PaneGroupEvent::TerminalViewStateChanged) {
                me.recompute_terminal_availability(ctx);
            }
        });

        self.active_pane_group = Some(pane_group);

        if let Some(state) = &mut self.code_review_state {
            let (active_repositories, saved_selection) =
                working_directories_model.read(ctx, |model, _| {
                    let repos: Vec<LocalOrRemotePath> = model
                        .most_recent_repositories_for_pane_group(pane_group_id)
                        .map(|repos| repos.collect())
                        .unwrap_or_default();
                    let saved = model.get_selected_review_repo(pane_group_id).cloned();
                    (repos, saved)
                });

            // Replace the carried-over selection from a different pane group
            // with whatever was saved for this pane group (if anything). This
            // ensures `set_available_repos` either keeps the saved selection
            // (when it's still in the repo list) or falls back to auto-selecting
            // the first repo, instead of preserving the previous tab's repo.
            state.selected_repo_path = saved_selection;
            state.set_available_repos(active_repositories, ctx);
        }

        let selected = self
            .code_review_state
            .as_ref()
            .and_then(|s| s.selected_repo_path.clone());

        if let Some(selected) = &selected {
            self.ensure_code_review_view_exists(selected, ctx);
        }

        #[cfg(feature = "local_fs")]
        self.update_file_tree_for_active_pane_group(ctx);

        let is_maximized = self.is_maximized(ctx);
        self.set_maximized(is_maximized, ctx);

        ctx.notify();
    }

    #[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
    /// Will only update repo_path if one is not already set
    pub fn open_code_review(
        &mut self,
        repo_path: Option<LocalOrRemotePath>,
        diff_state_model: ModelHandle<DiffStateModel>,
        terminal_view: WeakViewHandle<TerminalView>,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(repo_dropdown_state) = &mut self.code_review_state else {
            return;
        };
        let (Some(repo_path), Some(active_pane_group)) = (&repo_path, &self.active_pane_group)
        else {
            return;
        };
        if repo_path.is_remote() && !FeatureFlag::RemoteCodeReview.is_enabled() {
            return;
        }
        self.active_mode = RightPanelMode::CodeReview;
        let pane_group_id = active_pane_group.id();

        if repo_dropdown_state.selected_repo_path.is_none() {
            repo_dropdown_state.set_selected_repo(repo_path.clone(), ctx);
        }
        // Check if we already have a cached CodeReviewView
        let working_directories_model = self.working_directories_model.clone();
        let existing_view = working_directories_model
            .as_ref(ctx)
            .get_code_review_view(pane_group_id, repo_path);
        if let Some(view) = existing_view {
            view.update(ctx, |view, ctx| {
                view.set_terminal_view(terminal_view);
                view.on_open(ctx);
            });
            self.recompute_terminal_availability(ctx);
        } else if let Some(view) = self.create_code_review_view(
            repo_path,
            diff_state_model.clone(),
            pane_group_id,
            terminal_view.clone(),
            ctx,
        ) {
            view.update(ctx, |view, ctx| {
                view.on_open(ctx);
            });
            self.recompute_terminal_availability(ctx);
        };
        ctx.notify();
    }

    /// Closes the CodeReviewView for the given pane group and repo path (if any)
    /// by calling on_close. This stops event subscriptions and background work.
    fn close_code_review_view(
        &self,
        pane_group_id: EntityId,
        repo_path: &LocalOrRemotePath,
        ctx: &mut ViewContext<Self>,
    ) {
        if let Some(code_review_view) = self
            .working_directories_model
            .as_ref(ctx)
            .get_code_review_view(pane_group_id, repo_path)
        {
            code_review_view.update(ctx, |view, ctx| {
                view.on_close(ctx);
            });
        }
    }

    /// Closes the currently active CodeReviewView (if any) by calling on_close.
    fn close_active_code_review_view(&self, ctx: &mut ViewContext<Self>) {
        let Some(state) = &self.code_review_state else {
            return;
        };
        let (Some(repo_path), Some(pane_group)) =
            (&state.selected_repo_path, &self.active_pane_group)
        else {
            return;
        };
        self.close_code_review_view(pane_group.id(), repo_path, ctx);
    }

    #[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
    pub fn close_code_review(&mut self, ctx: &mut ViewContext<Self>) {
        self.close_active_code_review_view(ctx);

        // Views are cached in WorkingDirectoriesModel, so we just update the UI state
        if let Some(code_review_state) = &mut self.code_review_state {
            code_review_state.selected_repo_path = None;
        }
        ctx.notify();
    }

    pub fn hide_browser_host(&mut self, ctx: &mut ViewContext<Self>) {
        hide_external_browser_host();
        ctx.notify();
    }

    fn render_repo_dropdown(&self) -> Option<Box<dyn Element>> {
        let Some(state) = &self.code_review_state else {
            return None;
        };
        if state.available_repos.len() <= 1 {
            return None;
        }
        Some(
            Container::new(
                ConstrainedBox::new(ChildView::new(&state.dropdown).finish())
                    .with_max_width(300.)
                    .finish(),
            )
            .with_margin_right(4.)
            .finish(),
        )
    }

    fn close_button(&self, appearance: &Appearance, app: &AppContext) -> Box<dyn Element> {
        let ui_builder = appearance.ui_builder().clone();
        let tooltip_keybinding =
            keybinding_name_to_display_string(TOGGLE_RIGHT_PANEL_BINDING_NAME, app);

        let tooltip = if let Some(keybinding) = tooltip_keybinding {
            ui_builder
                .tool_tip_with_sublabel("Close panel".to_string(), keybinding)
                .build()
                .finish()
        } else {
            ui_builder
                .tool_tip("Close panel".to_string())
                .build()
                .finish()
        };

        let icon_color = appearance
            .theme()
            .sub_text_color(appearance.theme().background());
        icon_button_with_color(
            appearance,
            icons::Icon::X,
            false,
            self.close_button_mouse_state.clone(),
            icon_color,
        )
        .with_tooltip(move || tooltip)
        .build()
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(WorkspaceAction::ToggleRightPanel);
        })
        .with_cursor(Cursor::PointingHand)
        .finish()
    }

    fn render_mode_button(
        &self,
        mode: RightPanelMode,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let mouse_state = self.mode_button_mouse_states.handle(mode);
        let is_active = self.active_mode == mode;
        let icon_color = if is_active {
            theme.main_text_color(theme.background())
        } else {
            theme.sub_text_color(theme.background())
        };
        let font_family = appearance.ui_font_family();

        Hoverable::new(mouse_state, move |state| {
            let mut row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_main_axis_size(MainAxisSize::Min)
                .with_spacing(4.);
            row.add_child(
                ConstrainedBox::new(mode.icon().to_warpui_icon(icon_color).finish())
                    .with_width(14.)
                    .with_height(14.)
                    .finish(),
            );
            row.add_child(
                Text::new_inline(mode.label().to_string(), font_family, 12.)
                    .with_style(Properties::default().weight(Weight::Semibold))
                    .with_color(icon_color.into())
                    .finish(),
            );

            let mut container = Container::new(row.finish())
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
                .with_padding_left(8.)
                .with_padding_right(8.)
                .with_padding_top(3.)
                .with_padding_bottom(3.);

            if is_active {
                container = container.with_background(internal_colors::fg_overlay_3(theme));
            } else if state.is_hovered() {
                container = container.with_background(internal_colors::fg_overlay_2(theme));
            }

            container.finish()
        })
        .on_click(move |ctx, _, _| {
            ctx.dispatch_typed_action(RightPanelAction::SetMode(mode));
        })
        .with_cursor(Cursor::PointingHand)
        .finish()
    }

    fn render_mode_switcher(&self, appearance: &Appearance) -> Box<dyn Element> {
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Min)
            .with_spacing(4.)
            .with_child(self.render_mode_button(RightPanelMode::CodeReview, appearance))
            .with_child(self.render_mode_button(RightPanelMode::Files, appearance))
            .with_child(self.render_mode_button(RightPanelMode::Browser, appearance))
            .finish()
    }

    fn render_simple_header(
        &self,
        appearance: &Appearance,
        close_button: Box<dyn Element>,
    ) -> Box<dyn Element> {
        Container::new(
            ConstrainedBox::new(
                Flex::row()
                    .with_child(
                        Shrinkable::new(1.0, self.render_mode_switcher(appearance)).finish(),
                    )
                    .with_children(vec![close_button])
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .finish(),
            )
            .with_height(PANE_HEADER_HEIGHT)
            .finish(),
        )
        .with_padding_left(16.)
        .with_padding_right(HEADER_EDGE_PADDING)
        .finish()
    }

    fn render_panel_content(&self, app: &AppContext) -> Box<dyn Element> {
        match self.active_mode {
            RightPanelMode::CodeReview => self.render_code_review_panel_content(app),
            RightPanelMode::Files => self.render_files_panel_content(app),
            RightPanelMode::Browser => self.render_browser_panel_content(app),
        }
    }

    fn render_code_review_panel_content(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let close_button = self.close_button(appearance, app);

        let Some(state) = &self.code_review_state else {
            let simple_header = self.render_simple_header(appearance, close_button);
            return Flex::column()
                .with_child(simple_header)
                .with_child(
                    Shrinkable::new(1.0, CodeReviewView::render_loading_state(appearance)).finish(),
                )
                .finish();
        };

        let selected_repo_path = state.selected_repo_path.as_ref().filter(|repo_path| {
            if repo_path.is_remote() {
                FeatureFlag::RemoteCodeReview.is_enabled()
            } else {
                state.available_repos.contains(repo_path)
            }
        });

        let Some(selected_repo_path) = selected_repo_path else {
            let simple_header = self.render_simple_header(appearance, close_button);

            #[cfg(feature = "local_fs")]
            let no_repo_body = {
                let open_repo_button =
                    || Some(ChildView::new(&self.open_repository_button).finish());
                if let Some(env) = &self.code_review_session_env {
                    if env.is_remote {
                        // No "Open repository" CTA when the session is remote — the
                        // button navigates to a local folder, which is not meaningful
                        // in a remote session.
                        CodeReviewView::render_remote_state(appearance, None)
                    } else if env.is_wsl {
                        CodeReviewView::render_wsl_state(appearance, open_repo_button())
                    } else {
                        CodeReviewView::render_not_repo_state(appearance, open_repo_button())
                    }
                } else {
                    CodeReviewView::render_not_repo_state(appearance, open_repo_button())
                }
            };

            #[cfg(not(feature = "local_fs"))]
            let no_repo_body = CodeReviewView::render_not_repo_state(appearance, None);

            return Flex::column()
                .with_child(simple_header)
                .with_child(Shrinkable::new(1.0, no_repo_body).finish())
                .finish();
        };

        let current_code_review_view = self.active_pane_group.as_ref().and_then(|pane_group| {
            let pane_group_id = pane_group.id();
            self.working_directories_model
                .as_ref(app)
                .get_code_review_view(pane_group_id, selected_repo_path)
        });

        if let Some(code_review_view) = current_code_review_view {
            let header = if FeatureFlag::GitOperationsInCodeReview.is_enabled() {
                self.render_header(&code_review_view, appearance, app)
            } else {
                self.render_header_legacy(appearance, app)
            };
            let code_review_content =
                Shrinkable::new(1.0, ChildView::new(&code_review_view).finish()).finish();

            Flex::column()
                .with_child(header)
                .with_child(code_review_content)
                .finish()
        } else {
            let simple_header = self.render_simple_header(appearance, close_button);
            Flex::column()
                .with_child(simple_header)
                .with_child(
                    Shrinkable::new(1.0, CodeReviewView::render_loading_state(appearance)).finish(),
                )
                .finish()
        }
    }

    fn render_files_panel_content(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let header = self.render_simple_header(appearance, self.close_button(appearance, app));

        #[cfg(feature = "local_fs")]
        let body = if let Some(file_tree_view) = self.active_file_tree_view() {
            Shrinkable::new(
                1.0,
                Container::new(ChildView::new(&file_tree_view).finish())
                    .with_padding_left(2.)
                    .with_padding_right(2.)
                    .finish(),
            )
            .finish()
        } else {
            self.render_empty_panel_state(
                appearance,
                Icon::FolderClosed,
                "No project folder",
                "Open a terminal in a project to manage files here.",
            )
        };

        #[cfg(not(feature = "local_fs"))]
        let body = self.render_empty_panel_state(
            appearance,
            Icon::FolderClosed,
            "Files unavailable",
            "File management requires local filesystem support.",
        );

        Flex::column()
            .with_child(header)
            .with_child(Shrinkable::new(1.0, body).finish())
            .finish()
    }

    fn render_browser_panel_content(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let header = self.render_simple_header(appearance, self.close_button(appearance, app));

        let url_input = appearance
            .ui_builder()
            .text_input(self.browser_url_editor.clone())
            .with_style(UiComponentStyles {
                background: Some(theme.background().into()),
                border_color: Some(theme.outline().into()),
                border_width: Some(1.),
                border_radius: Some(CornerRadius::with_all(Radius::Pixels(6.))),
                padding: Some(Coords {
                    top: 7.,
                    bottom: 7.,
                    left: 8.,
                    right: 8.,
                }),
                ..Default::default()
            })
            .build()
            .finish();

        let address_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(4.)
            .with_child(self.render_browser_icon_button(&self.browser_back_button))
            .with_child(self.render_browser_icon_button(&self.browser_forward_button))
            .with_child(self.render_browser_icon_button(&self.browser_refresh_button))
            .with_child(Shrinkable::new(1.0, url_input).finish())
            .with_child(self.render_browser_icon_button(&self.browser_element_picker_button))
            .with_child(self.render_browser_icon_button(&self.browser_copy_button))
            .with_child(self.render_browser_icon_button(&self.browser_open_button))
            .finish();

        let browser_surface = self.render_browser_surface(appearance);

        Flex::column()
            .with_child(header)
            .with_child(
                Shrinkable::new(
                    1.0,
                    Container::new(
                        Flex::column()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_spacing(12.)
                            .with_child(address_row)
                            .with_child(Shrinkable::new(1.0, browser_surface).finish())
                            .finish(),
                    )
                    .with_padding_top(12.)
                    .with_padding_left(12.)
                    .with_padding_right(12.)
                    .finish(),
                )
                .finish(),
            )
            .finish()
    }

    fn render_browser_surface(&self, appearance: &Appearance) -> Box<dyn Element> {
        let theme = appearance.theme();
        if let Some(url) = self
            .browser_current_url
            .as_deref()
            .filter(|url| *url != "about:blank")
        {
            return self.render_external_browser_surface(appearance, url.to_string());
        }

        match &self.browser_load_state {
            BrowserLoadState::Blank => self.render_browser_empty_state(
                appearance,
                Icon::Globe4,
                "浏览器",
                "粘贴或输入 URL 以打开网页。",
                true,
            ),
            BrowserLoadState::Loading { url } => self.render_browser_empty_state(
                appearance,
                Icon::Loading,
                "Loading",
                url.as_str(),
                false,
            ),
            BrowserLoadState::Error { url, message } => self.render_browser_empty_state(
                appearance,
                Icon::AlertTriangle,
                message,
                url,
                false,
            ),
            BrowserLoadState::Loaded(document) => {
                Container::new(self.render_browser_document(document, appearance))
                    .with_border(Border::all(1.).with_border_fill(theme.outline()))
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
                    .finish()
            }
        }
    }

    fn render_external_browser_surface(
        &self,
        appearance: &Appearance,
        url: String,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        LiveElement::new(
            ExternalBrowserSurfaceElement::new(
                self.window_id,
                Some(url),
                theme.background().into(),
            )
            .finish(),
            BROWSER_HOST_REPAINT_INTERVAL,
        )
        .finish()
    }

    fn render_browser_empty_state(
        &self,
        appearance: &Appearance,
        icon: Icon,
        title: &str,
        message: &str,
        is_blank: bool,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let sub_text_color = theme.sub_text_color(theme.background());
        let main_text_color = theme.main_text_color(theme.background());
        let icon_size = if is_blank { 88. } else { 52. };
        let title_size = if is_blank { 32. } else { 13. };
        let message_size = if is_blank { 18. } else { 12. };
        let title_weight = if is_blank {
            Weight::Bold
        } else {
            Weight::Semibold
        };

        Align::new(
            Flex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(if is_blank { 20. } else { 10. })
                .with_child(
                    ConstrainedBox::new(icon.to_warpui_icon(sub_text_color).finish())
                        .with_width(icon_size)
                        .with_height(icon_size)
                        .finish(),
                )
                .with_child(
                    Text::new(title.to_string(), appearance.ui_font_family(), title_size)
                        .soft_wrap(true)
                        .with_style(Properties::default().weight(title_weight))
                        .with_color(main_text_color.into())
                        .finish(),
                )
                .with_child(
                    Container::new(
                        Text::new(
                            message.to_string(),
                            appearance.ui_font_family(),
                            message_size,
                        )
                        .soft_wrap(true)
                        .with_color(sub_text_color.into())
                        .finish(),
                    )
                    .with_horizontal_padding(24.)
                    .finish(),
                )
                .finish(),
        )
        .finish()
    }

    fn render_browser_document(
        &self,
        document: &BrowserDocument,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let sub_text_color = theme.sub_text_color(theme.background());
        let main_text_color = theme.main_text_color(theme.background());
        let title = document.title.as_deref().unwrap_or("Untitled page");
        let picker_text = if self.browser_element_picker_enabled {
            "Element picker active"
        } else {
            "Element picker idle"
        };

        let mut content = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_spacing(8.)
            .with_child(
                Container::new(
                    Flex::column()
                        .with_spacing(3.)
                        .with_child(
                            Text::new(title.to_string(), appearance.ui_font_family(), 14.)
                                .soft_wrap(true)
                                .with_style(Properties::default().weight(Weight::Semibold))
                                .with_color(main_text_color.into())
                                .finish(),
                        )
                        .with_child(
                            Text::new(document.url.clone(), appearance.ui_font_family(), 12.)
                                .soft_wrap(true)
                                .with_color(sub_text_color.into())
                                .finish(),
                        )
                        .with_child(
                            Text::new(
                                format!("{picker_text} - {} elements", document.elements.len()),
                                appearance.ui_font_family(),
                                12.,
                            )
                            .soft_wrap(true)
                            .with_color(sub_text_color.into())
                            .finish(),
                        )
                        .finish(),
                )
                .with_padding_left(10.)
                .with_padding_right(10.)
                .with_padding_top(10.)
                .finish(),
            );

        if document.elements.is_empty() {
            content.add_child(
                Container::new(
                    Text::new(
                        "No selectable elements found.".to_string(),
                        appearance.ui_font_family(),
                        12.,
                    )
                    .soft_wrap(true)
                    .with_color(sub_text_color.into())
                    .finish(),
                )
                .with_uniform_padding(10.)
                .finish(),
            );
        } else {
            for (index, element) in document.elements.iter().enumerate() {
                let mouse_state = self
                    .browser_element_mouse_states
                    .get(index)
                    .cloned()
                    .unwrap_or_default();
                content.add_child(self.render_browser_element_row(
                    index,
                    element,
                    mouse_state,
                    appearance,
                ));
            }
        }

        ClippedScrollable::vertical(
            self.browser_scroll_state.clone(),
            content.finish(),
            ScrollbarWidth::Auto,
            internal_colors::fg_overlay_2(theme).into(),
            internal_colors::fg_overlay_3(theme).into(),
            internal_colors::fg_overlay_1(theme).into(),
        )
        .finish()
    }

    fn render_browser_element_row(
        &self,
        index: usize,
        element: &BrowserElementCandidate,
        mouse_state: MouseStateHandle,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let font_family = appearance.ui_font_family();
        let picker_enabled = self.browser_element_picker_enabled;
        let tag = element.tag.clone();
        let selector = element.selector.clone();
        let text = element.text.clone();
        let attrs = element
            .attributes
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .join("  ");

        Hoverable::new(mouse_state, move |state| {
            let title_color = theme.main_text_color(theme.background());
            let sub_text_color = theme.sub_text_color(theme.background());

            let mut row = Flex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_spacing(5.)
                .with_child(
                    Flex::row()
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_spacing(6.)
                        .with_child(
                            Text::new_inline(format!("<{tag}>"), font_family, 12.)
                                .with_style(Properties::default().weight(Weight::Semibold))
                                .with_color(title_color.into())
                                .finish(),
                        )
                        .with_child(
                            Shrinkable::new(
                                1.0,
                                Clipped::new(
                                    Text::new_inline(selector.clone(), font_family, 12.)
                                        .with_color(sub_text_color.into())
                                        .finish(),
                                )
                                .finish(),
                            )
                            .finish(),
                        )
                        .finish(),
                )
                .with_child(
                    Text::new(text.clone(), font_family, 12.)
                        .soft_wrap(true)
                        .with_color(title_color.into())
                        .finish(),
                );

            if !attrs.is_empty() {
                row.add_child(
                    Text::new(attrs.clone(), font_family, 11.)
                        .soft_wrap(true)
                        .with_color(sub_text_color.into())
                        .finish(),
                );
            }

            let mut container = Container::new(row.finish())
                .with_margin_left(10.)
                .with_margin_right(10.)
                .with_margin_bottom(6.)
                .with_uniform_padding(9.)
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                .with_border(Border::all(1.).with_border_fill(theme.outline()));

            if picker_enabled {
                container = container
                    .with_background(internal_colors::fg_overlay_2(theme))
                    .with_border(Border::all(1.).with_border_fill(theme.active_ui_text_color()));
            } else if state.is_hovered() {
                container = container.with_background(internal_colors::fg_overlay_1(theme));
            }

            container.finish()
        })
        .on_click(move |ctx, _, _| {
            if picker_enabled {
                ctx.dispatch_typed_action(RightPanelAction::AttachBrowserElement { index });
            }
        })
        .with_cursor(Cursor::PointingHand)
        .finish()
    }

    fn render_empty_panel_state(
        &self,
        appearance: &Appearance,
        icon: Icon,
        title: &str,
        message: &str,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let sub_text_color = theme.sub_text_color(theme.background());
        Flex::column()
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.)
            .with_child(
                ConstrainedBox::new(icon.to_warpui_icon(sub_text_color).finish())
                    .with_width(36.)
                    .with_height(36.)
                    .finish(),
            )
            .with_child(
                Text::new_inline(title.to_string(), appearance.ui_font_family(), 13.)
                    .with_style(Properties::default().weight(Weight::Semibold))
                    .with_color(theme.main_text_color(theme.background()).into())
                    .finish(),
            )
            .with_child(
                Text::new_inline(message.to_string(), appearance.ui_font_family(), 12.)
                    .with_color(sub_text_color.into())
                    .finish(),
            )
            .finish()
    }

    fn render_browser_icon_button(&self, button: &ViewHandle<ActionButton>) -> Box<dyn Element> {
        ConstrainedBox::new(ChildView::new(button).finish())
            .with_width(warp_core::ui::icons::ICON_DIMENSIONS)
            .with_height(warp_core::ui::icons::ICON_DIMENSIONS)
            .finish()
    }

    #[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
    fn render_maximize_pane_button(&self) -> Box<dyn Element> {
        ConstrainedBox::new(ChildView::new(&self.maximize_button).finish())
            .with_height(warp_core::ui::icons::ICON_DIMENSIONS)
            .with_width(warp_core::ui::icons::ICON_DIMENSIONS)
            .finish()
    }

    fn render_header(
        &self,
        code_review_view: &ViewHandle<CodeReviewView>,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let sub_text_color = theme.sub_text_color(theme.background());

        let crv = code_review_view.as_ref(app);
        let repo_path = crv.repo_path();
        let branch_name = crv
            .diff_state_model()
            .read(app, |model, ctx| model.get_current_branch_name(ctx));
        let diff_stats = crv.loaded_diff_stats();

        let repo_path_element = repo_path.map(|repo_path| {
            let display_path = display_path_with_host(repo_path, true, app);
            Container::new(
                Text::new_inline(
                    format!("{display_path}:"),
                    appearance.ui_font_family(),
                    appearance.ui_font_size(),
                )
                .with_style(Properties::default().weight(Weight::Semibold))
                .with_color(sub_text_color.into())
                .finish(),
            )
            .with_margin_right(4.)
            .finish()
        });

        let branch_name_element = branch_name.map(|name| {
            Container::new(
                Text::new_inline(name, appearance.ui_font_family(), appearance.ui_font_size())
                    .with_style(Properties::default().weight(Weight::Semibold))
                    .with_color(sub_text_color.into())
                    .finish(),
            )
            .with_margin_right(8.)
            .finish()
        });

        let stats_element =
            diff_stats.map(|stats| CodeReviewView::render_diff_stats(&stats, appearance));

        let close_button = self.close_button(appearance, app);

        let mut left_section = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Max);
        left_section.add_child(
            Container::new(self.render_mode_switcher(appearance))
                .with_margin_right(8.)
                .finish(),
        );
        if let Some(repo_path_el) = repo_path_element {
            left_section.add_child(repo_path_el);
        }
        if let Some(branch_el) = branch_name_element {
            left_section.add_child(Shrinkable::new(100.0, branch_el).finish());
        }
        if let Some(stats) = stats_element {
            left_section.add_child(stats);
        }

        let mut right_section = Vec::new();
        if let Some(repo_dropdown) = self.render_repo_dropdown() {
            right_section.push(repo_dropdown);
        }
        right_section.push(self.render_maximize_pane_button());
        right_section.push(close_button);

        Container::new(
            ConstrainedBox::new(
                Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Clipped::new(Shrinkable::new(1.0, left_section.finish()).finish()).finish(),
                    )
                    .with_children(right_section)
                    .finish(),
            )
            .with_height(PANE_HEADER_HEIGHT)
            .finish(),
        )
        .with_padding_left(CONTENT_LEFT_MARGIN)
        .with_padding_right(CONTENT_RIGHT_MARGIN)
        .finish()
    }

    /// Legacy header layout: "Code review" title + file nav button.
    fn render_header_legacy(&self, appearance: &Appearance, app: &AppContext) -> Box<dyn Element> {
        let file_navigation_button = {
            let current_code_review_view = self
                .code_review_state
                .as_ref()
                .and_then(|state| state.selected_repo_path.as_ref())
                .and_then(|repo_path| {
                    self.active_pane_group.as_ref().and_then(|pane_group| {
                        let pane_group_id = pane_group.id();
                        self.working_directories_model
                            .as_ref(app)
                            .get_code_review_view(pane_group_id, repo_path)
                    })
                });

            let has_files = current_code_review_view
                .as_ref()
                .map(|view: &ViewHandle<CodeReviewView>| view.as_ref(app).has_file_states())
                .unwrap_or(false);

            let file_sidebar_expanded = current_code_review_view
                .as_ref()
                .map(|view| view.as_ref(app).file_sidebar_expanded())
                .unwrap_or(false);

            if has_files {
                Some(render_file_navigation_button(
                    appearance,
                    file_sidebar_expanded,
                    self.file_navigation_button_mouse_state.clone(),
                    |ctx| {
                        ctx.dispatch_typed_action(RightPanelAction::ToggleFileSidebar);
                    },
                ))
            } else {
                None
            }
        };

        let theme = appearance.theme();
        let sub_text_color = theme.sub_text_color(theme.background());

        let title = Shrinkable::new(
            1.0,
            Text::new_inline("Code review".to_string(), appearance.ui_font_family(), 12.)
                .with_style(Properties::default().weight(Weight::Bold))
                .with_color(sub_text_color.into())
                .finish(),
        )
        .finish();

        let close_button = self.close_button(appearance, app);

        let mut left_section = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Max);
        let has_nav_button = file_navigation_button.is_some();
        if let Some(nav_button) = file_navigation_button {
            left_section.add_child(nav_button);
        }
        left_section.add_child(
            Container::new(self.render_mode_switcher(appearance))
                .with_margin_right(8.)
                .finish(),
        );
        left_section.add_child(title);

        let mut right_section = Vec::new();
        if let Some(repo_dropdown) = self.render_repo_dropdown() {
            right_section.push(repo_dropdown);
        }
        right_section.push(self.render_maximize_pane_button());
        right_section.push(close_button);

        let left_padding = if has_nav_button { 12. } else { 16. };

        Container::new(
            ConstrainedBox::new(
                Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(Box::new(Shrinkable::new(1.0, left_section.finish())))
                    .with_children(right_section)
                    .finish(),
            )
            .with_height(PANE_HEADER_HEIGHT)
            .finish(),
        )
        .with_padding_left(left_padding)
        .with_padding_right(HEADER_EDGE_PADDING)
        .finish()
    }

    pub fn set_maximized(&mut self, is_maximized: bool, ctx: &mut ViewContext<Self>) {
        let (icon, tooltip) = if is_maximized {
            (Icon::Minimize, "Minimize")
        } else {
            (Icon::Maximize, "Maximize")
        };

        self.maximize_button.update(ctx, |button, ctx| {
            let mut new_button = ActionButton::new("", PaneHeaderTheme)
                .with_icon(icon)
                .with_tooltip(tooltip)
                .with_tooltip_positioning_provider(Arc::new(MenuPositioning::BelowInputBox))
                .on_click(|ctx| {
                    ctx.dispatch_typed_action(RightPanelAction::ToggleMaximize);
                });

            if let Some(keybinding_label) = keybinding_name_to_display_string(
                "workspace:toggle_maximize_code_review_panel",
                ctx,
            ) {
                new_button = new_button.with_tooltip_sublabel(keybinding_label);
            }

            *button = new_button;
            ctx.notify();
        });

        // Propagate maximize state to the active code review view's file sidebar
        if let Some(code_review_view) = self.get_active_code_review_view(ctx) {
            code_review_view.update(ctx, |view, ctx| {
                view.handle_maximization_toggle(is_maximized, ctx);
            });
        }
    }

    #[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
    pub fn focus_active_code_review_view(&self, ctx: &mut ViewContext<Self>) {
        let Some(state) = &self.code_review_state else {
            return;
        };
        let Some(selected_repo_path) = &state.selected_repo_path else {
            return;
        };
        let Some(active_pane_group) = &self.active_pane_group else {
            return;
        };
        let pane_group_id = active_pane_group.id();
        if let Some(code_review_view) = self
            .working_directories_model
            .as_ref(ctx)
            .get_code_review_view(pane_group_id, selected_repo_path)
        {
            ctx.focus(&code_review_view);
        }
    }

    fn get_active_code_review_view(&self, ctx: &AppContext) -> Option<ViewHandle<CodeReviewView>> {
        let state = self.code_review_state.as_ref()?;
        let selected_repo_path = state.selected_repo_path.as_ref()?;
        let active_pane_group = self.active_pane_group.as_ref()?;
        let pane_group_id = active_pane_group.id();
        self.working_directories_model
            .as_ref(ctx)
            .get_code_review_view(pane_group_id, selected_repo_path)
    }

    fn is_maximized(&self, app: &AppContext) -> bool {
        self.active_pane_group
            .as_ref()
            .map(|pane_group| pane_group.as_ref(app).is_right_panel_maximized)
            .unwrap_or(false)
    }

    fn create_code_review_view(
        &self,
        repo_path: &LocalOrRemotePath,
        diff_state_model: ModelHandle<DiffStateModel>,
        pane_group_id: EntityId,
        terminal_view: WeakViewHandle<TerminalView>,
        ctx: &mut ViewContext<Self>,
    ) -> Option<ViewHandle<CodeReviewView>> {
        // Early check: if pane group has no active repositories, don't create a view.
        // Remote repos require the RemoteCodeReview feature flag; local repos go
        // through the active-repos check.
        let has_active_repos = if repo_path.is_remote() {
            FeatureFlag::RemoteCodeReview.is_enabled()
        } else {
            self.working_directories_model
                .as_ref(ctx)
                .most_recent_repositories_for_pane_group(pane_group_id)
                .is_some_and(|mut repos| repos.any(|r| &r == repo_path))
        };

        if !has_active_repos {
            return None;
        }

        let diff_state_model_clone = diff_state_model.clone();
        let code_review_comment_batch =
            self.working_directories_model
                .update(ctx, |working_directories, ctx| {
                    working_directories.get_or_create_code_review_comments(repo_path, ctx)
                });
        let code_review_view = ctx.add_typed_action_view(|ctx| {
            CodeReviewView::new(
                Some(repo_path.clone()),
                diff_state_model_clone,
                code_review_comment_batch,
                Some(terminal_view),
                ctx,
            )
        });

        // Store in cache
        self.working_directories_model.update(ctx, |model, _ctx| {
            model.store_code_review_view(
                pane_group_id,
                repo_path.clone(),
                code_review_view.clone(),
            );
        });

        ctx.subscribe_to_model(&diff_state_model, |_me, _, _event, ctx| {
            ctx.notify();
        });

        ctx.subscribe_to_view(&code_review_view, |me, code_review, event, ctx| {
            match event {
                CodeReviewViewEvent::ReviewSubmitted => {
                    if me.is_maximized(ctx) {
                        me.handle_action(&RightPanelAction::ToggleMaximize, ctx);
                    }
                }
                CodeReviewViewEvent::SubmitReviewComments {
                    comments,
                    repo_path,
                } => {
                    Self::route_review_comments(me, &code_review, comments.clone(), repo_path, ctx);
                }
                #[cfg(feature = "local_fs")]
                CodeReviewViewEvent::OpenFileWithTarget {
                    path,
                    target,
                    line_col,
                } => {
                    ctx.emit(RightPanelEvent::OpenFileWithTarget {
                        path: path.clone(),
                        target: target.clone(),
                        line_col: *line_col,
                    });
                }
                CodeReviewViewEvent::OpenFileInNewTab {
                    path,
                    line_and_column,
                } => {
                    ctx.emit(RightPanelEvent::OpenFileInNewTab {
                        path: path.clone(),
                        line_and_column: *line_and_column,
                    });
                }
                #[cfg(not(target_family = "wasm"))]
                CodeReviewViewEvent::OpenLspLogs { log_path } => {
                    ctx.emit(RightPanelEvent::OpenLspLogs {
                        log_path: log_path.clone(),
                    });
                }
                _ => {}
            }
            ctx.notify();
        });

        Some(code_review_view)
    }

    /// Routes review comments to the best available terminal.
    /// Tries the preferred terminal first, then falls back to other terminals
    /// in the same repo working directory.
    fn route_review_comments(
        &mut self,
        code_review_view: &ViewHandle<CodeReviewView>,
        comments: AgentReviewCommentBatch,
        repo_path: &LocalOrRemotePath,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(pane_group) = &self.active_pane_group else {
            code_review_view.update(ctx, |view, ctx| {
                view.handle_review_submission_result(ReviewSubmissionResult::Error, ctx);
            });
            return;
        };

        let ai_enabled = AISettings::as_ref(ctx).is_any_ai_enabled(ctx);
        let chosen = self.find_review_terminal(pane_group, repo_path, ai_enabled, ctx);

        let Some(terminal_view) = chosen else {
            log::warn!("No available terminal found for submitting review comments");
            code_review_view.update(ctx, |view, ctx| {
                view.handle_review_submission_result(ReviewSubmissionResult::Error, ctx);
            });
            return;
        };

        let comment_count = comments.comments.len();
        let file_count = comments
            .comments
            .iter()
            .filter_map(|c| {
                c.target
                    .absolute_file_path()
                    .map(LocalOrRemotePath::display_path)
            })
            .collect::<std::collections::HashSet<_>>()
            .len();

        let active_cli_agent = terminal_view.read(ctx, |t, ctx| t.active_cli_agent(ctx));

        let (result, destination) = if active_cli_agent.is_some() {
            let r = terminal_view.update(ctx, |terminal, ctx| {
                terminal.send_review_to_cli_agent_or_rich_input(&comments, ctx)
            });
            let dest = if terminal_view.read(ctx, |t, ctx| t.is_cli_agent_rich_input_open(ctx)) {
                CodeReviewContextDestination::RichInput
            } else {
                CodeReviewContextDestination::Pty
            };
            (r, dest)
        } else {
            let r = terminal_view.update(ctx, |terminal, ctx| {
                terminal.send_inline_review(comments, ctx)
            });
            (r, CodeReviewContextDestination::AgentReview)
        };

        if let Err(err) = &result {
            log::error!("Failed to submit review comments to terminal: {err}");
        }

        let submission_result = if result.is_ok() {
            ReviewSubmissionResult::Success {
                comment_count,
                file_count,
                destination,
            }
        } else {
            ReviewSubmissionResult::Error
        };

        code_review_view.update(ctx, |view, ctx| {
            view.handle_review_submission_result(submission_result, ctx);
        });
    }

    fn format_optional_path(path: Option<&Path>) -> String {
        path.map(|path| path.display().to_string())
            .unwrap_or_else(|| "<none>".to_string())
    }

    fn format_optional_location(path: Option<&LocalOrRemotePath>) -> String {
        path.map(LocalOrRemotePath::display_path)
            .unwrap_or_else(|| "<none>".to_string())
    }

    fn review_terminal_status(
        tv: &ViewHandle<TerminalView>,
        repo_path: Option<&LocalOrRemotePath>,
        ai_enabled: bool,
        ctx: &AppContext,
    ) -> ReviewTerminalStatus {
        tv.read(ctx, |t, ctx| {
            let active_session_path = t.active_session_path_if_local(ctx);
            let current_repo_path = t.current_repo_path().cloned();
            let active_cli_agent = t.active_cli_agent(ctx).map(|agent| format!("{agent:?}"));
            let model = t.model.lock();
            let is_executing = model.block_list().active_block().is_executing();
            let is_input_box_visible = t.is_input_box_visible(&model, ctx);
            let mut unavailable_reasons = Vec::new();

            match repo_path {
                Some(repo_path) => match (repo_path, t.current_repo_path()) {
                    (LocalOrRemotePath::Local(repo_path), _) => {
                        match active_session_path.as_ref() {
                            Some(cwd)
                                if canonicalize(cwd)
                                    .as_deref()
                                    .unwrap_or(cwd)
                                    .starts_with(repo_path) => {}
                            Some(_) => unavailable_reasons
                                .push(ReviewTerminalUnavailableReason::SessionOutsideSelectedRepo),
                            None => unavailable_reasons
                                .push(ReviewTerminalUnavailableReason::SessionPathUnavailable),
                        }
                    }
                    (repo_path @ LocalOrRemotePath::Remote(_), Some(current_repo_path))
                        if repo_path.strip_repo_prefix(current_repo_path).is_some() => {}
                    (LocalOrRemotePath::Remote(_), Some(_)) => unavailable_reasons
                        .push(ReviewTerminalUnavailableReason::SessionOutsideSelectedRepo),
                    (LocalOrRemotePath::Remote(_), None) => unavailable_reasons
                        .push(ReviewTerminalUnavailableReason::SessionPathUnavailable),
                },
                None => unavailable_reasons.push(ReviewTerminalUnavailableReason::NoSelectedRepo),
            }

            if active_cli_agent.is_none() {
                if !ai_enabled {
                    unavailable_reasons.push(ReviewTerminalUnavailableReason::AIDisabled);
                }
                if is_executing {
                    unavailable_reasons.push(ReviewTerminalUnavailableReason::TerminalExecuting);
                }
                if !is_input_box_visible {
                    unavailable_reasons.push(ReviewTerminalUnavailableReason::InputBoxNotVisible);
                }
            }

            ReviewTerminalStatus {
                active_session_path,
                current_repo_path,
                active_cli_agent,
                is_executing,
                is_input_box_visible,
                unavailable_reasons,
            }
        })
    }

    fn log_code_review_debug_state(debug_state: &CodeReviewCommentDebugState) {
        log::info!(
            "Active code review view: repo_path={}, has_active_comment_model={}, review_destination={:?}, total_comments={}, sendable_comments={}, is_collapsed={}, is_outdated_section_collapsed={:?}, ai_available={}, ai_enabled={}, send_button_tooltip={}",
            Self::format_optional_location(debug_state.repo_path.as_ref()),
            debug_state.has_active_comment_model,
            debug_state.comment_list.review_destination,
            debug_state.comment_list.total_comments,
            debug_state.comment_list.sendable_comments,
            debug_state.comment_list.is_collapsed,
            debug_state.comment_list.is_outdated_section_collapsed,
            debug_state.comment_list.ai_available,
            debug_state.comment_list.ai_enabled,
            debug_state.comment_list.send_button_tooltip_text,
        );
    }

    pub fn log_review_comment_send_status_for_active_tab(&self, ctx: &AppContext) {
        let selected_repo_path = self.selected_repo_path().cloned();
        let ai_enabled = AISettings::as_ref(ctx).is_any_ai_enabled(ctx);
        let code_review_debug_state =
            self.get_active_code_review_view(ctx)
                .map(|code_review_view| {
                    code_review_view.read(ctx, |view, ctx| view.debug_review_comment_state(ctx))
                });

        let Some(pane_group) = &self.active_pane_group else {
            log::info!(
                "Review comment send status for active tab: no active pane group, selected_repo_path={}, ai_enabled={}",
                Self::format_optional_location(selected_repo_path.as_ref()),
                ai_enabled,
            );
            if let Some(debug_state) = &code_review_debug_state {
                Self::log_code_review_debug_state(debug_state);
            }
            return;
        };

        let pane_group_id = pane_group.id();
        let visible_pane_ids = pane_group.read(ctx, |pane_group, _| pane_group.visible_pane_ids());
        let focused_pane_id =
            pane_group.read(ctx, |pane_group, ctx| pane_group.focused_pane_id(ctx));
        let preferred_terminal_id = selected_repo_path.as_ref().and_then(|repo_path| {
            self.working_directories_model
                .as_ref(ctx)
                .get_terminal_id_for_root_path(pane_group_id, repo_path)
        });
        let chosen_terminal_id = selected_repo_path.as_ref().and_then(|repo_path| {
            self.find_review_terminal(pane_group, repo_path, ai_enabled, ctx)
                .map(|terminal_view| terminal_view.id())
        });

        log::info!(
            "Review comment send status for active tab: pane_group_id={pane_group_id}, selected_repo_path={}, ai_enabled={}, focused_pane_id={focused_pane_id}, preferred_terminal_id={preferred_terminal_id:?}, chosen_terminal_id={chosen_terminal_id:?}, visible_pane_count={}",
            Self::format_optional_location(selected_repo_path.as_ref()),
            ai_enabled,
            visible_pane_ids.len(),
        );

        if let Some(debug_state) = &code_review_debug_state {
            Self::log_code_review_debug_state(debug_state);
        } else {
            log::info!(
                "No active code review view is associated with the current tab/repo selection"
            );
        }

        for (index, pane_id) in visible_pane_ids.iter().enumerate() {
            let is_focused = *pane_id == focused_pane_id;
            if !pane_id.is_terminal_pane() {
                log::info!(
                    "Pane #{index}: pane_id={pane_id}, pane_type={}, focused={is_focused}, skipped=not a terminal pane",
                    pane_id.pane_type(),
                );
                continue;
            }

            let terminal_view = pane_group.read(ctx, |pane_group, ctx| {
                pane_group.terminal_view_from_pane_id(*pane_id, ctx)
            });
            let Some(terminal_view) = terminal_view else {
                log::info!(
                    "Pane #{index}: pane_id={pane_id}, pane_type={}, focused={is_focused}, skipped=terminal view missing",
                    pane_id.pane_type(),
                );
                continue;
            };

            let terminal_id = terminal_view.id();
            let terminal_status = Self::review_terminal_status(
                &terminal_view,
                selected_repo_path.as_ref(),
                ai_enabled,
                ctx,
            );
            let unavailable_reasons = if terminal_status.unavailable_reasons.is_empty() {
                "<none>".to_string()
            } else {
                terminal_status
                    .unavailable_reasons
                    .iter()
                    .map(ReviewTerminalUnavailableReason::label)
                    .join("; ")
            };

            log::info!(
                "Pane #{index}: pane_id={pane_id}, pane_type={}, terminal_view_id={terminal_id}, focused={is_focused}, preferred={}, chosen={}, available={}, active_session_path={}, current_repo_path={}, active_cli_agent={}, is_executing={}, is_input_box_visible={}, unavailable_reasons={}",
                pane_id.pane_type(),
                preferred_terminal_id == Some(terminal_id),
                chosen_terminal_id == Some(terminal_id),
                terminal_status.is_available(),
                Self::format_optional_path(terminal_status.active_session_path.as_deref()),
                Self::format_optional_location(terminal_status.current_repo_path.as_ref()),
                terminal_status
                    .active_cli_agent
                    .as_deref()
                    .unwrap_or("<none>"),
                terminal_status.is_executing,
                terminal_status.is_input_box_visible,
                unavailable_reasons,
            );
        }
    }

    /// Returns whether a terminal is in the given repo and available to receive
    /// review comments. A terminal is available if it is not executing a command
    /// and has its input box visible, OR if it has an active CLI agent
    /// (CLI agents are long-running commands that accept review input).
    ///
    /// When `ai_enabled` is `false`, only terminals with an active CLI agent are
    /// considered available (non-CLI Warp terminals require AI to be on).
    fn is_terminal_available_for_review(
        tv: &ViewHandle<TerminalView>,
        repo_path: &LocalOrRemotePath,
        ai_enabled: bool,
        ctx: &AppContext,
    ) -> bool {
        Self::review_terminal_status(tv, Some(repo_path), ai_enabled, ctx).is_available()
    }

    /// Finds the best terminal to send review comments to.
    /// Priority: focused terminal > preferred terminal > other terminals with
    /// matching CWD that are available.
    fn find_available_terminal_for_review(
        terminal_views: &[ViewHandle<TerminalView>],
        focused_terminal: Option<&ViewHandle<TerminalView>>,
        preferred_terminal_id: Option<EntityId>,
        repo_path: &LocalOrRemotePath,
        ai_enabled: bool,
        ctx: &AppContext,
    ) -> Option<ViewHandle<TerminalView>> {
        let is_available = |tv: &ViewHandle<TerminalView>| {
            Self::is_terminal_available_for_review(tv, repo_path, ai_enabled, ctx)
        };

        // Try the focused terminal first.
        if let Some(tv) = focused_terminal {
            if is_available(tv) {
                return Some(tv.clone());
            }
        }

        // Try the preferred (repo-mapped) terminal next.
        if let Some(preferred_id) = preferred_terminal_id {
            if let Some(tv) = terminal_views.iter().find(|tv| tv.id() == preferred_id) {
                if is_available(tv) {
                    return Some(tv.clone());
                }
            }
        }

        // Fallback: any terminal in the repo that is available.
        terminal_views.iter().find(|tv| is_available(tv)).cloned()
    }

    /// Finds the best available terminal for review in the given pane group,
    /// gathering the terminal list, focused terminal, and preferred terminal ID
    /// before delegating to `find_available_terminal_for_review`.
    fn find_review_terminal(
        &self,
        pane_group: &ViewHandle<PaneGroup>,
        repo_path: &LocalOrRemotePath,
        ai_enabled: bool,
        ctx: &AppContext,
    ) -> Option<ViewHandle<TerminalView>> {
        let terminal_views = pane_group.read(ctx, |pg, ctx| pg.visible_terminal_views(ctx));
        let focused_terminal = pane_group.read(ctx, |pg, ctx| pg.focused_session_view(ctx));
        let pane_group_id = pane_group.id();
        let preferred_terminal_id = self
            .working_directories_model
            .as_ref(ctx)
            .get_terminal_id_for_root_path(pane_group_id, repo_path);

        Self::find_available_terminal_for_review(
            &terminal_views,
            focused_terminal.as_ref(),
            preferred_terminal_id,
            repo_path,
            ai_enabled,
            ctx,
        )
    }

    /// Checks whether any terminal in the pane group is available for input in
    /// the correct working directory and pushes the result to the active
    /// CodeReviewView.
    pub fn recompute_terminal_availability(&self, ctx: &mut ViewContext<Self>) {
        let Some(code_review_view) = self.get_active_code_review_view(ctx) else {
            return;
        };

        let repo_path = code_review_view.read(ctx, |view, _| view.repo_path().cloned());
        let Some(repo_path) = repo_path else {
            code_review_view.update(ctx, |view, ctx| {
                view.set_review_destination(ReviewDestination::None, ctx);
            });
            return;
        };

        let Some(pane_group) = &self.active_pane_group else {
            code_review_view.update(ctx, |view, ctx| {
                view.set_review_destination(ReviewDestination::None, ctx);
            });
            return;
        };

        let ai_enabled = AISettings::as_ref(ctx).is_any_ai_enabled(ctx);
        let destination = self
            .find_review_terminal(pane_group, &repo_path, ai_enabled, ctx)
            .map(|tv| {
                tv.read(ctx, |t, ctx| {
                    t.active_cli_agent(ctx)
                        .map(ReviewDestination::Cli)
                        .unwrap_or(ReviewDestination::Warp)
                })
            })
            .unwrap_or(ReviewDestination::None);

        code_review_view.update(ctx, |view, ctx| {
            view.set_review_destination(destination, ctx);
        });
    }

    fn ensure_code_review_view_exists(
        &mut self,
        repo_path: &LocalOrRemotePath,
        ctx: &mut ViewContext<Self>,
    ) {
        if repo_path.is_remote() && !FeatureFlag::RemoteCodeReview.is_enabled() {
            return;
        }
        let Some(pane_group) = &self.active_pane_group else {
            return;
        };
        let pane_group_id = pane_group.id();
        // Only set up subscriptions and diff loading when the panel is visible.
        // When the panel opens later, open_code_review will call on_open.
        let is_panel_open = pane_group.as_ref(ctx).right_panel_open;

        let existing_view = self
            .working_directories_model
            .as_ref(ctx)
            .get_code_review_view(pane_group_id, repo_path);

        if let Some(view) = existing_view {
            if is_panel_open {
                // on_open is idempotent (guards on is_open), so this is safe for
                // already-open views and correctly re-opens cached-but-closed ones.
                view.update(ctx, |view, ctx| {
                    view.on_open(ctx);
                });
            }
        } else {
            // Prefer the pane group's active session so the diff request rides
            // the connection actually showing the review; the manager falls
            // back to any connected session for the host when unavailable.
            let preferred_session = pane_group
                .read(ctx, |pg, ctx| pg.active_session_view(ctx))
                .and_then(|tv| tv.as_ref(ctx).active_block_session_id());
            let diff_state_model = self.working_directories_model.update(ctx, |model, ctx| {
                model.get_or_create_diff_state_model(repo_path.clone(), preferred_session, ctx)
            });

            let Some(diff_state_model) = diff_state_model else {
                return;
            };
            let is_known_repo = self
                .working_directories_model
                .as_ref(ctx)
                .most_recent_repositories_for_pane_group(pane_group_id)
                .is_some_and(|mut repos| repos.any(|r| &r == repo_path));

            let terminal_view = if is_known_repo {
                let Some(terminal_view_id) = self
                    .working_directories_model
                    .as_ref(ctx)
                    .get_terminal_id_for_root_path(pane_group_id, repo_path)
                else {
                    return;
                };
                ctx.view_with_id::<TerminalView>(ctx.window_id(), terminal_view_id)
            } else {
                // For repos not yet tracked (e.g. remote repos from direct open),
                // fall back to the active session.
                pane_group.read(ctx, |pane_group, ctx| pane_group.active_session_view(ctx))
            };

            if let Some(terminal_view) = terminal_view {
                if let Some(view) = self.create_code_review_view(
                    repo_path,
                    diff_state_model,
                    pane_group_id,
                    terminal_view.downgrade(),
                    ctx,
                ) {
                    if is_panel_open {
                        view.update(ctx, |view, ctx| {
                            view.on_open(ctx);
                        });
                    }
                }
            }
        }
    }
}

impl Entity for RightPanelView {
    type Event = RightPanelEvent;
}

#[cfg(feature = "local_fs")]
impl TypedActionView for RightPanelView {
    type Action = RightPanelAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            RightPanelAction::ToggleFileSidebar => {
                if let Some(state) = &self.code_review_state {
                    if let Some(repo_path) = &state.selected_repo_path {
                        if let Some(pane_group) = &self.active_pane_group {
                            let pane_group_id = pane_group.id();
                            let working_directories_model = self.working_directories_model.clone();
                            if let Some(code_review_view) = working_directories_model
                                .as_ref(ctx)
                                .get_code_review_view(pane_group_id, repo_path)
                            {
                                code_review_view.update(ctx, |view, ctx| {
                                    view.handle_action(&CodeReviewAction::ToggleFileSidebar, ctx);
                                });
                            }
                        }
                    }
                }
            }
            RightPanelAction::SetMode(mode) => {
                self.set_active_mode(*mode, ctx);
            }
            RightPanelAction::SelectRepo {
                repo_path,
                from_dropdown,
            } => {
                // Only close the old view if we're actually switching to a different repo.
                let is_switching = self
                    .code_review_state
                    .as_ref()
                    .and_then(|s| s.selected_repo_path.as_ref())
                    .is_some_and(|old| old != repo_path);
                if is_switching {
                    self.close_active_code_review_view(ctx);
                }
                if let Some(state) = &mut self.code_review_state {
                    // Don't update dropdown when selection comes from dropdown itself
                    let should_update_dropdown = !from_dropdown;
                    state.set_selected_repo_internal(
                        repo_path.clone(),
                        should_update_dropdown,
                        ctx,
                    );
                    self.ensure_code_review_view_exists(repo_path, ctx);

                    // Persist the user's manual selection so it can be restored when
                    // they leave this pane group's session and come back. We only
                    // persist explicit `SelectRepo` actions (i.e. dropdown picks or
                    // contextual opens) so that the auto-selected default doesn't
                    // overwrite an earlier manual choice for a different pane group.
                    if let Some(pane_group) = &self.active_pane_group {
                        let pane_group_id = pane_group.id();
                        let repo_path = repo_path.clone();
                        self.working_directories_model.update(ctx, |model, _| {
                            model.set_selected_review_repo(pane_group_id, repo_path);
                        });
                    }

                    ctx.notify();
                }
            }
            RightPanelAction::ToggleMaximize => {
                ctx.emit(RightPanelEvent::ToggleMaximize);
                ctx.notify();
            }
            RightPanelAction::OpenBrowserCurrentUrl => {
                if let Some(url) = self.current_browser_url_from_editor(ctx) {
                    self.open_browser_url(url, ctx);
                }
            }
            RightPanelAction::OpenBrowserExternal => {
                self.open_browser_external(ctx);
            }
            RightPanelAction::CopyBrowserUrl => {
                if let Some(url) = self.current_browser_url_from_editor(ctx) {
                    ctx.clipboard().write(ClipboardContent::plain_text(url));
                }
            }
            RightPanelAction::RefreshBrowserUrl => {
                if let Some(url) = self.browser_current_url.clone() {
                    self.open_browser_url(url, ctx);
                } else if let Some(url) = self.current_browser_url_from_editor(ctx) {
                    self.open_browser_url(url, ctx);
                }
            }
            RightPanelAction::BrowserBack => {
                if self.can_browser_go_back() {
                    self.navigate_browser_history(-1, ctx);
                }
            }
            RightPanelAction::BrowserForward => {
                if self.can_browser_go_forward() {
                    self.navigate_browser_history(1, ctx);
                }
            }
            RightPanelAction::ToggleBrowserElementPicker => {
                self.toggle_browser_element_picker(ctx);
            }
            RightPanelAction::AttachBrowserElement { index } => {
                self.attach_browser_element_as_context(*index, ctx);
            }
            RightPanelAction::OpenRepository => {
                if let Some(active_pane_group) = &self.active_pane_group {
                    let terminal_view = active_pane_group.read(ctx, |pane_group, ctx| {
                        pane_group
                            .active_session_id(ctx)
                            .and_then(|id| pane_group.terminal_view_from_pane_id(id, ctx))
                    });

                    if let Some(terminal_view) = terminal_view {
                        terminal_view.update(ctx, |terminal, ctx| {
                            terminal.handle_action(
                                &crate::terminal::view::TerminalAction::PickRepoToOpen,
                                ctx,
                            );
                        });
                    }
                }
            }
        }
    }
}

#[cfg(not(feature = "local_fs"))]
impl TypedActionView for RightPanelView {
    type Action = RightPanelAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            RightPanelAction::SetMode(mode) => self.set_active_mode(*mode, ctx),
            RightPanelAction::ToggleMaximize => {
                ctx.emit(RightPanelEvent::ToggleMaximize);
                ctx.notify();
            }
            RightPanelAction::OpenBrowserCurrentUrl => {
                if let Some(url) = self.current_browser_url_from_editor(ctx) {
                    self.open_browser_url(url, ctx);
                }
            }
            RightPanelAction::OpenBrowserExternal => {
                self.open_browser_external(ctx);
            }
            RightPanelAction::CopyBrowserUrl => {
                if let Some(url) = self.current_browser_url_from_editor(ctx) {
                    ctx.clipboard().write(ClipboardContent::plain_text(url));
                }
            }
            RightPanelAction::RefreshBrowserUrl => {
                if let Some(url) = self.browser_current_url.clone() {
                    self.open_browser_url(url, ctx);
                } else if let Some(url) = self.current_browser_url_from_editor(ctx) {
                    self.open_browser_url(url, ctx);
                }
            }
            RightPanelAction::BrowserBack => {
                if self.can_browser_go_back() {
                    self.navigate_browser_history(-1, ctx);
                }
            }
            RightPanelAction::BrowserForward => {
                if self.can_browser_go_forward() {
                    self.navigate_browser_history(1, ctx);
                }
            }
            RightPanelAction::ToggleBrowserElementPicker => {
                self.toggle_browser_element_picker(ctx);
            }
            RightPanelAction::AttachBrowserElement { index } => {
                self.attach_browser_element_as_context(*index, ctx);
            }
            RightPanelAction::ToggleFileSidebar
            | RightPanelAction::SelectRepo { .. }
            | RightPanelAction::OpenRepository => {}
        }
    }
}

impl View for RightPanelView {
    fn ui_name() -> &'static str {
        "RightPanelView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let panel_content = self.render_panel_content(app);

        if self.is_maximized(app) {
            return Shrinkable::new(1.0, panel_content).finish();
        }

        let drag_side = match self.panel_position {
            super::PanelPosition::Left => DragBarSide::Right,
            super::PanelPosition::Right => DragBarSide::Left,
        };
        Resizable::new(self.resizable_state_handle.clone(), panel_content)
            .with_dragbar_side(drag_side)
            .on_resize(move |ctx, _| {
                ctx.notify();
            })
            .with_bounds_callback(Box::new(|window_size| {
                let min_width = MIN_SIDEBAR_WIDTH;
                let max_width = window_size.x() * MAX_SIDEBAR_WIDTH_RATIO;
                (min_width, max_width.max(min_width))
            }))
            .finish()
    }
}
