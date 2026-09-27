//! Client-side projections.
//!
//! The daemon owns the truth; these stores only merge what arrived over SSE,
//! de-duplicating by sequence so a reconnect, a history fetch and a live delta
//! can all land in the same list (see docs/architecture.md). Both surfaces use
//! their own store: the inspector observes, the panel configures.

use std::collections::BTreeMap;
use std::future::Future;

use leptos::prelude::*;
use leptos::task::spawn_local;
use relay_api::{
    CodexStatus, Health, InspectorSnapshot, InstallResult, RelayConfigView, RuntimeOptionsView, RuntimeProbe,
    StreamMessage,
};
use relay_core::{AgentProfile, RelayEvent, RelayPolicy, RelayPolicyOverride, RunStatus};

use crate::api::{self, Client, Route};
use crate::dom;
use crate::format;
use crate::i18n::{Locale, Translator};

/// How long a notice pill stays on screen before it dismisses itself.
pub const NOTICE_MS: u32 = 3_500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    Live,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectorTab {
    Console,
    Changes,
    Raw,
}

impl InspectorTab {
    /// The console's three views, in the order the tabs are shown.
    pub const ALL: [InspectorTab; 3] = [InspectorTab::Console, InspectorTab::Changes, InspectorTab::Raw];

}

/// The run a session should open on: whatever is still moving, else the newest.
pub fn preferred_run(session: &relay_api::SessionView) -> Option<&relay_api::RunView> {
    let find = |status: RunStatus| session.runs.iter().find(|view| view.run.status == status);
    find(RunStatus::Running)
        .or_else(|| find(RunStatus::Starting))
        .or_else(|| find(RunStatus::AwaitingHost))
        .or_else(|| session.runs.first())
}

/// The inspector's page-wide state.
#[derive(Clone, Copy)]
pub struct Store {
    client: RwSignal<Option<Client>>,
    pub snapshot: RwSignal<Option<InspectorSnapshot>>,
    /// One `seq -> event` map per run: a re-sent sequence replaces the old one,
    /// and iterating the map is already sorted by sequence.
    pub events: RwSignal<BTreeMap<String, BTreeMap<u64, RelayEvent>>>,
    pub connection: RwSignal<Connection>,
    pub error: RwSignal<Option<String>>,
    pub session: RwSignal<Option<String>>,
    pub run: RwSignal<Option<String>>,
    pub step: RwSignal<Option<String>>,
    pub tab: RwSignal<InspectorTab>,
    pub locale: RwSignal<Locale>,
    pub notice: RwSignal<Option<String>>,
    notice_seq: RwSignal<u64>,
}

impl Store {
    pub fn new(client: Option<Client>, locale: Locale) -> Self {
        Self {
            client: RwSignal::new(client),
            snapshot: RwSignal::new(None),
            events: RwSignal::new(BTreeMap::new()),
            connection: RwSignal::new(Connection::Connecting),
            error: RwSignal::new(None),
            session: RwSignal::new(None),
            run: RwSignal::new(None),
            step: RwSignal::new(None),
            tab: RwSignal::new(InspectorTab::Console),
            locale: RwSignal::new(locale),
            notice: RwSignal::new(None),
            notice_seq: RwSignal::new(0),
        }
    }

    pub fn client(&self) -> Option<Client> {
        self.client.get()
    }

    pub fn translator(&self) -> Translator {
        Translator::new(self.locale)
    }

    /// The inspector remembers the language; the panel's comes from `?lang`.
    pub fn set_locale(&self, locale: Locale) {
        dom::set_stored(dom::LOCALE_KEY, locale.code());
        self.locale.set(locale);
    }

    /// A bottom-centre notice pill that clears itself after 3.5s. The sequence
    /// keeps a later notice from being dismissed by an earlier timer.
    pub fn notify(&self, message: impl Into<String>) {
        let next = self.notice_seq.get_untracked() + 1;
        self.notice_seq.set(next);
        self.notice.set(Some(message.into()));
        let store = *self;
        gloo_timers::callback::Timeout::new(NOTICE_MS, move || {
            if store.notice_seq.get_untracked() == next {
                store.notice.set(None);
            }
        })
        .forget();
    }

    /// Merge rule from the React store: keyed by sequence, then sorted.
    pub fn merge_run_events(&self, run_id: &str, incoming: Vec<RelayEvent>) {
        if incoming.is_empty() {
            return;
        }
        self.events.update(|all| {
            let bucket = all.entry(run_id.to_string()).or_default();
            for event in incoming {
                bucket.insert(event.seq, event);
            }
        });
    }

    /// Everything cached for one run, oldest first.
    pub fn run_events(&self, run_id: &str) -> Vec<RelayEvent> {
        self.events
            .with(|all| all.get(run_id).map(|bucket| bucket.values().cloned().collect()).unwrap_or_default())
    }

    pub fn codex_configured(&self) -> bool {
        self.snapshot.with(|snapshot| snapshot.as_ref().map(|value| value.codex.configured).unwrap_or(false))
    }

    pub fn session_view(&self, session_id: &str) -> Option<relay_api::SessionView> {
        self.snapshot.with(|snapshot| {
            snapshot.as_ref().and_then(|value| {
                value.sessions.iter().find(|view| view.session.id == session_id).cloned()
            })
        })
    }

    pub fn run_view(&self, session_id: &str, run_id: &str) -> Option<relay_api::RunView> {
        self.snapshot.with(|snapshot| {
            snapshot.as_ref().and_then(|value| {
                value
                    .sessions
                    .iter()
                    .find(|view| view.session.id == session_id)
                    .and_then(|session| session.runs.iter().find(|view| view.run.id == run_id))
                    .cloned()
            })
        })
    }
}

/// One stream per page: the store owns the merge, this owns the socket.
pub fn start_stream(store: Store, client: Client) {
    // The stream also sends a snapshot; this fetch covers a slow or refused open.
    let initial_client = client.clone();
    spawn_local(async move {
        match initial_client.snapshot().await {
            Ok(snapshot) => store.snapshot.set(Some(snapshot)),
            Err(error) => {
                store.connection.set(Connection::Error);
                store.error.set(Some(error.to_string()));
            }
        }
    });

    let connected = api::connect_stream(
        &client.stream_url(),
        move || store.connection.set(Connection::Live),
        move |message| {
            store.connection.set(Connection::Error);
            store.error.set(Some(message));
        },
        move |message| match message {
            // `hello` only carries the port and start time: nothing to render.
            StreamMessage::Hello { .. } => {}
            StreamMessage::Snapshot { snapshot } => {
                store.snapshot.set(Some(*snapshot));
                store.connection.set(Connection::Live);
            }
            StreamMessage::Events { batch } => store.merge_run_events(&batch.run_id, batch.events),
        },
    );

    match connected {
        // The stream lives exactly as long as the page, so its handle is leaked
        // on purpose instead of being dropped with the bootstrap function.
        Ok(stream) => {
            Box::leak(Box::new(stream));
        }
        Err(error) => {
            store.connection.set(Connection::Error);
            store.error.set(Some(error.to_string()));
        }
    }
}

/// Pulls history for a run that has nothing cached yet.
pub fn fetch_run_history(store: Store, client: Client, run_id: String) {
    spawn_local(async move {
        match client.events(&run_id, 0).await {
            Ok(batch) => store.merge_run_events(&batch.run_id, batch.events),
            Err(error) => store.notify(error.to_string()),
        }
    });
}

/// Copies the daemon's diagnostics report and reports the outcome in the pill.
pub fn copy_diagnostics(store: Store) {
    let Some(client) = store.client() else {
        return;
    };
    spawn_local(async move {
        match client.diagnostics().await {
            Ok(report) => match dom::write_clipboard(&report).await {
                Ok(()) => store.notify(store.translator().t("inspector.diagnosticsCopied")),
                Err(message) => store.notify(message),
            },
            Err(error) => store.notify(error.to_string()),
        }
    });
}

/// `POST /api/workers/{id}/cancel` for the console's Stop button.
pub fn cancel_worker(store: Store, worker_session_id: String) {
    let Some(client) = store.client() else {
        return;
    };
    spawn_local(async move {
        match client.cancel_worker(&worker_session_id).await {
            Ok(_) => store.notify(store.translator().t("inspector.stopped")),
            Err(error) => store.notify(error.to_string()),
        }
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelTab {
    Agents,
    Runtimes,
    Policy,
    Codex,
    Status,
}

impl PanelTab {
    /// Tabs in the order the panel shows them. `runtime` is the id the old panel
    /// used in `?tab=`, so existing deep links keep working.
    pub const ORDER: [PanelTab; 5] =
        [PanelTab::Agents, PanelTab::Runtimes, PanelTab::Policy, PanelTab::Codex, PanelTab::Status];

    pub fn key(self) -> &'static str {
        match self {
            PanelTab::Agents => "nav.agents",
            PanelTab::Runtimes => "panel.runtime",
            PanelTab::Policy => "panel.policy",
            PanelTab::Codex => "menu.codex",
            PanelTab::Status => "panel.status",
        }
    }

    pub fn from_id(value: &str) -> Self {
        match value {
            "policy" => PanelTab::Policy,
            "codex" => PanelTab::Codex,
            "runtime" | "runtimes" => PanelTab::Runtimes,
            "status" => PanelTab::Status,
            _ => PanelTab::Agents,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelIntent {
    NewAgent,
    EditAgent,
    AddRuntime,
    CodexActions,
}

impl PanelIntent {
    pub fn from_id(value: &str) -> Option<Self> {
        match value {
            "new-agent" => Some(PanelIntent::NewAgent),
            "edit-agent" => Some(PanelIntent::EditAgent),
            "add-runtime" => Some(PanelIntent::AddRuntime),
            "codex-actions" => Some(PanelIntent::CodexActions),
            _ => None,
        }
    }
}

/// A value the panel loads over HTTP and then keeps for the page.
///
/// `Loading` doubles as the in-flight marker: while it is set another request
/// for the same key is a no-op. That is what stops a failing request from
/// re-triggering itself, because a load is asked for by a click or by an editor
/// opening — never by a reactive effect that watches the state it writes.
#[derive(Debug, Clone, PartialEq)]
pub enum Remote<T> {
    /// Never asked for.
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> Remote<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Remote::Ready(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Remote::Loading)
    }

    /// In flight or finished: `ensure_*` treats this as "do not ask again".
    pub fn is_settled(&self) -> bool {
        matches!(self, Remote::Loading | Remote::Ready(_))
    }
}

/// One runtime's model/reasoning lists plus the request that owns them: a
/// response whose `request` id no longer matches is stale and is dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeOptionsEntry {
    pub request: u64,
    pub state: Remote<RuntimeOptionsView>,
}

/// The control panel's state: configuration, the environment snapshot and the
/// Codex integration status.
#[derive(Clone, Copy)]
pub struct PanelStore {
    client: RwSignal<Option<Client>>,
    base: RwSignal<String>,
    pub config: RwSignal<Option<RelayConfigView>>,
    pub snapshot: RwSignal<Option<InspectorSnapshot>>,
    pub codex: RwSignal<Option<CodexStatus>>,
    pub notice: RwSignal<Option<String>>,
    pub error: RwSignal<Option<String>>,
    pub busy: RwSignal<bool>,
    /// The panel is served by the daemon: without the token nothing can load.
    pub missing_token: RwSignal<bool>,
    pub tab: RwSignal<PanelTab>,
    pub locale: RwSignal<Locale>,
    /// Tray navigation: an intent opens an editor instead of the list.
    pub intent: RwSignal<Option<PanelIntent>>,
    pub intent_profile: RwSignal<Option<String>>,
    /// Model and reasoning values per runtime id, fetched when the agent editor
    /// opens one. Owned here so a tab switch while the request is in flight
    /// cannot write into a branch that has been disposed.
    pub runtime_options: RwSignal<BTreeMap<String, RuntimeOptionsEntry>>,
    /// The adapter catalogue the runtime editor offers.
    pub adapter_catalog: RwSignal<Remote<Vec<String>>>,
    /// The last executable probe, and whether one is in flight right now.
    pub probe: RwSignal<Option<RuntimeProbe>>,
    pub probing: RwSignal<bool>,
    /// Daemon health for the Status tab, and the error that replaced it.
    pub health: RwSignal<Option<Health>>,
    pub health_error: RwSignal<Option<String>>,
    /// Request ids: a late response only writes while it still owns its request.
    options_seq: RwSignal<u64>,
    adapters_seq: RwSignal<u64>,
    probe_seq: RwSignal<u64>,
    health_seq: RwSignal<u64>,
}

impl PanelStore {
    pub fn new(
        client: Option<Client>,
        base: String,
        locale: Locale,
        tab: PanelTab,
        intent: Option<PanelIntent>,
        profile_id: Option<String>,
    ) -> Self {
        let missing_token = client.is_none();
        Self {
            client: RwSignal::new(client),
            base: RwSignal::new(base),
            config: RwSignal::new(None),
            snapshot: RwSignal::new(None),
            codex: RwSignal::new(None),
            notice: RwSignal::new(None),
            error: RwSignal::new(None),
            busy: RwSignal::new(false),
            missing_token: RwSignal::new(missing_token),
            tab: RwSignal::new(tab),
            locale: RwSignal::new(locale),
            intent: RwSignal::new(intent),
            intent_profile: RwSignal::new(profile_id),
            runtime_options: RwSignal::new(BTreeMap::new()),
            adapter_catalog: RwSignal::new(Remote::Idle),
            probe: RwSignal::new(None),
            probing: RwSignal::new(false),
            health: RwSignal::new(None),
            health_error: RwSignal::new(None),
            options_seq: RwSignal::new(0),
            adapters_seq: RwSignal::new(0),
            probe_seq: RwSignal::new(0),
            health_seq: RwSignal::new(0),
        }
    }

    pub fn client(&self) -> Option<Client> {
        self.client.get()
    }

    pub fn base(&self) -> String {
        self.base.get()
    }

    pub fn translator(&self) -> Translator {
        Translator::new(self.locale)
    }

    /// True when the panel has no daemon to talk to at all: the token was missing
    /// from the URL, which is the one thing the panel cannot recover from.
    pub fn daemon_down(&self) -> bool {
        self.missing_token.get()
    }

    pub fn net_codex_configured(&self) -> bool {
        self.snapshot.with(|snapshot| snapshot.as_ref().map(|value| value.codex.configured).unwrap_or(false))
    }

    /// The panel's footer notice stays until the next action replaces it, which
    /// is what the old panel did: an install/repair report is worth reading twice.
    /// (The inspector's pill does auto-dismiss; see `Store::notify`.)
    pub fn notify(&self, message: impl Into<String>) {
        self.notice.set(Some(message.into()));
    }

    pub fn consume_intent(&self) {
        self.intent.set(None);
        self.intent_profile.set(None);
    }

    /// Configuration and snapshot together: the panel never shows one without the
    /// other, so a half-loaded view cannot look like a broken daemon.
    pub fn reload(&self) {
        let Some(client) = self.client() else {
            return;
        };
        let store = *self;
        spawn_local(async move {
            match client.config().await {
                Ok(config) => store.config.set(Some(config)),
                Err(error) => {
                    store.error.set(Some(error.to_string()));
                    return;
                }
            }
            match client.snapshot().await {
                Ok(snapshot) => {
                    store.codex.set(Some(snapshot.codex.clone()));
                    store.snapshot.set(Some(snapshot));
                    store.error.set(None);
                }
                Err(error) => store.error.set(Some(error.to_string())),
            }
        });
    }

    /// Runs a mutation with the panel's busy flag and failure notice.
    pub fn guard<F>(&self, task: F)
    where
        F: Future<Output = Result<(), String>> + 'static,
    {
        self.busy.set(true);
        let store = *self;
        spawn_local(async move {
            if let Err(message) = task.await {
                store.notify(format!("{}: {message}", store.translator().t("panel.saveFailed")));
            }
            store.busy.set(false);
        });
    }

    pub fn save_profile(&self, profile: AgentProfile) {
        let store = *self;
        self.guard(async move {
            let client = store.client().ok_or_else(|| "missing token".to_string())?;
            let config = client.save_profile(&profile).await.map_err(|error| error.to_string())?;
            store.config.set(Some(config));
            store.consume_intent();
            store.reload();
            store.notify(store.translator().t("panel.saved"));
            Ok(())
        });
    }

    pub fn delete_profile(&self, profile_id: String) {
        let store = *self;
        self.guard(async move {
            let client = store.client().ok_or_else(|| "missing token".to_string())?;
            let config = client.delete_profile(&profile_id).await.map_err(|error| error.to_string())?;
            store.config.set(Some(config));
            store.consume_intent();
            store.reload();
            store.notify(store.translator().t("panel.saved"));
            Ok(())
        });
    }

    pub fn save_policy(&self, policy: RelayPolicy, workspace_overrides: BTreeMap<String, RelayPolicyOverride>) {
        let store = *self;
        self.guard(async move {
            let client = store.client().ok_or_else(|| "missing token".to_string())?;
            let config = client
                .save_policy(&policy, &workspace_overrides)
                .await
                .map_err(|error| error.to_string())?;
            store.config.set(Some(config));
            store.reload();
            store.notify(store.translator().t("panel.saved"));
            Ok(())
        });
    }

    pub fn save_runtime(&self, id: String, adapter_id: String, executable_path: String, label: Option<String>) {
        let store = *self;
        self.guard(async move {
            let client = store.client().ok_or_else(|| "missing token".to_string())?;
            let mutation = client
                .save_runtime(&id, &adapter_id, &executable_path, label)
                .await
                .map_err(|error| error.to_string())?;
            store.config.set(Some(mutation.config));
            store.reload();
            let translator = store.translator();
            let message = if mutation.probe.ok {
                format!("{} · {}", translator.t("panel.saved"), mutation.probe.version.unwrap_or_default())
            } else {
                mutation.probe.error.unwrap_or_else(|| translator.t("panel.saveFailed"))
            };
            store.notify(message);
            Ok(())
        });
    }

    pub fn delete_runtime(&self, runtime_id: String) {
        let store = *self;
        self.guard(async move {
            let client = store.client().ok_or_else(|| "missing token".to_string())?;
            let config = client.delete_runtime(&runtime_id).await.map_err(|error| error.to_string())?;
            store.config.set(Some(config));
            store.reload();
            store.notify(store.translator().t("panel.saved"));
            Ok(())
        });
    }

    /// Codex lifecycle: the returned status replaces the cached one and every
    /// message the daemon produced goes to the footer notice.
    pub fn run_codex(&self, action: relay_api::CodexAction) {
        let store = *self;
        self.guard(async move {
            let client = store.client().ok_or_else(|| "missing token".to_string())?;
            let result: InstallResult = client.codex(action).await.map_err(|error| error.to_string())?;
            store.codex.set(Some(result.status));
            store.notify(result.messages.join(" · "));
            store.reload();
            Ok(())
        });
    }

    pub fn rescan(&self) {
        let store = *self;
        self.guard(async move {
            let client = store.client().ok_or_else(|| "missing token".to_string())?;
            let result = client.refresh().await.map_err(|error| error.to_string())?;
            store.reload();
            store.notify(format!("{} runtimes · {} profiles", result.runtimes, result.profiles));
            Ok(())
        });
    }

    /// The model/reasoning values for one runtime, once the daemon answered.
    ///
    /// Entries are keyed by runtime id, so a response that arrives after the
    /// editor moved on can never be shown under another runtime.
    pub fn runtime_options_for(&self, runtime_id: &str) -> Option<RuntimeOptionsView> {
        if runtime_id.is_empty() {
            return None;
        }
        self.runtime_options.with(|all| all.get(runtime_id).and_then(|entry| entry.state.ready().cloned()))
    }

    /// True while that runtime's model-options request is in flight.
    pub fn runtime_options_loading(&self, runtime_id: &str) -> bool {
        !runtime_id.is_empty()
            && self
                .runtime_options
                .with(|all| all.get(runtime_id).map(|entry| entry.state.is_loading()).unwrap_or(false))
    }

    /// Asks for one runtime's model/reasoning values, at most one request at a
    /// time: a Loading or Ready entry short-circuits, and a failed one is
    /// retried only by the next explicit open of the editor.
    ///
    /// The request outlives the editor that asked for it: every write below
    /// targets a PanelStore signal, never a view's.
    pub fn ensure_runtime_options(&self, runtime_id: String) {
        if runtime_id.is_empty() {
            return;
        }
        let settled = self
            .runtime_options
            .with_untracked(|all| all.get(&runtime_id).map(|entry| entry.state.is_settled()).unwrap_or(false));
        if settled {
            return;
        }
        let request = self.options_seq.get_untracked() + 1;
        self.options_seq.set(request);
        let store = *self;
        let id = runtime_id.clone();
        store.runtime_options.update(|all| {
            all.insert(id, RuntimeOptionsEntry { request, state: Remote::Loading });
        });
        spawn_local(async move {
            let client = store.client.get_untracked();
            let result = match client {
                Some(client) => client.runtime_options(&runtime_id).await.map_err(|error| error.to_string()),
                None => Err("missing token".to_string()),
            };
            store.finish_runtime_options(runtime_id, request, result);
        });
    }

    /// The stale-response guard for ensure_runtime_options: only the request
    /// that still owns the runtime's entry may write to it.
    fn finish_runtime_options(&self, runtime_id: String, request: u64, result: Result<RuntimeOptionsView, String>) {
        let owned = self
            .runtime_options
            .with_untracked(|all| all.get(&runtime_id).map(|entry| entry.request == request).unwrap_or(false));
        if !owned {
            return;
        }
        let state = match result {
            Ok(options) => Remote::Ready(options),
            Err(message) => Remote::Failed(message),
        };
        self.runtime_options.update(|all| {
            all.insert(runtime_id, RuntimeOptionsEntry { request, state });
        });
    }

    /// The adapter catalogue as a view closure sees it: reading through here
    /// subscribes the view, so it re-renders when the list arrives.
    pub fn adapters(&self) -> Option<Vec<String>> {
        self.adapter_catalog.with(|state| state.ready().cloned())
    }

    /// The same value for event handlers, which must not subscribe.
    pub fn adapters_cached(&self) -> Option<Vec<String>> {
        self.adapter_catalog.with_untracked(|state| state.ready().cloned())
    }

    /// Fetches the adapter catalogue unless a request is in flight or a list is
    /// already cached. A failure is retried only when the editor is opened
    /// again: nothing reactive watches this state, so a daemon that is down
    /// cannot turn a failure into a request loop.
    pub fn ensure_adapters(&self) {
        if self.adapter_catalog.with_untracked(|state| state.is_settled()) {
            return;
        }
        let request = self.adapters_seq.get_untracked() + 1;
        self.adapters_seq.set(request);
        let store = *self;
        store.adapter_catalog.set(Remote::Loading);
        spawn_local(async move {
            let client = store.client.get_untracked();
            let result = match client {
                Some(client) => client.adapters().await.map(|catalog| catalog.adapters).map_err(|error| error.to_string()),
                None => Err("missing token".to_string()),
            };
            store.finish_adapters(request, result);
        });
    }

    /// Stale-response guard for ensure_adapters: only the newest catalogue
    /// request may write, so a slow failure cannot replace a newer list.
    fn finish_adapters(&self, request: u64, result: Result<Vec<String>, String>) {
        if self.adapters_seq.get_untracked() != request {
            return;
        }
        match result {
            Ok(list) => self.adapter_catalog.set(Remote::Ready(list)),
            Err(message) => {
                // The panel already has an error line; a catalogue the user
                // asked for and did not get belongs there.
                self.error.set(Some(message.clone()));
                self.adapter_catalog.set(Remote::Failed(message));
            }
        }
    }

    /// Probes an executable for the runtime editor. The result and the in-flight
    /// flag live here, so leaving the Runtimes tab mid-probe cannot write into
    /// the editor that asked.
    pub fn probe_runtime(&self, adapter_id: String, executable_path: String) {
        let request = self.probe_seq.get_untracked() + 1;
        self.probe_seq.set(request);
        self.probing.set(true);
        let store = *self;
        spawn_local(async move {
            let client = store.client.get_untracked();
            let result = match client {
                Some(client) => {
                    client.probe_runtime(&adapter_id, &executable_path).await.map_err(|error| error.to_string())
                }
                None => Err("missing token".to_string()),
            };
            // A probe the user has already superseded (path edited, adapter
            // changed, editor reopened) must not overwrite the newer answer.
            if store.probe_seq.get_untracked() != request {
                return;
            }
            let probe = match result {
                Ok(probe) => probe,
                Err(message) => RuntimeProbe { ok: false, version: None, error: Some(message) },
            };
            store.probe.set(Some(probe));
            store.probing.set(false);
        });
    }

    /// Drops a probe result as soon as the adapter or the path changes: it
    /// described a different executable. Bumping the request id also orphans a
    /// probe that is still in flight.
    pub fn clear_probe(&self) {
        self.probe_seq.set(self.probe_seq.get_untracked() + 1);
        self.probe.set(None);
        self.probing.set(false);
    }

    /// Daemon health for the Status tab. Cached on the store so a request that
    /// is still in flight when the user leaves the tab has somewhere to land,
    /// and request-id-guarded so an older answer cannot replace a newer one.
    pub fn load_health(&self) {
        let request = self.health_seq.get_untracked() + 1;
        self.health_seq.set(request);
        let store = *self;
        spawn_local(async move {
            let client = store.client.get_untracked();
            let result = match client {
                Some(client) => client.health().await.map_err(|error| error.to_string()),
                None => Err("missing token".to_string()),
            };
            if store.health_seq.get_untracked() != request {
                return;
            }
            match result {
                Ok(health) => {
                    store.health.set(Some(health));
                    store.health_error.set(None);
                }
                Err(message) => store.health_error.set(Some(message)),
            }
        });
    }

    pub fn copy_diagnostics(&self) {
        let Some(client) = self.client() else {
            return;
        };
        let store = *self;
        spawn_local(async move {
            match client.diagnostics().await {
                Ok(report) => match dom::write_clipboard(&report).await {
                    Ok(()) => store.notify(store.translator().t("inspector.diagnosticsCopied")),
                    Err(message) => store.notify(message),
                },
                Err(error) => store.notify(error.to_string()),
            }
        });
    }

    pub fn open_inspector(&self) {
        dom::open_new_tab("/");
    }
}

/// URL is the source of truth for what is selected, so the tray can deep-link
/// into a session or a run and the browser back button keeps working.
pub fn go(route: RwSignal<Route>, next: Route) {
    let path = dom::route_path(&next);
    if dom::pathname() != path {
        dom::push_url(&path);
    }
    route.set(next);
}

/// Human-readable diff stat used by both the row and the changes list.
pub fn diff_label(additions: Option<f64>, deletions: Option<f64>) -> String {
    let mut label = String::new();
    if let Some(value) = additions.filter(|value| *value != 0.0) {
        label.push_str(&format!(" +{}", format::number(value)));
    }
    if let Some(value) = deletions.filter(|value| *value != 0.0) {
        label.push_str(&format!(" -{}", format::number(value)));
    }
    label
}
