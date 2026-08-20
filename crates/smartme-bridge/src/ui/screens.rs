//! The configuration and confirmation screens (Story 6.2, FR46, FR25).
//!
//! # One validation, and it is not here
//!
//! Nothing in this module decides whether a setting is acceptable. It collects
//! what the browser posted into the same [`RawConfig`] the file produces, hands
//! it to the same [`config::validate`], and renders whatever faults come back.
//!
//! That restraint is the entire reason Story 5.1 exists — *"two consumers
//! assembling the same struct by hand is how their rules drift apart"*. A form
//! that pre-checked a bound would feel more responsive and would eventually
//! accept a value the bridge refuses to boot on, which is the failure this
//! project is built to avoid. **If you are about to add an `if` that compares a
//! posted value against a limit, it belongs in `app::config` instead.**
//!
//! # There is no credential field
//!
//! Not empty, not masked — absent ([ADR 0023]). The credential lives in the
//! environment and never descends to disk, so the strongest form of *never
//! rendered* turned out to be *never present*.
//!
//! [ADR 0023]: ../../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md

use std::sync::Arc;

use axum::Form;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};

use smart_me_client::{Credentials, DeviceListing, SmartMeClient, SmartMeError};

use crate::app::config::{self, ConfigErrors, Fault, RawConfig, RawMeter, Source};
use crate::app::reconfigure::Cost;
use crate::app::store::{self, Credential, StoredConfig, StoredMeter};
use crate::ui::UiState;

/// Escape text for an HTML body or a double-quoted attribute.
///
/// Hand-written rather than pulled in: five replacements, no dependency, and the
/// set is closed. **It is not optional.** Every value on these screens was typed
/// by whoever can reach the UI, and rendering a meter named `"><script>` back
/// into the form it came from would turn the configuration screen into the
/// delivery mechanism for anything a drive-by request could save (see the
/// same-origin guard in `ui::origin`).
pub(super) fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// The posted form, as pairs.
///
/// **Pairs rather than a struct**, because the meter rows repeat and a checkbox
/// that is not ticked posts nothing at all — so a positional decoding would
/// silently shift every field after the first disabled meter onto the wrong
/// meter. Each row is therefore indexed by the form (`meter.0.serial`), and the
/// index is what binds a value to its row rather than its position in the body.
type Fields = Vec<(String, String)>;

/// First value for a key, trimmed, `None` when absent or empty.
///
/// Empty and absent are deliberately the same thing here: an operator who clears
/// a text box means *unset*, and `RawConfig` already spells unset as `None`.
fn field<'a>(fields: &'a Fields, key: &str) -> Option<&'a str> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.trim())
        .filter(|v| !v.is_empty())
}

/// Collect the indexed meter rows, in index order, **dropping the ones nobody
/// filled in**.
///
/// # An empty row is a row the operator did not enter
///
/// The form always renders one blank "Add a meter" row so that adding a meter
/// needs no JavaScript. A browser submits empty text inputs, so that row arrives
/// as three present keys with empty values — and until 2026-08-05 it became a
/// `RawMeter` of three `None`s, which `validate` correctly refused with three
/// *"is missing or empty"* faults about a meter nobody had typed.
///
/// **The configuration screen could therefore be saved exactly once.** Every
/// later edit — changing the publish period, correcting the broker — was refused
/// on the operator's first press of Save, and it compounded: the refusal
/// re-rendered the empty row as a real one plus a fresh blank, so the next
/// attempt produced six faults, then nine. The story exists to remove the text
/// editor from the loop and it put it back on the second visit.
///
/// **A row with nothing in it and its box unticked is dropped**, and the two
/// halves of that rule are both deliberate. Dropping on emptiness alone would
/// silently discard a row somebody ticked and then forgot to fill, turning a
/// fault they could act on into a value that vanished. Clearing a row's three
/// fields is therefore also how a meter is *removed*, which is the behaviour the
/// blank row already implied.
fn meters(fields: &Fields) -> Vec<RawMeter> {
    let mut indices: Vec<usize> = fields
        .iter()
        .filter_map(|(k, _)| k.strip_prefix("meter.")?.split('.').next()?.parse().ok())
        .collect();
    indices.sort_unstable();
    indices.dedup();

    indices
        .into_iter()
        .map(|i| RawMeter {
            meter_id: field(fields, &format!("meter.{i}.meter_id")).map(str::to_string),
            device_id: field(fields, &format!("meter.{i}.device_id")).map(str::to_string),
            serial: field(fields, &format!("meter.{i}.serial")).map(str::to_string),
            // An unticked checkbox posts NOTHING. Absent therefore means
            // disabled, and that is the safe direction: a meter is published
            // because somebody ticked it, never because a value went missing.
            enabled: Some(field(fields, &format!("meter.{i}.enabled")).is_some()),
            // Same rule, same reason: an unticked box posts nothing, and silence
            // is not a claim that this meter matters (FR35, [ADR 0039]).
            priority: Some(field(fields, &format!("meter.{i}.priority")).is_some()),
        })
        .filter(|m| {
            m.meter_id.is_some()
                || m.device_id.is_some()
                || m.serial.is_some()
                || m.enabled == Some(true)
        })
        .collect()
}

/// Assemble what the browser posted into the shape the file produces.
///
/// The credential is joined from the environment exactly as `main.rs` does — and
/// **never from the form**, which has no field for it.
fn posted(fields: &Fields, state_dir: &std::path::Path) -> RawConfig {
    RawConfig {
        api_base: field(fields, "api_base").map(str::to_string),
        client_id: std::env::var("SMARTME_CLIENT_ID").ok(),
        client_secret: std::env::var("SMARTME_CLIENT_SECRET").ok(),
        group_id: field(fields, "group_id").map(str::to_string),
        node_id: field(fields, "node_id").map(str::to_string),
        broker_host: field(fields, "broker_host").map(str::to_string),
        broker_port: field(fields, "broker_port").map(str::to_string),
        state_dir: Some(state_dir.display().to_string()),
        publish_period_secs: field(fields, "publish_period_secs").map(str::to_string),
        log_dir: field(fields, "log_dir").map(str::to_string),
        // Carried through as typed. Parsing here with `.ok()` is what made a
        // mistyped port vanish without a word.
        log_keep: field(fields, "log_keep").map(str::to_string),
        ui_port: field(fields, "ui_port").map(str::to_string),
        meters: meters(fields),
    }
}

/// What the operator typed, in the stored shape, **for redisplay only**.
///
/// # This value must never be written to disk
///
/// It exists so that a refused submission comes back with the boxes still full,
/// rather than making the operator retype what they entered. It is therefore a
/// record of *strings*, including ones `validate` rejected — and re-deriving
/// numbers from them is how the browser came to write `publish_period_secs = 0`
/// over a submission `validate` had accepted as the 30 s default, leaving a
/// container that refused to start and served no UI to repair it.
///
/// What gets written is [`StoredConfig::from(&BridgeConfig)`](store), built from
/// the struct `validate` returned. If you find yourself passing this function's
/// result to [`store::save`], that is the defect coming back.
fn as_typed(fields: &Fields, raw: &RawConfig) -> StoredConfig {
    StoredConfig {
        schema_version: store::SCHEMA_VERSION,
        group_id: raw.group_id.clone().unwrap_or_default(),
        node_id: raw.node_id.clone().unwrap_or_default(),
        broker_host: raw.broker_host.clone().unwrap_or_default(),
        broker_port: raw
            .broker_port
            .as_deref()
            .and_then(|p| p.parse().ok())
            .unwrap_or_default(),
        publish_period_secs: raw
            .publish_period_secs
            .as_deref()
            .and_then(|p| p.parse().ok())
            .unwrap_or_default(),
        api_base: raw.api_base.clone(),
        log_dir: raw.log_dir.clone(),
        log_keep: raw.log_keep.as_deref().and_then(|v| v.parse().ok()),
        mapping_confirmed: false,
        // The form carries no dates and must not: they are `save`'s, computed from
        // the file being overwritten ([ADR 0039]). This struct exists to re-render
        // what was typed, and nobody typed these.
        created_ms: None,
        last_change_ms: None,
        ui_port: raw.ui_port.as_deref().and_then(|v| v.parse().ok()),
        meters: meters(fields)
            .into_iter()
            .map(|m| StoredMeter {
                meter_id: m.meter_id.unwrap_or_default(),
                device_id: m.device_id.unwrap_or_default(),
                serial: m.serial.unwrap_or_default(),
                enabled: m.enabled.unwrap_or(false),
                priority: m.priority.unwrap_or(false),
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

const STYLE: &str = "<style>\
body{font-family:system-ui,sans-serif;max-width:52rem;margin:2rem auto;padding:0 1rem;line-height:1.5}\
label{display:block;margin:.75rem 0 .2rem;font-weight:600}\
input[type=text],input[type=number]{width:100%;padding:.4rem;font:inherit;box-sizing:border-box}\
fieldset{margin:1rem 0;border:1px solid #8888;padding:.5rem 1rem}\
.fault{color:#a00;font-weight:600;margin:.2rem 0}\
table{border-collapse:collapse;width:100%}td,th{border:1px solid #8888;padding:.35rem .5rem;text-align:left}\
code{word-break:break-all}\
button{font:inherit;padding:.5rem 1rem;margin-top:1rem}\
</style>";

pub(super) fn page(title: &str, body: &str) -> Html<String> {
    Html(format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport \
         content=\"width=device-width,initial-scale=1\"><title>{}</title>{STYLE}{body}",
        escape(title)
    ))
}

/// Does this fault belong to the input named `key`?
///
/// **Keyed on [`Source`], not on `Fault::field`.** `field` is a human label —
/// `"publish period"`, `"meter 0 serial"` — written for a console message, while
/// `Source::File` carries the exact `config.toml` key, which is what these
/// inputs are named after. Matching on `field` bound almost nothing: every fault
/// fell into the lump at the top of the form while the form still *looked*
/// right, because the words the assertion searched for were the field labels.
/// Found by falsification, not by reading.
fn belongs_to(fault: &Fault, key: &str) -> bool {
    matches!(&fault.source, Some(Source::File(k)) if k == key)
}

/// The faults for one input, rendered from what [`Fault`] already carries.
///
/// **Never re-derived.** The message is the one `Fault` already wrote; this
/// module has no opinion about what is wrong with a value.
fn faults_for(errors: Option<&ConfigErrors>, key: &str) -> String {
    let Some(errors) = errors else {
        return String::new();
    };
    errors
        .0
        .iter()
        .filter(|f| belongs_to(f, key))
        .map(|f| format!("<p class=fault>{}</p>", escape(&f.problem)))
        .collect()
}

/// Faults that belong to no input on this form.
///
/// Three kinds land here, and all three must: the environment's — **which must
/// never be rendered as an editable field**, because the operator cannot fix
/// `SMARTME_CLIENT_SECRET` from a browser (AC2) — the whole-configuration ones
/// like `enabled meters`, and any fault about a key this form does not show.
fn orphan_faults(errors: Option<&ConfigErrors>, rendered: &[String]) -> String {
    let Some(errors) = errors else {
        return String::new();
    };
    let orphans: Vec<&Fault> = errors
        .0
        .iter()
        .filter(|f| !rendered.iter().any(|key| belongs_to(f, key)))
        .collect();
    if orphans.is_empty() {
        return String::new();
    }
    let items: String = orphans
        .iter()
        .map(|f| {
            format!(
                "<li><strong>{}</strong> — {}{}</li>",
                escape(&f.field),
                escape(&f.problem),
                match &f.source {
                    Some(source) => format!(" <em>({})</em>", escape(&source.to_string())),
                    None => String::new(),
                }
            )
        })
        .collect();
    format!("<div class=fault><ul>{items}</ul></div>")
}

fn text_input(name: &str, label: &str, value: &str, errors: Option<&ConfigErrors>) -> String {
    format!(
        "<label for={0}>{1}</label>\
         <input type=text id={0} name={0} value=\"{2}\">{3}",
        escape(name),
        escape(label),
        escape(value),
        faults_for(errors, name)
    )
}

/// What one round of discovery produced, for the screen (story 3.4).
///
/// This is a value, not a process: the handler fetches (or picks) and the pure
/// [`discovery_section`] renders — the property lives in the mapping from
/// outcome to what the operator reads, which is where the tests sit. There is
/// deliberately no state between requests: the loop that watches the account
/// over time is story 3.5's subject, and a cached list is exactly the "one
/// instant" hazard the epic warns about.
#[derive(Debug, PartialEq)]
enum Discovery {
    /// The account answered. `dropped` carries serde's reason for every element
    /// that did not parse — rendered, never swallowed (AC1's caveat: a meter
    /// missing from a pick-list with nobody told is a drop-down lying by
    /// omission).
    Listed {
        devices: Vec<DeviceListing>,
        dropped: Vec<String>,
    },
    /// The account answered and has no meters. A state, not a fault.
    Empty,
    /// The account could not be asked, in the error taxonomy's own words —
    /// [`SmartMeError`]'s `Display` already names each repair (story 2.6 AC5),
    /// and this module has no opinion of its own about what went wrong.
    Failed { what: String },
    /// No credential in the environment. Named like every environment fault:
    /// by its variables, never as a box the operator could type into
    /// (ADR 0019/0023 — there is no credential field, so there is nothing to
    /// render but the instruction).
    NoCredential,
    /// A device was picked from the previous round's listing: its pair is now
    /// in the row below, waiting for a name and a deliberate Save.
    Picked { serial: String },
    /// The picked device is already among the rows — a second click adds
    /// nothing, said rather than silently duplicated (the duplicate would be
    /// refused at save by a rule the screen never names). Emitted only on an
    /// EXACT pair match, so the sentence is never false.
    AlreadyMapped { serial: String },
    /// One existing row held half of the picked pair; its pair was corrected
    /// to the account's — the transcription repair the pick-list exists for.
    /// `row` is 1-based, as the fieldset legends number them.
    Corrected { row: usize, serial: String },
}

/// The discovery section of the form, rendered from the outcome alone.
fn discovery_section(discovery: Option<&Discovery>) -> String {
    let mut out = String::from(
        "<fieldset><legend>The account&#39;s meters (smart-me)</legend>\
         <p>Load the meters your smart-me account has, then pick one: picking \
         fills the device id AND the serial together, the pair the bridge \
         verifies on every fetch.</p>\
         <button type=submit formaction=/config/discover formmethod=post>\
         Load the account&#39;s meters</button>",
    );
    match discovery {
        None => {}
        Some(Discovery::Listed { devices, dropped }) => {
            let rows: String = devices
                .iter()
                .map(|d| {
                    format!(
                        "<tr><td>{name}</td><td>{serial}</td><td><code>{id}</code></td>\
                         <td><button type=submit formaction=/config/discover \
                         formmethod=post name=pick value=\"{serial}|{id}\">\
                         Use this meter</button></td></tr>",
                        // A null name shows the serial; nothing invents a name.
                        name = match &d.name {
                            Some(name) => escape(name),
                            None => format!("(unnamed — serial {})", d.serial),
                        },
                        serial = d.serial,
                        id = escape(&d.id),
                    )
                })
                .collect();
            out.push_str(&format!(
                "<table><tr><th>Name</th><th>Serial</th><th>Device id</th><th></th></tr>\
                 {rows}</table>\
                 <p>This list is the account at one instant. Picking from it does \
                 not soften anything: a device that has gone by the next fetch is \
                 still refused, loudly.</p>"
            ));
            for reason in dropped {
                out.push_str(&format!(
                    "<p class=fault>One device could not be read from the account \
                     listing and is NOT shown above: {}</p>",
                    escape(reason)
                ));
            }
        }
        Some(Discovery::Empty) => {
            out.push_str(
                "<p>The account answered: it has no meters. Nothing is wrong with \
                 the bridge or its sign-in — there is simply nothing to pick.</p>",
            );
        }
        Some(Discovery::Failed { what }) => {
            out.push_str(&format!(
                "<p class=fault>The account could not be asked: {}</p>\
                 <p>Typed entry below still works — discovery being down locks \
                 nothing.</p>",
                escape(what)
            ));
        }
        Some(Discovery::NoCredential) => {
            out.push_str(
                "<p class=fault>The environment holds no smart-me client id and \
                 secret (SMARTME_CLIENT_ID / SMARTME_CLIENT_SECRET), so the \
                 account cannot be asked. They are never entered here — set them \
                 in the environment and reload.</p>",
            );
        }
        Some(Discovery::Picked { serial }) => {
            out.push_str(&format!(
                "<p>Meter with serial <strong>{}</strong> added below with its \
                 device id and serial filled in. Give it a name, tick Published \
                 when you mean it, then Save.</p>",
                escape(serial)
            ));
        }
        Some(Discovery::AlreadyMapped { serial }) => {
            out.push_str(&format!(
                "<p>The meter with serial <strong>{}</strong> is already among \
                 the rows below — nothing was added.</p>",
                escape(serial)
            ));
        }
        Some(Discovery::Corrected { row, serial }) => {
            out.push_str(&format!(
                "<p>Meter {row} below held half of this pair; its device id and \
                 serial were corrected to the account&#39;s (serial \
                 <strong>{}</strong>). Check the row, then Save.</p>",
                escape(serial)
            ));
        }
    }
    // WORDING CONSTRAINT, load-bearing: the first-run browser test scans this
    // page for the tokens `client_secret`, `client_id`, `credential` and
    // `password` — the mechanical form of ADR 0019's "no such field, not even
    // evoked". The established convention is "client id" with spaces and the
    // uppercase environment names; GitHub caught the first draft of the
    // sentence below using the forbidden word while every local suite was green.
    out.push_str(
        "<p>The account is asked at the SAVED API base. A base typed above but \
         not yet saved is not used — the file is the configuration, and this \
         button must not be a way to point the bridge&#39;s smart-me sign-in \
         at a host the file does not name.</p></fieldset>",
    );
    out
}

/// Render the whole form. `values` is what to show in the boxes — the
/// submission when there was one, the file otherwise, so a refused save never
/// makes the operator retype what they had entered.
fn form(
    values: &StoredConfig,
    errors: Option<&ConfigErrors>,
    discovery: Option<&Discovery>,
) -> String {
    let meter_rows: String = values
        .meters
        .iter()
        .enumerate()
        .map(|(i, m)| {
            format!(
                "<fieldset><legend>Meter {n}</legend>\
                 <label for=m{i}n>Name (used in the Sparkplug metric path)</label>\
                 <input type=text id=m{i}n name=\"meter.{i}.meter_id\" value=\"{name}\">{f_name}\
                 <label for=m{i}d>smart-me device id</label>\
                 <input type=text id=m{i}d name=\"meter.{i}.device_id\" value=\"{device}\">{f_dev}\
                 <label for=m{i}s>Serial (becomes the device level of the topic)</label>\
                 <input type=text id=m{i}s name=\"meter.{i}.serial\" value=\"{serial}\">{f_ser}\
                 <label><input type=checkbox name=\"meter.{i}.enabled\" value=1 {checked}> \
                 Published</label>\
                 <label><input type=checkbox name=\"meter.{i}.priority\" value=1 {starred}> \
                 One of the meters that matter</label></fieldset>",
                n = i + 1,
                name = escape(&m.meter_id),
                device = escape(&m.device_id),
                serial = escape(&m.serial),
                checked = if m.enabled { "checked" } else { "" },
                // FR35's priority half ([ADR 0039]). **Rendered as well as read**:
                // `posted` takes this field, so a form that did not render it would
                // clear the tick on the next Save — the round-trip defect story 3.4
                // repaired once already, arriving through a new field.
                starred = if m.priority { "checked" } else { "" },
                f_name = faults_for(errors, &format!("meters[{i}].meter_id")),
                f_dev = faults_for(errors, &format!("meters[{i}].device_id")),
                f_ser = faults_for(errors, &format!("meters[{i}].serial")),
            )
        })
        .collect();

    // One always-blank row, so adding a meter needs no JavaScript. An empty row
    // contributes nothing: every field comes back `None` and `validate` is not
    // asked about a meter nobody entered.
    let blank = values.meters.len();
    // Every key this form renders an input for. A fault about anything else goes
    // into the lump at the top, which is where the environment's belong: the
    // operator cannot fix `SMARTME_CLIENT_SECRET` from a browser, so it must not
    // be drawn as a box they can type in.
    let mut named: Vec<String> = [
        "group_id",
        "node_id",
        "broker_host",
        "broker_port",
        "publish_period_secs",
        "api_base",
        "log_dir",
        "log_keep",
        "ui_port",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    for i in 0..values.meters.len() {
        for f in ["meter_id", "device_id", "serial"] {
            named.push(format!("meters[{i}].{f}"));
        }
    }

    // THE HIDDEN FIRST BUTTON IS THE ENTER KEY'S TARGET, and it must stay
    // first. HTML implicit submission activates the first submit button in
    // tree order, and the discovery section put its `formaction=/config/discover`
    // buttons before Save — so pressing Enter in any text box DISCOVERED
    // instead of saving, silently, on a page that re-renders almost
    // identically (the review of story 3.4 walked the scenario: an operator
    // corrects the broker, presses Enter, and walks away unsaved). This
    // button restores Enter-means-Save; it is hidden because the visible Save
    // at the bottom is the affordance, and inert to keyboards and readers.
    format!(
        "{orphans}\
         <form method=post action=/config>\
         <button type=submit hidden aria-hidden=true tabindex=-1>Save</button>\
         {group}{node}{host}{port}{period}\
         <fieldset><legend>Meters</legend>{meter_rows}\
         <fieldset><legend>Add a meter</legend>\
         <label for=nb>Name</label><input type=text id=nb name=\"meter.{blank}.meter_id\" value=\"\">\
         <label for=db>smart-me device id</label><input type=text id=db name=\"meter.{blank}.device_id\" value=\"\">\
         <label for=sb>Serial</label><input type=text id=sb name=\"meter.{blank}.serial\" value=\"\">\
         <label><input type=checkbox name=\"meter.{blank}.enabled\" value=1> Published</label>\
         <label><input type=checkbox name=\"meter.{blank}.priority\" value=1> One of the \
         meters that matter</label>\
         </fieldset>{discover}</fieldset>\
         <fieldset><legend>Optional</legend>{api}{logdir}{logkeep}{uiport}</fieldset>\
         <button type=submit>Save</button></form>",
        orphans = orphan_faults(errors, &named),
        group = text_input("group_id", "Sparkplug group id", &values.group_id, errors),
        node = text_input("node_id", "Sparkplug edge node id", &values.node_id, errors),
        host = text_input(
            "broker_host",
            "MQTT broker host",
            &values.broker_host,
            errors
        ),
        port = text_input(
            "broker_port",
            "MQTT broker port",
            &values.broker_port.to_string(),
            errors
        ),
        period = text_input(
            "publish_period_secs",
            "Publish period, seconds",
            &values.publish_period_secs.to_string(),
            errors
        ),
        api = text_input(
            "api_base",
            "smart-me API base (blank for the default)",
            values.api_base.as_deref().unwrap_or(""),
            errors
        ),
        logdir = text_input(
            "log_dir",
            "Log directory (blank for console only)",
            values.log_dir.as_deref().unwrap_or(""),
            errors
        ),
        logkeep = text_input(
            "log_keep",
            "Log files to keep",
            &values.log_keep.map(|k| k.to_string()).unwrap_or_default(),
            errors
        ),
        uiport = text_input(
            "ui_port",
            "Web UI port (takes effect at the next restart)",
            &values.ui_port.map(|p| p.to_string()).unwrap_or_default(),
            errors
        ),
        discover = discovery_section(discovery),
    )
}

/// What the file holds, or a blank slate on a first run.
fn current_or_blank(state_dir: &std::path::Path) -> StoredConfig {
    store::read(state_dir).unwrap_or_else(|_| StoredConfig {
        schema_version: store::SCHEMA_VERSION,
        // A blank slate has no history, and `save` will stamp the creation the
        // first time this is written ([ADR 0039]).
        created_ms: None,
        last_change_ms: None,
        group_id: String::new(),
        node_id: String::new(),
        broker_host: String::new(),
        broker_port: 1883,
        publish_period_secs: config::PERIOD_DEFAULT.as_secs(),
        api_base: None,
        log_dir: None,
        log_keep: None,
        mapping_confirmed: false,
        ui_port: None,
        meters: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub(super) async fn config_form(State(state): State<Arc<UiState>>) -> Response {
    let values = current_or_blank(state.state_dir());
    // THE FAULTS COME FROM `validate`, NOT ONLY FROM `read`.
    //
    // This used to be `store::read(...).err()`, which covers a TOML parse
    // failure and a schema mismatch and nothing else. A file that parses but
    // that the bridge refuses to boot on — the exact file a `publish_period_secs
    // = 0` produced — was therefore rendered with **zero faults** and the
    // offending value shown as if it were fine. The one screen an operator opens
    // to find out why the bridge is silent was the one screen that did not run
    // the validation.
    //
    // A file that is absent is not a fault: that is a first run, and the form is
    // blank on purpose.
    let errors = if store::exists(state.state_dir()) {
        match store::read(state.state_dir()) {
            Err(errors) => Some(errors),
            Ok(stored) => {
                let credential = Credential {
                    client_id: std::env::var("SMARTME_CLIENT_ID").ok(),
                    client_secret: std::env::var("SMARTME_CLIENT_SECRET").ok(),
                };
                config::validate(store::into_raw(stored, credential, state.state_dir())).err()
            }
        }
    } else {
        None
    };
    page(
        "Configuration — smartme_mqtt",
        &form(&values, errors.as_ref(), None),
    )
    .into_response()
}

/// The PER-REQUEST budget for discovery — and a round is TWO requests (token,
/// then listing), so the worst wait is about twice this. The first version
/// claimed 10 s while delivering up to 20; the review read the constant against
/// the code and the doc lost. 5 s per request keeps the worst round near the
/// 10 s an operator will actually wait out, and a timeout that RENDERS as "the
/// account could not be asked" beats a browser spinner — the taxonomy message
/// is the feature.
const DISCOVERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// `POST /config/discover` — the account's meters, on demand (story 3.4).
///
/// Reading smart-me is NOT publishing (the story's decision 3, drawing the 5.3
/// boundary): this handler runs in every phase, confirmed or not, and by
/// construction it publishes nothing, adopts nothing, and writes no
/// configuration — it fetches (or picks) and renders. The submitted form
/// values ride along and come back in the boxes, so pressing the button does
/// not cost the operator their unsaved edits.
///
/// A PICK does not refetch: the pair travelled in the button's value from the
/// listing the operator was just shown, and asking the account again would
/// confirm nothing more — the pair is verified where it has always been,
/// against every response, by ADR 0029.
pub(super) async fn discover(
    State(state): State<Arc<UiState>>,
    headers: HeaderMap,
    Form(fields): Form<Fields>,
) -> Response {
    if let Some(refusal) = super::origin::refusal(&headers) {
        return refusal;
    }
    let raw = posted(&fields, state.state_dir());
    let mut values = as_typed(&fields, &raw);
    // THE FAULTS RENDER HERE TOO — the review of this story found the omission.
    // `as_typed` is lossy by design (a mistyped port re-renders as its default,
    // an unparsable value as blank), and the save path pairs that loss with the
    // fault beside the box. Rendering with `None` here made the discover round
    // trip the one door through which a value could be rewritten WITHOUT a word
    // — the exact `publish_period_secs = 0` incident `as_typed`'s own doc
    // memorialises, reopened. Same helper, same faults, both paths.
    //
    // ONLY WHEN SOMETHING WAS TYPED, though — the review of the repair caught
    // the unconditional version rendering a page of refusals on a pristine
    // first run for a save nobody attempted, while `GET /config` deliberately
    // renders zero faults on the same state ("a file that is absent is not a
    // fault"). The rewrite hazard exists exactly when a value was entered, so
    // that is exactly when the faults render.
    let anything_typed = [
        &raw.group_id,
        &raw.node_id,
        &raw.broker_host,
        &raw.broker_port,
        &raw.publish_period_secs,
        &raw.api_base,
        &raw.log_dir,
        &raw.log_keep,
        &raw.ui_port,
    ]
    .iter()
    .any(|v| v.is_some())
        || !raw.meters.is_empty();
    let errors = anything_typed
        .then(|| config::validate(raw.clone()).err())
        .flatten();

    let outcome = if let Some(pick) = field(&fields, "pick") {
        match pick.split_once('|') {
            Some((serial, device_id)) if !serial.is_empty() && !device_id.is_empty() => {
                // FOUR CASES, and each says the truth about itself — the review
                // of the repair caught the first dedup LYING: a plain OR refused
                // any half-match with "already among the rows", which is false
                // when no row carries that serial, and it blocked the pick's one
                // repair use — correcting a row whose other half was mistyped.
                //
                //  - the EXACT pair is already a row: a second click adds
                //    nothing (the listing vanishes on the way back, so "did
                //    that register?" ends in Back-and-click-again, and a
                //    duplicate is refused at save by a rule the screen never
                //    names);
                //  - exactly ONE row holds half of the pair: that row's pair is
                //    corrected — the account is the authority on which id goes
                //    with which serial, and this is precisely the transcription
                //    repair the pick-list exists for;
                //  - two DIFFERENT rows each hold a half: correcting either
                //    would silently merge two meters; said, and left to the
                //    human;
                //  - no overlap: a new row, pair filled. The operator names it
                //    (a smart-me display name is not a Sparkplug topic level)
                //    and Published stays a deliberate tick — an unconfirmed
                //    mapping must gain nothing publishable from convenience.
                let exact = values
                    .meters
                    .iter()
                    .any(|m| m.device_id == device_id && m.serial == serial);
                let halves: Vec<usize> = values
                    .meters
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.device_id == device_id || m.serial == serial)
                    .map(|(i, _)| i)
                    .collect();
                if exact {
                    Discovery::AlreadyMapped {
                        serial: serial.to_string(),
                    }
                } else {
                    match halves.as_slice() {
                        [] => {
                            values.meters.push(StoredMeter {
                                meter_id: String::new(),
                                device_id: device_id.to_string(),
                                serial: serial.to_string(),
                                enabled: false,
                                // A picked row is a transcription, not a judgement
                                // about what matters: the operator ticks both boxes
                                // deliberately or neither is claimed.
                                priority: false,
                            });
                            Discovery::Picked {
                                serial: serial.to_string(),
                            }
                        }
                        [row] => {
                            values.meters[*row].device_id = device_id.to_string();
                            values.meters[*row].serial = serial.to_string();
                            Discovery::Corrected {
                                row: *row + 1,
                                serial: serial.to_string(),
                            }
                        }
                        _ => Discovery::Failed {
                            what: format!(
                                "two different rows below each hold half of this \
                                 pair (rows {}), so a correction would merge two \
                                 meters; nothing was changed — fix the rows by \
                                 hand",
                                halves
                                    .iter()
                                    .map(|i| (i + 1).to_string())
                                    .collect::<Vec<_>>()
                                    .join(" and ")
                            ),
                        },
                    }
                }
            }
            _ => Discovery::Failed {
                what: "the pick did not carry a serial|device-id pair; use the \
                       buttons in the listing"
                    .to_string(),
            },
        }
    } else {
        fetch_listing(state.state_dir()).await
    };

    page(
        "Configuration — smartme_mqtt",
        &form(&values, errors.as_ref(), Some(&outcome)),
    )
    .into_response()
}

/// Which base URL discovery asks — the SAVED one, never the submitted one.
///
/// **This is the review's gravest finding repaired, and the rule is ADR 0023's
/// own letter**: the file is the configuration, and a form value that has not
/// been saved is not the configuration. The first version took the submitted
/// `api_base`, and `fetch_token` POSTs the client credential to
/// `{base}/oauth/token` — so one un-Origined request (`origin::refusal`'s
/// documented curl pass-through) could hand `SMARTME_CLIENT_SECRET` to any
/// https host, persisting nothing and leaving no trace. Reading the STORE
/// restores the pre-story invariant: a request-supplied base reaches the
/// credential only by being validated AND written to disk first, where the
/// operator can see it.
/// FAIL-CLOSED on an unreadable file — the review of the repair caught the
/// first version failing OPEN: `store::read(..).ok()` silently fell back to
/// the default base, so a schema bump or a permissions regression would have
/// sent the sign-in to api.smart-me.com while the operator's saved mirror sat
/// unread and the screen claimed the SAVED base was asked. Absence is not
/// invalidity (ADR 0023 §5): no file means the default is right; a file that
/// cannot be read means nothing may be asked at all.
///
/// One residual, stated rather than hidden: an un-Origined client that can
/// POST `/config` can still SAVE a hostile base, discover, and restore — the
/// window rides with the UI's no-authentication posture (ADR 0019, [#56]'s
/// world), not with this function; what this function guarantees is only that
/// the base always comes from the file the operator can inspect.
fn discovery_base(state_dir: &std::path::Path) -> Result<String, String> {
    if !store::exists(state_dir) {
        return Ok(SmartMeClient::DEFAULT_BASE.to_string());
    }
    match store::read(state_dir) {
        Ok(stored) => Ok(stored
            .api_base
            .unwrap_or_else(|| SmartMeClient::DEFAULT_BASE.to_string())),
        Err(errors) => Err(errors.to_string()),
    }
}

/// One fetch of the account listing, mapped to what the screen renders.
///
/// The credential comes from the environment — never from the form, which has
/// no field for it — and the base from the FILE ([`discovery_base`]); the one
/// thing the submission contributes to a fetch is the click. Trimmed like
/// `config::present` trims, because a trailing newline from a `docker env_file`
/// otherwise turns a working credential into a rendered 401 the bridge itself
/// does not have. No token reuse and no retry-once here, recorded rather than
/// hidden: discovery is stateless by decision 2, so a server-side hiccup renders
/// its taxonomy line and the operator's retry is the retry.
async fn fetch_listing(state_dir: &std::path::Path) -> Discovery {
    let client_id = std::env::var("SMARTME_CLIENT_ID")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let client_secret = std::env::var("SMARTME_CLIENT_SECRET")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let (Some(client_id), Some(client_secret)) = (client_id, client_secret) else {
        return Discovery::NoCredential;
    };
    let base = match discovery_base(state_dir) {
        Ok(base) => base,
        Err(why) => {
            return Discovery::Failed {
                what: format!(
                    "the saved configuration could not be read, so the saved API \
                     base is unknown and nothing was asked: {why}"
                ),
            };
        }
    };
    let client = match SmartMeClient::new(
        base,
        Credentials::ClientCredentials {
            client_id,
            client_secret,
        },
        DISCOVERY_TIMEOUT,
    ) {
        Ok(client) => client,
        Err(refused) => return failed(&refused),
    };
    let token = match client.fetch_token().await {
        Ok(token) => token,
        Err(error) => return failed(&error),
    };
    match client.get_devices(Some(&token)).await {
        // Empty means EMPTY: no devices and nothing dropped. A listing whose
        // only devices failed to parse must render its caveats, not a shrug.
        Ok(list) if list.devices.is_empty() && list.dropped.is_empty() => Discovery::Empty,
        Ok(list) => Discovery::Listed {
            devices: list.devices,
            dropped: list.dropped,
        },
        Err(error) => failed(&error),
    }
}

/// The taxonomy wording is [`SmartMeError`]'s own (story 2.6 AC5 wrote each
/// repair into `Display`); this module adds no opinion of its own.
fn failed(error: &SmartMeError) -> Discovery {
    Discovery::Failed {
        what: error.to_string(),
    }
}

pub(super) async fn save_config(
    State(state): State<Arc<UiState>>,
    headers: HeaderMap,
    Form(fields): Form<Fields>,
) -> Response {
    if let Some(refusal) = super::origin::refusal(&headers) {
        return refusal;
    }

    let raw = posted(&fields, state.state_dir());
    let validated = match config::validate(raw.clone()) {
        Ok(config) => config,
        // AC2 — every fault at once, each beside its field, in the words
        // `Fault` already carries.
        Err(errors) => {
            return (
                StatusCode::BAD_REQUEST,
                page(
                    "Configuration — smartme_mqtt",
                    &form(&as_typed(&fields, &raw), Some(&errors), None),
                ),
            )
                .into_response();
        }
    };

    // Built from the VALIDATED struct, never from the raw strings — see
    // `as_typed`'s docs for the container-bricking defect that came of the
    // latter.
    if let Err(error) = store::save(
        state.state_dir(),
        &StoredConfig::from(&validated),
        state.wall_now(),
    ) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            page(
                "Configuration — smartme_mqtt",
                &format!(
                    "<p class=fault>The configuration was NOT written: {}. \
                     Nothing has changed.</p><p><a href=/config>Back</a></p>",
                    escape(&error.to_string())
                ),
            ),
        )
            .into_response();
    }

    // Did this write withdraw the confirmation?
    //
    // `save` computes the flag rather than taking it, so the only honest way to
    // know is to ask the file it just wrote. Absent or unreadable is treated as
    // withdrawn, which is the safe direction.
    let withdrawn = !matches!(store::read(state.state_dir()), Ok(s) if s.mapping_confirmed);

    // AC4 — what it cost, and what it did NOT cost.
    //
    // Only a running bridge has a control surface to apply to. A silent one has
    // nothing in force to compare against, so the honest report is that the
    // saved values take effect when it starts publishing.
    let report = match state.phase().control() {
        // A CHANGE THAT WITHDREW THE CONFIRMATION IS NOT CARRIED TO THE WIRE.
        //
        // `store::save` clears `mapping_confirmed` when the meter set changed,
        // and until 2026-08-05 this handler then called `apply` anyway — four
        // lines later, on the same submission. The withdrawal was written to
        // disk and had no effect at all: the bridge published `DDEATH` for the
        // old device and `DBIRTH` for the new one immediately, so a SCADA host
        // acquired a persisted tag folder for a mapping no human had ever
        // vouched for. That is FR25 defeated through the very screen built to
        // enforce it — `prd.md:136` calls the confirmation *"the only guard
        // against a mis-map the machine cannot detect"*, and a guard that the
        // save path walks straight past is not a guard.
        //
        // The bridge keeps publishing what it was already publishing. Nothing on
        // the wire changes until a human has looked at the new mapping.
        Some(_) if withdrawn => "<p><strong>Saved — but NOT in force, and the meter \
             mapping is no longer confirmed.</strong></p>\
             <p>What you changed alters which meter is published where, so the \
             confirmation given for the previous mapping no longer applies to it. \
             The bridge is still publishing the <em>old</em> mapping and will keep \
             doing so.</p>\
             <p>Review the new mapping below and confirm it, then restart the \
             bridge to put it in force. Until you do, a restart would bring the \
             bridge up publishing nothing.</p>"
            .to_string(),
        Some(control) => {
            let plan = control.apply(validated).await;
            // Re-read what is IN FORCE, rather than echoing the submission.
            // `current()` deliberately keeps reporting the old value for
            // anything not applied, and rendering the posted value instead would
            // show the operator a change that has not happened.
            let in_force = control.current();
            cost_report(&plan, in_force.poll.interval.as_secs())
        }
        None => "<p>Saved. Nothing is published yet — confirm the meter mapping \
                 below to start.</p>"
            .to_string(),
    };

    // The nudge carries no configuration: the lifecycle loop re-reads the file.
    state.notify_ready();

    page(
        "Saved — smartme_mqtt",
        &format!(
            "{report}<p><a href=/confirm>Review and confirm the meter mapping</a> · \
             <a href=/config>Back to the configuration</a></p>"
        ),
    )
    .into_response()
}

/// Render a [`Plan`](crate::app::reconfigure::Plan) in the operator's terms.
fn cost_report(plan: &crate::app::reconfigure::Plan, period_in_force: u64) -> String {
    if plan.is_empty() {
        return "<p>Saved. Nothing changed, so nothing was disturbed.</p>".to_string();
    }
    let headline = match plan.cost() {
        None => "Saved.".to_string(),
        Some(Cost::Hot) => "Saved, and in force now.".to_string(),
        Some(Cost::DeviceCertificate) => {
            "Saved. One or more devices were re-announced on the same session.".to_string()
        }
        Some(Cost::NewSession) => {
            "Saved — but NOT in force. What changed needs a new Sparkplug session, \
             which this bridge cannot open without being restarted."
                .to_string()
        }
        Some(Cost::ProcessRestart) => {
            "Saved — but NOT in force. What changed takes effect the next time the \
             bridge starts."
                .to_string()
        }
    };
    let held = plan.needs_restart();
    let waiting = if held.is_empty() {
        String::new()
    } else {
        let names: Vec<String> = held
            .iter()
            .map(|f| format!("<code>{}</code>", escape(f)))
            .collect();
        format!("<p>Waiting for a restart: {}</p>", names.join(", "))
    };
    // Certificates the driver never accepted. Empty is the normal case and the
    // claim; naming them is what stops the screen reporting a bury that was
    // dropped on the floor.
    let undelivered = if plan.undelivered.is_empty() {
        String::new()
    } else {
        let names: Vec<String> = plan
            .undelivered
            .iter()
            .map(|s| format!("<code>{}</code>", escape(s.as_str())))
            .collect();
        format!(
            "<p class=fault>These devices were NOT announced to the broker: {}. \
             The bridge could not reach its own publishing task, so a SCADA host \
             still shows whatever it last saw. Check the log.</p>",
            names.join(", ")
        )
    };
    let changes: String = plan
        .changes
        .iter()
        .map(|c| {
            format!(
                "<tr><td><code>{}</code></td><td>{}</td></tr>",
                escape(c.field),
                match c.cost {
                    Cost::Hot => "in force now",
                    Cost::DeviceCertificate => "one device re-announced",
                    Cost::NewSession => "needs a new session — not in force",
                    Cost::ProcessRestart => "needs a restart — not in force",
                }
            )
        })
        .collect();
    format!(
        "<p><strong>{headline}</strong></p>{undelivered}{waiting}\
         <table><tr><th>Setting</th><th>What happened</th></tr>{changes}</table>\
         <p>The publish period in force is {period_in_force} s.</p>"
    )
}

pub(super) async fn confirm_form(State(state): State<Arc<UiState>>) -> Response {
    let stored = match store::read(state.state_dir()) {
        Ok(stored) => stored,
        Err(errors) => {
            return (
                StatusCode::BAD_REQUEST,
                page(
                    "Confirm the mapping — smartme_mqtt",
                    &format!(
                        "<p class=fault>{}</p><p><a href=/config>Configure the bridge</a></p>",
                        escape(&errors.to_string())
                    ),
                ),
            )
                .into_response();
        }
    };

    let credential = Credential {
        client_id: std::env::var("SMARTME_CLIENT_ID").ok(),
        client_secret: std::env::var("SMARTME_CLIENT_SECRET").ok(),
    };
    let fingerprint = mapping_fingerprint(&stored);
    let validated = match config::validate(store::into_raw(stored, credential, state.state_dir())) {
        Ok(config) => config,
        Err(errors) => {
            return (
                StatusCode::BAD_REQUEST,
                page(
                    "Confirm the mapping — smartme_mqtt",
                    &format!(
                        "<p class=fault>{}</p><p><a href=/config>Fix the configuration</a></p>",
                        escape(&errors.to_string())
                    ),
                ),
            )
                .into_response();
        }
    };

    let rows = match config::mapping_preview(&validated) {
        Ok(rows) => rows,
        // A preview that quietly differed from what is published would be a
        // check that passes for the wrong reason.
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                page(
                    "Confirm the mapping — smartme_mqtt",
                    &format!(
                        "<p class=fault>The topics cannot be built, so there is nothing \
                         honest to show: {}</p>",
                        escape(&error.to_string())
                    ),
                ),
            )
                .into_response();
        }
    };

    let body: String = rows
        .iter()
        .map(|r| {
            format!(
                "<tr><td>{}</td><td><code>{}</code></td><td><code>{}</code></td>\
                 <td><code>{}</code></td><td>{}</td></tr>",
                escape(&r.meter),
                escape(&r.serial),
                escape(&r.device_id),
                escape(&r.topic),
                if r.enabled {
                    "published"
                } else {
                    "not published"
                }
            )
        })
        .collect();

    page(
        "Confirm the mapping — smartme_mqtt",
        &format!(
            "<h1>Confirm the meter mapping</h1>\
             <p>Check the <strong>serial</strong> against the <strong>topic</strong> on \
             each row. A name that looks right is exactly the part that looks right when \
             it is wrong, and a SCADA host keeps the tags it discovers.</p>\
             <table><tr><th>Meter</th><th>Serial</th><th>smart-me device id</th>\
             <th>Topic</th><th></th></tr>{body}</table>\
             <form method=post action=/confirm>\
             <input type=hidden name=mapping value=\"{fingerprint}\">\
             <button type=submit>These are correct — start publishing</button></form>\
             <p><a href=/config>Something is wrong — change the configuration</a></p>"
        ),
    )
    .into_response()
}

/// A fingerprint of exactly what the confirmation screen showed.
///
/// **This is what the click is bound to.** `store::confirm` blesses whatever is
/// on disk when it runs, so without this any write interleaved between the
/// preview and the click would be confirmed instead — the operator would have
/// looked at one mapping and vouched for another. Cheap to add now, and Story
/// 6.2 is what gives `confirm` its first caller.
///
/// Story 5.3 chose a boolean over a fingerprint *in the file*, and that stands:
/// a stored fingerprint cannot be written by hand, and FR23 promises a headless
/// bring-up that does exactly that. This one is never stored — it lives only
/// between a rendered page and its submission.
fn mapping_fingerprint(stored: &StoredConfig) -> String {
    // ONE reader of ONE projection since story 3.4 (AC5, [#64]). This function
    // used to format four meter fields by name, so a field added to
    // `StoredMeter` was invisible to it while `store::same_mapping`'s derived
    // `==` counted it automatically — two answers to "is this the mapping a
    // human looked at". Both now read `store::mapping_projection`, whose
    // exhaustive destructure stops the build until a new field is classified.
    let joined = store::mapping_projection(stored).canonical();
    // FNV-1a, 64-bit. Not a security boundary — it guards against an accidental
    // interleaved write, not against somebody who can already post to this UI.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in joined.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{hash:016x}")
}

pub(super) async fn confirm_mapping(
    State(state): State<Arc<UiState>>,
    headers: HeaderMap,
    Form(fields): Form<Fields>,
) -> Response {
    if let Some(refusal) = super::origin::refusal(&headers) {
        return refusal;
    }

    let shown = field(&fields, "mapping").unwrap_or_default().to_string();
    let stored = match store::read(state.state_dir()) {
        Ok(stored) => stored,
        Err(errors) => {
            return (
                StatusCode::BAD_REQUEST,
                page(
                    "Confirm the mapping — smartme_mqtt",
                    &format!("<p class=fault>{}</p>", escape(&errors.to_string())),
                ),
            )
                .into_response();
        }
    };

    // The guard: confirm what was SHOWN, or nothing.
    if mapping_fingerprint(&stored) != shown {
        return (
            StatusCode::CONFLICT,
            page(
                "Confirm the mapping — smartme_mqtt",
                "<p class=fault>The configuration changed between the mapping you were \
                 shown and this click, so nothing was confirmed. Nothing is published.</p>\
                 <p><a href=/confirm>Look at the mapping again</a></p>",
            ),
        )
            .into_response();
    }

    if let Err(errors) = store::confirm(state.state_dir()) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            page(
                "Confirm the mapping — smartme_mqtt",
                &format!("<p class=fault>{}</p>", escape(&errors.to_string())),
            ),
        )
            .into_response();
    }

    // AC7 — the click that ends the silence. The loop re-reads the file and
    // enters the publishing phase; nothing here tells it what to publish.
    state.notify_ready();
    Redirect::to("/").into_response()
}

/// **Story 6.4 — the meter view.** FR28, FR30, FR34 and FR36 on one page.
///
/// # It reads; it does not judge
///
/// AR19: *"UI consumes this state, never recomputes it."* Every verdict, quality
/// and culprit on this page comes from `MeterState`, written by the poll loop that
/// reached the judgement. The only things derived here are **words** — the repair
/// gesture and the relative ages — because story 6.3 AC4 keeps text out of the
/// state, where it would be built under a lock every poll task waits on.
pub(super) async fn meter_view(State(state): State<Arc<UiState>>) -> impl IntoResponse {
    let phase = state.phase();
    let Some(control) = phase.control() else {
        // FR32's first state: nothing is configured, and an empty table would read
        // as "all quiet" — the exact misreading this requirement exists to prevent.
        return page(
            "Meters",
            "<h1>Meters</h1><p>The bridge is not publishing. There is no meter list \
             because no configuration has been confirmed yet.</p>\
             <p><a href=/config>Configure it</a></p>",
        );
    };
    let config = control.current();
    let fleet = phase.fleet();
    let now = control.clock().wall();

    let Some(fleet) = fleet else {
        return page(
            "Meters",
            "<h1>Meters</h1><p>The bridge is starting: no meter has completed a poll \
             cycle yet. This is not an error, and it is not silence — come back in \
             one period.</p>",
        );
    };

    let mut rows = String::new();
    for meter in &fleet.meters {
        let configured = config.meters.iter().find(|m| m.meter == meter.meter);
        let serial = configured.map(|m| m.serial.as_str());
        // THE TOPIC IS BUILT BY THE PUBLISHER'S OWN PATH, never spelled here: a
        // page that concatenated it could show a topic the grammar refuses and the
        // bridge would never publish on.
        //
        // **AND NO TOPIC IS SHOWN WITHOUT A SERIAL.** The first draft passed the
        // dash through, and a falsification run printed the result:
        // `spBv1.0/G/DDATA/N/—` — a topic nothing will ever publish on, rendered
        // as though it were the destination. A meter the running configuration no
        // longer carries has no topic, and saying so is the honest answer.
        let topic = serial
            .and_then(|serial| {
                sparkplug_b::EdgeNode::new(&config.group_id, &config.node_id)
                    .ok()
                    .and_then(|node| {
                        node.device_topic(sparkplug_b::MessageType::DData, serial)
                            .ok()
                    })
            })
            .map_or_else(|| "—".to_string(), |t| t.to_string());
        let serial = serial.unwrap_or("—");

        let power = meter
            .last_power_kw
            .map(|v| format!("{v:.3} kW"))
            // `None` is "nothing published yet", never "0" — FR16's rule reaching
            // the screen: a missing value is never a substituted one.
            .unwrap_or_else(|| "—".to_string());
        let energy = meter
            .last_energy_kwh
            .map(|v| format!("{v:.3} kWh"))
            .unwrap_or_else(|| "—".to_string());

        // FR28's FRESHNESS AGE, and it is the reading's own age — not the age of
        // our last publication.
        //
        // **Added by the review of story 6.4 (2026-08-20).** The page shipped with
        // eight of AC2's nine columns: `last_published_at` and `last_changed_at`
        // were there, and the age of the measurement itself was not — so story 6.3
        // stored `source_value_date` and `staleness_threshold_ms` for a consumer
        // that did not exist, and the one number FR28 names by that name was the
        // one missing. The two are different questions: a bridge republishing every
        // ten seconds has a fresh publication instant and may be carrying a reading
        // an hour old.
        //
        // **The threshold travels with the age** (story 6.3 AC1): an age read
        // against a different threshold than the one that judged it is a different
        // judgement, and an operator comparing "four minutes" against a bound they
        // have to remember is being asked to redo the oracle's work in their head.
        let freshness = match (meter.source_value_date, meter.staleness_threshold_ms) {
            (Some(measured), Some(threshold)) => format!(
                "{} (stale past {} s)",
                ago(now, measured),
                threshold / 1_000
            ),
            (Some(measured), None) => ago(now, measured),
            // Nothing published yet, or an opinion retired with a disabled meter.
            // Absent, never zero — the same rule the two values above apply.
            (None, _) => "—".to_string(),
        };

        let quality = meter
            .published
            .map(|v| {
                v.cause().map_or_else(
                    || format!("{:?}", v.quality()),
                    |c| format!("{:?} ({})", v.quality(), c.as_str()),
                )
            })
            .unwrap_or_else(|| "not yet judged".to_string());

        let published = meter
            .last_published_at
            .map(|at| ago(now, at))
            .unwrap_or_else(|| "never".to_string());
        // FR30 AND AC5's pair: "published a second ago, changed an hour ago" is a
        // frozen meter; the same two words with the same value is a quiet one.
        let changed = meter
            .last_changed_at
            .map(|at| ago(now, at))
            .unwrap_or_else(|| "never".to_string());

        // THE GESTURE IS THE CAUSE'S when there is one (story 6.8, [#103]).
        //
        // `Culprit::repair` names three repairs where the cause knows which one it
        // is — *"a credential, a serial or a device id"* — and it stays for the one
        // case that has no cause: a reading the bridge itself lost, which is
        // `DropReason`'s.
        let blame = meter.culprit.map_or_else(
            || "—".to_string(),
            |c| {
                let gesture = meter.published.and_then(|v| v.cause()).map_or_else(
                    || repair(c).to_string(),
                    |cause| cause.gesture().to_string(),
                );
                format!("{} — {}", c.as_str(), gesture)
            },
        );

        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{power}</td><td>{energy}</td>\
             <td>{freshness}</td>\
             <td>{}</td><td>{published}</td><td>{changed}</td><td>{}</td></tr>",
            escape(meter.meter.as_str()),
            escape(serial),
            escape(&topic),
            escape(&quality),
            escape(&blame),
        ));
    }

    // FR29's two healths, side by side and INDEPENDENT. "Nothing is being
    // published" is useless without which end to look at: a source that answers
    // nothing and a broker that is gone produce the same silence on the wire and
    // need opposite gestures.
    //
    // **The sentence is built in ONE place** — the state screen says the same
    // thing, and two spellings of "the broker is unreachable" is two places for
    // the truth to live (the review of story 6.5, 2026-08-20).
    let sink_line = sink_health_line(control.sink(), now);

    page(
        "Meters",
        &format!(
            "<h1>Meters</h1><p>{sink_line}</p>\
             <table><tr><th>Meter</th><th>Serial</th><th>Topic</th><th>Power</th>\
             <th>Energy</th><th>Reading age</th><th>Published as</th>\
             <th>Last published</th>\
             <th>Last changed</th><th>Whose fault</th></tr>{rows}</table>\
             <p><strong>Reading age</strong> is how old the measurement itself \
             is, beside the threshold it was judged against. <strong>Last \
             published</strong> is every cycle; <strong>last changed</strong> is \
             when the meter last measured something new. A gap between the last two \
             is a meter that has stopped moving — not a bridge that has stopped \
             publishing.</p>\
             <p><a href=/check>Check one meter end to end</a> · \
             <a href=/>State of the bridge</a> · <a href=/config>Configuration</a></p>"
        ),
    )
}

/// What to do about a fault, derived from its culprit (story 6.4 AC4).
///
/// **Derived, never stored**: story 6.3 AC4 keeps `String`s out of the fleet state,
/// which is written under a lock every poll task waits on.
/// FR35's context line: what the bridge knows about its own configuration.
///
/// **Reads the FILE**, because the file is the configuration ([ADR 0023]) and the
/// dates beside the counts are that file's own. A line built from the settings in
/// force would disagree with its own timestamps the moment a cold change was saved
/// and not yet carried out.
///
/// **A date the file does not carry is said to be unknown, with the reason.** The
/// alternative was the file's mtime, and [ADR 0039] refused it: a `docker cp`, a
/// restore, a `touch` or an image update rewriting the volume all move it, so the
/// line would carry a plausible date that is not the date of anything this bridge
/// did — on the one screen whose job is to orient somebody at three in the morning.
///
/// [ADR 0023]: ../../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md
/// [ADR 0039]: ../../../docs/adr/0039-the-configuration-remembers-when-it-was-written-and-which-meters-matter.md
pub(super) fn configuration_context(
    state_dir: &std::path::Path,
    now: crate::domain::UtcMillis,
) -> String {
    let Ok(stored) = store::read(state_dir) else {
        // Unconfigured, or a file this build cannot read. Both are already named
        // by the lifecycle line above; repeating a guess here would add nothing
        // and could contradict it.
        return String::new();
    };
    let meters = stored.meters.len();
    let priority = stored.meters.iter().filter(|m| m.priority).count();
    let created = stored.created_ms.map_or_else(
        || "created before this bridge recorded creation dates, so that is unknown".to_string(),
        |ms| format!("created {}", ago(now, crate::domain::UtcMillis(ms))),
    );
    let changed = stored.last_change_ms.map_or_else(
        || "last change unknown for the same reason".to_string(),
        |ms| format!("last changed {}", ago(now, crate::domain::UtcMillis(ms))),
    );
    format!(
        "<p>This configuration was {created}, {changed}. It carries {meters} \
         {meter_word}, {priority} of them marked as mattering.</p>",
        meter_word = if meters == 1 { "meter" } else { "meters" },
    )
}

/// The broker's own health, in the words an operator reads (FR29, story 6.5).
///
/// **Shared by the meter page and the state screen**, because the review of story
/// 6.5 found the sink named on `/meters` only: the page a human opens first said
/// the bridge "is polling the meters and publishing what it reads" and that the
/// broker's reachability "is not reported here yet" — about a bridge that had just
/// learned it. Data in, words out, exactly like [`repair`]: nothing formatted is
/// stored (story 6.3 AC4).
pub(super) fn sink_health_line(
    sink: Option<crate::app::mqtt_driver::SinkState>,
    now: crate::domain::UtcMillis,
) -> String {
    match sink {
        // `None` is not `Disconnected`: a bridge that never reached the broker has
        // not lost anything, and reporting a loss sends an operator after an outage
        // that did not happen.
        None => "The broker: <strong>never connected</strong> since this bridge \
                 started. Nothing has been published, and nothing was lost — check \
                 the broker address in the configuration."
            .to_string(),
        Some(s) if s.connected => format!(
            "The broker: <strong>connected</strong>, {}.",
            ago(now, s.since)
        ),
        Some(s) => format!(
            "The broker: <strong>unreachable</strong> since {}. Readings are still \
             judged and their verdicts still stand; what stops here is delivery. \
             This is not a bridge fault and restarting it repairs nothing.",
            ago(now, s.since)
        ),
    }
}

pub(super) fn repair(culprit: crate::core::oracle::Culprit) -> &'static str {
    use crate::core::oracle::Culprit;
    match culprit {
        Culprit::World => "nothing to do here; the source or the broker has to come back",
        Culprit::You => "open the configuration: a credential, a serial or a device id is wrong",
        Culprit::Bridge => "the bridge lost the reading itself — read the log, then report it",
    }
}

/// A human-readable age. Absolute instants belong in the log; a screen read at
/// three in the morning wants "four minutes ago" (FR36).
pub(super) fn ago(now: crate::domain::UtcMillis, then: crate::domain::UtcMillis) -> String {
    let seconds = (now.0 - then.0).max(0) / 1_000;
    match seconds {
        0 => "just now".to_string(),
        1 => "1 second ago".to_string(),
        s if s < 60 => format!("{s} seconds ago"),
        s if s < 120 => "1 minute ago".to_string(),
        s if s < 3_600 => format!("{} minutes ago", s / 60),
        s if s < 7_200 => "1 hour ago".to_string(),
        s => format!("{} hours ago", s / 3_600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises the two tests that write `SMARTME_CLIENT_*`.
    ///
    /// **They used to claim "single-threaded test setup" and race.** `cargo test`
    /// runs unit tests on a thread pool, and the two set `SMARTME_CLIENT_SECRET`
    /// to *different* values before asserting on what [`posted`] read back — so
    /// whichever wrote last decided both outcomes. Observed 2026-08-06 in
    /// `ci-local.sh`, having passed five consecutive local runs first:
    ///
    /// ```text
    /// ---- ui::screens::tests::a_posted_credential_is_ignored_because_the_form_has_no_such_field ----
    /// assertion `left == right` failed: a browser must not be able to set the credential
    ///   left: Some("secret")
    ///  right: Some("from-the-environment")
    /// ```
    ///
    /// `Some("secret")` is the *other* test's value, which is the whole diagnosis.
    ///
    /// **What this lock does not fix**, and it should not be mistaken for it: the
    /// `unsafe` on `set_var` is about concurrent *readers*, and other tests in
    /// this module call [`posted`], which reads these variables. The lock makes
    /// the assertions deterministic; it does not make the write sound. The real
    /// repair is for `posted` to take a [`Credential`] instead of reading the
    /// process environment — a production change, deliberately not made here.
    static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// **Story 3.4 AC2/AC3 — the discovery section speaks every outcome, and the
    /// assertions are on the rendered bytes** (story 3.2's lesson: a status code
    /// proves nothing about what an operator reads).
    #[test]
    fn the_discovery_section_speaks_every_outcome() {
        // No outcome yet: the affordance alone, wired to its own action.
        let idle = discovery_section(None);
        assert!(
            idle.contains("formaction=/config/discover"),
            "the button must reach the discovery route from inside the main \
             form, so unsaved edits ride along: {idle}"
        );

        // A listing: names shown, a NULL name falls back to the serial, the
        // pick button carries the PAIR, and a dropped element is SAID.
        let listed = discovery_section(Some(&Discovery::Listed {
            devices: vec![
                DeviceListing {
                    id: "aaa-1".into(),
                    name: Some("appart-est".into()),
                    serial: 9_202_685,
                },
                DeviceListing {
                    id: "bbb-2".into(),
                    name: None,
                    serial: 6_387_488,
                },
            ],
            dropped: vec!["missing field `Serial`".into()],
        }));
        assert!(listed.contains("appart-est"));
        assert!(
            listed.contains("(unnamed — serial 6387488)"),
            "a null name shows the serial; nothing invents a name: {listed}"
        );
        assert!(
            listed.contains("value=\"9202685|aaa-1\""),
            "the pick carries device id AND serial together — the pair ADR 0029 \
             verifies — so choosing is one gesture, not two transcriptions: {listed}"
        );
        assert!(
            listed.contains("could not be read from the account listing")
                && listed.contains("missing field `Serial`"),
            "a dropped element is a meter the operator cannot pick, and the \
             screen must say so with serde's reason: {listed}"
        );

        // The taxonomy states (AC3), each in words the operator can act on.
        let empty = discovery_section(Some(&Discovery::Empty));
        assert!(
            empty.contains("has no meters") && !empty.contains("class=fault"),
            "an empty account is a state, not a fault: {empty}"
        );
        let failed = discovery_section(Some(&Discovery::Failed {
            what: "the smart-me API rejected the credentials (HTTP 401)".into(),
        }));
        assert!(
            failed.contains("class=fault") && failed.contains("401"),
            "a failure renders in the taxonomy's own words: {failed}"
        );
        assert!(
            failed.contains("Typed entry below still works"),
            "discovery being down must not read as the form being down: {failed}"
        );
        let no_credential = discovery_section(Some(&Discovery::NoCredential));
        assert!(
            no_credential.contains("SMARTME_CLIENT_ID")
                && no_credential.contains("SMARTME_CLIENT_SECRET"),
            "the environment fault names its variables and is never a box to \
             type into (ADR 0023): {no_credential}"
        );

        // AND NO OUTCOME MAY SPEAK THE FORBIDDEN TOKENS — the first-run browser
        // test scans GET /config for these four as the mechanical form of
        // ADR 0019's rule, and GitHub caught this section's first draft using
        // one while every local suite was green. Extending the scan to every
        // outcome is what lets the next violation fail HERE, before a push.
        for outcome in [
            None,
            Some(Discovery::Empty),
            Some(Discovery::NoCredential),
            Some(Discovery::Failed { what: "x".into() }),
            Some(Discovery::Picked { serial: "1".into() }),
            Some(Discovery::AlreadyMapped { serial: "1".into() }),
            Some(Discovery::Listed {
                devices: vec![DeviceListing {
                    id: "a".into(),
                    name: None,
                    serial: 1,
                }],
                dropped: vec!["r".into()],
            }),
        ] {
            let rendered = discovery_section(outcome.as_ref());
            for forbidden in ["client_secret", "client_id", "credential", "password"] {
                assert!(
                    !rendered.contains(forbidden),
                    "{forbidden:?} must not appear on any discovery surface — \
                     say 'client id' with spaces and the uppercase environment \
                     names, as the rest of the screen does: {rendered}"
                );
            }
        }
    }

    /// **Story 3.4 AC2 + AC4 — a pick fills the pair, and discovery saves and
    /// publishes NOTHING**, in the unconfirmed state where it matters most.
    ///
    /// The handler is driven directly, with the headers a non-browser client
    /// sends (no Origin — the same-origin guard's pass-through case). The state
    /// dir is a scratch directory with NO stored configuration: an unconfigured,
    /// unconfirmed bridge, which is exactly when an operator needs discovery.
    #[tokio::test]
    async fn a_pick_fills_the_pair_and_neither_saves_nor_publishes() {
        let dir =
            std::env::temp_dir().join(format!("smartme_discover_pick_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let state = Arc::new(UiState::new(
            crate::ui::Phase::silent(crate::ui::Lifecycle::Unconfigured).into_handle(),
            dir.clone(),
            std::sync::Arc::new(crate::core::clock::FakeClock::new(
                crate::domain::UtcMillis(1_784_984_793_000),
            )),
            Arc::new(tokio::sync::Notify::new()),
        ));
        // The operator had typed half a form; those values must survive the trip.
        let fields: Fields = vec![
            ("group_id".into(), "Home".into()),
            ("meter.0.meter_id".into(), "garage".into()),
            ("meter.0.device_id".into(), "dev-0".into()),
            ("meter.0.serial".into(), "1112222".into()),
            ("pick".into(), "9202685|aaa-1".into()),
        ];
        let response = discover(State(Arc::clone(&state)), HeaderMap::new(), Form(fields)).await;

        let page = crate::ui::rendered_body(response).await;
        assert!(
            page.contains("value=\"aaa-1\"") && page.contains("value=\"9202685\""),
            "the picked PAIR must be in the form's boxes, together: {page}"
        );
        assert!(
            page.contains("value=\"garage\"") && page.contains("value=\"Home\""),
            "the unsaved edits rode along — the discover round trip must not \
             cost the operator what they had typed: {page}"
        );
        assert!(
            !page.contains("name=\"meter.1.enabled\" value=1 checked"),
            "Published stays a deliberate tick, never a side effect of picking: {page}"
        );
        assert!(
            !store::exists(&dir),
            "discovery writes no configuration — reading smart-me is not \
             publishing, and picking is not saving (the 5.3 boundary)"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **Review repair — Enter still means Save.** The discovery buttons sit
    /// before the visible Save in tree order, and HTML implicit submission
    /// activates the FIRST submit button — so without the hidden leading Save,
    /// pressing Enter in a text box discovered instead of saving, silently, on
    /// a page that re-renders almost identically.
    #[test]
    fn enter_still_means_save() {
        let rendered = form(
            &current_or_blank(std::path::Path::new("/nonexistent")),
            None,
            None,
        );
        let hidden_save = rendered
            .find("<button type=submit hidden")
            .expect("the hidden leading Save button exists");
        let first_discover = rendered
            .find("formaction=/config/discover")
            .expect("the discovery affordance exists");
        assert!(
            hidden_save < first_discover,
            "the first submit button in tree order is the Enter key's target, \
             and it must be Save — not the discovery fetch"
        );
    }

    /// **Review repair — the gravest one: discovery asks the SAVED base, never
    /// the submitted one.** `fetch_token` POSTs the client credential to
    /// `{base}/oauth/token`, and the first version took the base from the form
    /// — one un-Origined request could hand the secret to any https host,
    /// persisting nothing. The file is the configuration (ADR 0023): a base
    /// reaches the credential only by being validated and SAVED first.
    #[test]
    fn discovery_asks_the_saved_base_never_the_submitted_one() {
        let dir =
            std::env::temp_dir().join(format!("smartme_discovery_base_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");

        assert_eq!(
            discovery_base(&dir).expect("absence is not invalidity"),
            SmartMeClient::DEFAULT_BASE,
            "no file: the default base, not whatever a request carried"
        );

        let mut stored = store_fixture();
        stored.api_base = Some("https://mirror.example".into());
        store::save(&dir, &stored, crate::domain::UtcMillis(1_784_984_793_000)).expect("save");
        assert_eq!(
            discovery_base(&dir).expect("a sound file reads"),
            "https://mirror.example",
            "a SAVED base is the operator's committed, visible choice — that \
             one is honoured"
        );

        // FAIL-CLOSED on an unreadable file — the review of the repair caught
        // the `.ok()` version failing OPEN to the default base, sending the
        // sign-in to a host the operator's (unreadable) file does not name
        // while the screen claimed the SAVED base was asked.
        std::fs::write(store::config_path(&dir), "this is not toml {{{")
            .expect("plant the corrupt file");
        assert!(
            discovery_base(&dir).is_err(),
            "a file that cannot be read means nothing may be asked at all — \
             falling back to the default would ask a host the saved (and now \
             unreadable) configuration may well not name"
        );
        // And by signature: `discovery_base` and `fetch_listing` take the state
        // dir alone. The submitted `api_base` cannot reach the client without
        // this test's subject changing shape.
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **Story 6.7 AC1 — the mark makes the round trip, in both directions.**
    ///
    /// A field the form READS but does not RENDER is worse than one it ignores: the
    /// tick survives one Save and disappears at the next, and nothing tells the
    /// operator. Story 3.4 repaired exactly that defect for the other fields, and
    /// this asserts the new one cannot reintroduce it.
    ///
    /// FALSIFIED 2026-08-20 — mutation RUN: dropping `{starred}` from the rendered
    /// checkbox goes red with `a meter marked as mattering must come back TICKED`.
    #[test]
    fn a_meter_marked_as_mattering_survives_the_round_trip() {
        // Inbound: an unticked box posts nothing, a ticked one posts a value.
        let ticked: Fields = vec![
            ("meter.0.meter_id".into(), "appart-est".into()),
            ("meter.0.device_id".into(), "dev-0".into()),
            ("meter.0.serial".into(), "1112222".into()),
            ("meter.0.enabled".into(), "1".into()),
            ("meter.0.priority".into(), "1".into()),
        ];
        let raw = posted(&ticked, std::path::Path::new("/nonexistent"));
        let values = as_typed(&ticked, &raw);
        assert!(
            values.meters[0].priority,
            "a ticked box must reach the value the screen re-renders from"
        );

        // Outbound: it comes back ticked.
        let html = form(&values, None, None);
        assert!(
            html.contains("name=\"meter.0.priority\" value=1 checked"),
            "a meter marked as mattering must come back TICKED, or the next Save \
             silently clears a mark the operator made — the round-trip defect story \
             3.4 already repaired once, arriving through a new field:\n{html}"
        );

        // And an unmarked meter comes back unticked, or the assertion above would
        // pass against a checkbox that is always on.
        let plain: Fields = vec![
            ("meter.0.meter_id".into(), "appart-est".into()),
            ("meter.0.device_id".into(), "dev-0".into()),
            ("meter.0.serial".into(), "1112222".into()),
            ("meter.0.enabled".into(), "1".into()),
        ];
        let raw = posted(&plain, std::path::Path::new("/nonexistent"));
        let values = as_typed(&plain, &raw);
        assert!(!values.meters[0].priority);
        let html = form(&values, None, None);
        assert!(
            !html.contains("name=\"meter.0.priority\" value=1 checked"),
            "and silence is not a claim that a meter matters:\n{html}"
        );
    }

    /// **Review-of-the-repair repair — a half-matched pick CORRECTS, one lie
    /// removed and one use restored.** The first dedup refused any half-match
    /// with "already among the rows" — false when no row carries that serial —
    /// and blocked the pick's one repair use: fixing a row whose other half
    /// was mistyped. The account is the authority on which id goes with which
    /// serial.
    #[tokio::test]
    async fn a_half_matched_pick_corrects_the_row_and_says_so() {
        let state = Arc::new(UiState::new(
            crate::ui::Phase::silent(crate::ui::Lifecycle::Unconfigured).into_handle(),
            std::path::PathBuf::from("/nonexistent"),
            std::sync::Arc::new(crate::core::clock::FakeClock::new(
                crate::domain::UtcMillis(1_784_984_793_000),
            )),
            Arc::new(tokio::sync::Notify::new()),
        ));
        // The device id is right, the serial was mistyped by hand.
        let fields: Fields = vec![
            ("meter.0.meter_id".into(), "garage".into()),
            ("meter.0.device_id".into(), "aaa-1".into()),
            ("meter.0.serial".into(), "1112222".into()),
            ("pick".into(), "9202685|aaa-1".into()),
        ];
        let response = discover(State(Arc::clone(&state)), HeaderMap::new(), Form(fields)).await;
        let page = crate::ui::rendered_body(response).await;
        assert!(
            page.contains("value=\"9202685\"") && !page.contains("1112222"),
            "the row's pair is corrected to the account's — the mistyped serial \
             is gone from the boxes: {page}"
        );
        assert!(
            !page.contains("meter.2."),
            "corrected IN PLACE, not appended beside the wrong row: {page}"
        );
        assert!(
            page.contains("corrected to the account"),
            "and the screen says what happened to which row: {page}"
        );

        // THE AMBIGUOUS CASE: two rows each hold a half — correcting either
        // would merge two meters, so nothing moves and the refusal names the
        // rows.
        let fields: Fields = vec![
            ("meter.0.meter_id".into(), "garage".into()),
            ("meter.0.device_id".into(), "aaa-1".into()),
            ("meter.0.serial".into(), "1112222".into()),
            ("meter.1.meter_id".into(), "cellar".into()),
            ("meter.1.device_id".into(), "bbb-2".into()),
            ("meter.1.serial".into(), "9202685".into()),
            ("pick".into(), "9202685|aaa-1".into()),
        ];
        let response = discover(State(Arc::clone(&state)), HeaderMap::new(), Form(fields)).await;
        let page = crate::ui::rendered_body(response).await;
        assert!(
            page.contains("rows 1 and 2") && page.contains("nothing was changed"),
            "two half-matches must move nothing and say which rows collide: {page}"
        );
        assert!(
            page.contains("value=\"1112222\"") && page.contains("value=\"bbb-2\""),
            "both rows are untouched: {page}"
        );
    }

    /// **Review-of-the-repair repair — a pristine first run stays fault-free.**
    /// The unconditional fault rendering put a page of refusals on a blank
    /// first run for a save nobody attempted, while `GET /config` deliberately
    /// renders zero faults on the same state. The faults exist for the rewrite
    /// hazard, and the hazard exists exactly when a value was typed.
    #[tokio::test]
    async fn a_blank_first_run_discover_renders_no_faults() {
        let state = Arc::new(UiState::new(
            crate::ui::Phase::silent(crate::ui::Lifecycle::Unconfigured).into_handle(),
            std::path::PathBuf::from("/nonexistent"),
            std::sync::Arc::new(crate::core::clock::FakeClock::new(
                crate::domain::UtcMillis(1_784_984_793_000),
            )),
            Arc::new(tokio::sync::Notify::new()),
        ));
        let fields: Fields = vec![("pick".into(), "9202685|aaa-1".into())];
        let response = discover(State(Arc::clone(&state)), HeaderMap::new(), Form(fields)).await;
        let page = crate::ui::rendered_body(response).await;
        assert!(
            !page.contains("is missing or empty"),
            "nothing was typed, so nothing may be refused — a first run is not \
             a fault (the GET /config rule, honoured on this path too): {page}"
        );
    }

    /// **Review repair — a mistyped number survives the discover round trip in
    /// its fault.** `as_typed` is lossy by design (the box comes back blank or
    /// defaulted); the save path pairs that with the fault beside the box, and
    /// the discover path rendered `errors: None` — so a typo vanished without a
    /// word, the exact `publish_period_secs = 0` incident `as_typed`'s doc
    /// memorialises, reopened through the new route.
    #[tokio::test]
    async fn a_mistyped_number_survives_the_discover_round_trip_in_its_fault() {
        let dir =
            std::env::temp_dir().join(format!("smartme_discover_fault_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let state = Arc::new(UiState::new(
            crate::ui::Phase::silent(crate::ui::Lifecycle::Unconfigured).into_handle(),
            dir.clone(),
            std::sync::Arc::new(crate::core::clock::FakeClock::new(
                crate::domain::UtcMillis(1_784_984_793_000),
            )),
            Arc::new(tokio::sync::Notify::new()),
        ));
        let fields: Fields = vec![
            ("ui_port".into(), "8O80".into()),
            ("pick".into(), "9202685|aaa-1".into()),
        ];
        let response = discover(State(Arc::clone(&state)), HeaderMap::new(), Form(fields)).await;
        let page = crate::ui::rendered_body(response).await;
        assert!(
            page.contains("8O80"),
            "the mistyped value must survive ON the page — the box is blanked \
             by `as_typed`, so the fault that QUOTES it is the only witness: {page}"
        );
        assert!(
            page.contains("class=fault"),
            "and it renders as a fault, beside its box, exactly as the save \
             path would have said it: {page}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **Review repair — a second pick of the same meter adds nothing.** The
    /// listing disappears on the way back, so "did that register?" ends in
    /// Back-and-click-again — and a duplicated row is refused at save by a
    /// rule the screen never names.
    #[tokio::test]
    async fn a_second_pick_of_the_same_meter_adds_nothing() {
        let state = Arc::new(UiState::new(
            crate::ui::Phase::silent(crate::ui::Lifecycle::Unconfigured).into_handle(),
            std::path::PathBuf::from("/nonexistent"),
            std::sync::Arc::new(crate::core::clock::FakeClock::new(
                crate::domain::UtcMillis(1_784_984_793_000),
            )),
            Arc::new(tokio::sync::Notify::new()),
        ));
        // The pair is already row 0 — the operator picked it a moment ago.
        let fields: Fields = vec![
            ("meter.0.meter_id".into(), "garage".into()),
            ("meter.0.device_id".into(), "aaa-1".into()),
            ("meter.0.serial".into(), "9202685".into()),
            ("pick".into(), "9202685|aaa-1".into()),
        ];
        let response = discover(State(Arc::clone(&state)), HeaderMap::new(), Form(fields)).await;
        let page = crate::ui::rendered_body(response).await;
        assert!(
            !page.contains("meter.2."),
            "one existing row plus the blank one: a second pick must not have \
             appended a third row for save to refuse: {page}"
        );
        assert!(
            page.contains("already among"),
            "and the screen says WHY nothing was added, rather than looking \
             like a click that did not register: {page}"
        );
    }

    /// A minimal stored configuration for the tests above.
    fn store_fixture() -> StoredConfig {
        StoredConfig {
            created_ms: None,
            last_change_ms: None,
            schema_version: store::SCHEMA_VERSION,
            group_id: "Group".into(),
            node_id: "Node".into(),
            broker_host: "broker".into(),
            broker_port: 1883,
            publish_period_secs: 30,
            api_base: None,
            log_dir: None,
            log_keep: None,
            mapping_confirmed: false,
            ui_port: None,
            meters: vec![StoredMeter {
                priority: false,
                meter_id: "meter-a".into(),
                device_id: "dev-a".into(),
                serial: "9202685".into(),
                enabled: true,
            }],
        }
    }

    #[test]
    fn markup_in_a_meter_name_cannot_escape_the_attribute_it_is_rendered_into() {
        let hostile = "\"><script>alert(1)</script>";
        let rendered = escape(hostile);
        assert!(
            !rendered.contains('<') && !rendered.contains('"'),
            "every value on these screens was typed by whoever can reach the UI; \
             rendering one back unescaped turns the configuration form into a \
             delivery mechanism. Got {rendered}"
        );
    }

    /// An unticked checkbox posts nothing, so the rows must be bound by index
    /// and never by position.
    #[test]
    fn a_disabled_meter_does_not_shift_the_meters_after_it() {
        let fields: Fields = vec![
            ("meter.0.meter_id".into(), "garage".into()),
            ("meter.0.device_id".into(), "dev-0".into()),
            ("meter.0.serial".into(), "111".into()),
            // meter.0.enabled deliberately absent — the checkbox was not ticked.
            ("meter.1.meter_id".into(), "cellar".into()),
            ("meter.1.device_id".into(), "dev-1".into()),
            ("meter.1.serial".into(), "222".into()),
            ("meter.1.enabled".into(), "1".into()),
        ];
        let parsed = meters(&fields);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].serial.as_deref(), Some("111"));
        assert_eq!(parsed[0].enabled, Some(false));
        assert_eq!(
            parsed[1].serial.as_deref(),
            Some("222"),
            "the second meter's serial must stay on the second meter even though \
             the first posted no checkbox"
        );
        assert_eq!(parsed[1].enabled, Some(true));
    }

    /// The form has no credential field, so nothing the browser posts can reach
    /// the credential — it comes from the environment or not at all.
    #[test]
    fn a_posted_credential_is_ignored_because_the_form_has_no_such_field() {
        let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: see `ENV`. Both variables are set, so this test does not depend
        // on what the other one left behind.
        unsafe {
            std::env::set_var("SMARTME_CLIENT_ID", "from-the-environment-id");
            std::env::set_var("SMARTME_CLIENT_SECRET", "from-the-environment");
        }
        let fields: Fields = vec![
            ("client_secret".into(), "posted-by-a-browser".into()),
            ("group_id".into(), "Site".into()),
        ];
        let raw = posted(&fields, std::path::Path::new("/data"));
        assert_eq!(
            raw.client_secret.as_deref(),
            Some("from-the-environment"),
            "a browser must not be able to set the credential: ADR 0023 keeps it \
             in the environment and off disk"
        );
    }

    /// A blank trailing row must contribute nothing, or every save would add an
    /// empty meter and `validate` would refuse a form the operator filled in
    /// correctly.
    ///
    /// **This test asserted the exact opposite until 2026-08-05** — `assert_eq!(
    /// parsed.len(), 2, "the empty row is still a row here")` — and never called
    /// `validate`, which is the only thing that could have shown the harm. It
    /// was named for the property and codified its absence, and the
    /// configuration screen was therefore saveable exactly once. It now ends at
    /// `validate`, because "the row is dropped" is not the claim; "the form the
    /// bridge served can be sent back to it" is.
    #[test]
    fn the_blank_add_a_meter_row_contributes_nothing() {
        let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: see `ENV`. `validate` joins the credential from the
        // environment, so the row-dropping claim would otherwise be masked by a
        // missing-credential fault.
        unsafe {
            std::env::set_var("SMARTME_CLIENT_ID", "id");
            std::env::set_var("SMARTME_CLIENT_SECRET", "secret");
        }
        let fields: Fields = vec![
            ("group_id".into(), "Plant".into()),
            ("node_id".into(), "Bridge01".into()),
            ("broker_host".into(), "broker".into()),
            ("broker_port".into(), "1883".into()),
            ("publish_period_secs".into(), "30".into()),
            ("meter.0.meter_id".into(), "garage".into()),
            ("meter.0.device_id".into(), "dev".into()),
            ("meter.0.serial".into(), "111".into()),
            ("meter.0.enabled".into(), "1".into()),
            ("meter.1.meter_id".into(), "".into()),
            ("meter.1.device_id".into(), "".into()),
            ("meter.1.serial".into(), "".into()),
        ];
        let parsed = meters(&fields);
        assert_eq!(
            parsed.len(),
            1,
            "the untouched row must not become a meter: {parsed:?}"
        );
        assert_eq!(parsed[0].serial.as_deref(), Some("111"));

        // And the claim that actually matters — `validate` accepts it. The
        // previous version of this test stopped one call short of the only
        // function that could refuse.
        let raw = posted(&fields, std::path::Path::new("/data"));
        assert!(
            config::validate(raw).is_ok(),
            "the form the bridge itself rendered must be acceptable when sent \
             back unchanged, or the screen can be saved exactly once"
        );
    }

    /// A row with nothing typed but its box ticked is NOT dropped.
    ///
    /// Dropping on emptiness alone would turn a fault the operator can act on
    /// into a value that silently vanished — they said "publish this" and would
    /// be shown a page with no meter and no complaint.
    #[test]
    fn a_ticked_but_empty_row_is_kept_so_its_faults_are_reported() {
        let fields: Fields = vec![
            ("meter.0.meter_id".into(), "".into()),
            ("meter.0.device_id".into(), "".into()),
            ("meter.0.serial".into(), "".into()),
            ("meter.0.enabled".into(), "1".into()),
        ];
        assert_eq!(meters(&fields).len(), 1);
        let raw = posted(&fields, std::path::Path::new("/data"));
        assert!(
            config::validate(raw).is_err(),
            "a row somebody ticked must produce faults, never silence"
        );
    }

    /// **The defect neither module could see alone.** `store::save` decides
    /// whether a confirmation survives a write; `mapping_fingerprint` decides
    /// whether a click still refers to what the screen displayed. They are two
    /// encodings of one question — *"is this the same mapping?"* — and for a month
    /// they disagreed: the fingerprint covered `group_id` and `node_id`, the
    /// withdrawal rule did not. Confirm, correct the node id, restart, and the
    /// bridge published into a namespace nobody had ever seen.
    ///
    /// Neither module's own tests could catch that, because each was internally
    /// consistent. This is the only test that puts them side by side, so it must
    /// stay exhaustive over the fields either one reads.
    ///
    /// **NARROWED IN WHAT IT CAN CATCH since story 3.4, and honestly so** (the
    /// review of that story read this doc against the new code): both sides now
    /// derive from ONE `store::mapping_projection` call, so the two-readers
    /// divergence this test was written for is impossible by construction and
    /// the 2026-08-06 falsification below can no longer be reproduced. What it
    /// still guards is the remaining seam: `canonical()` dropping or merging a
    /// field that the projection's `PartialEq` keeps (the fingerprint reads the
    /// bytes, the withdrawal rule reads the value). The exhaustive-destructure
    /// build-stop and `store::…::the_projection_decides_membership_for_every_field`
    /// are where the membership question now lives.
    ///
    /// FALSIFIED 2026-08-06 by removing the identity comparison from
    /// `store::same_mapping` — the exact state of the code before today. Copied:
    ///
    /// ```text
    /// test ui::screens::tests::the_withdrawal_rule_and_the_fingerprint_answer_the_same_question ... FAILED
    ///
    /// thread '…the_withdrawal_rule_and_the_fingerprint_answer_the_same_question' (353) panicked at
    /// crates/smartme-bridge/src/ui/screens.rs:1097:13:
    /// assertion `left == right` failed: "the node id" moves the fingerprint but not the
    /// withdrawal rule, or the reverse: the operator's click and the stored confirmation
    /// would disagree about whether this is the same mapping.
    /// fingerprint moved: true, same_mapping: true
    ///   left: true
    ///  right: false
    ///
    /// test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 165 filtered out
    /// ```
    #[test]
    fn the_withdrawal_rule_and_the_fingerprint_answer_the_same_question() {
        let base = StoredConfig {
            created_ms: None,
            last_change_ms: None,
            schema_version: store::SCHEMA_VERSION,
            group_id: "Site".into(),
            node_id: "Bridge".into(),
            broker_host: "b".into(),
            broker_port: 1883,
            publish_period_secs: 30,
            api_base: None,
            log_dir: None,
            log_keep: None,
            mapping_confirmed: false,
            ui_port: None,
            meters: vec![
                StoredMeter {
                    priority: false,
                    meter_id: "garage".into(),
                    device_id: "dev-a".into(),
                    serial: "111".into(),
                    enabled: true,
                },
                StoredMeter {
                    priority: false,
                    meter_id: "cellar".into(),
                    device_id: "dev-b".into(),
                    serial: "222".into(),
                    enabled: false,
                },
            ],
        };

        /// One named edit to a stored configuration.
        type Case = (&'static str, fn(&mut StoredConfig));

        // Every field either rule reads, plus two that neither may react to.
        let cases: Vec<Case> = vec![
            ("the node id", |c| c.node_id = "Other".into()),
            ("the group id", |c| c.group_id = "Other".into()),
            ("a meter id", |c| c.meters[0].meter_id = "attic".into()),
            ("a device id", |c| c.meters[0].device_id = "dev-z".into()),
            ("a serial", |c| c.meters[0].serial = "999".into()),
            ("an enabled flag", |c| c.meters[1].enabled = true),
            ("a meter added", |c| c.meters.push(c.meters[0].clone())),
            ("a meter removed", |c| {
                c.meters.pop();
            }),
            ("the row order", |c| c.meters.swap(0, 1)),
            ("the broker host", |c| c.broker_host = "elsewhere".into()),
            ("the publish period", |c| c.publish_period_secs = 45),
        ];

        for (what, mutate) in cases {
            let mut edited = base.clone();
            mutate(&mut edited);
            let fingerprint_moved = mapping_fingerprint(&edited) != mapping_fingerprint(&base);
            let still_same = store::same_mapping(&base, &edited);
            assert_eq!(
                fingerprint_moved, !still_same,
                "{what:?} moves the fingerprint but not the withdrawal rule, or the \
                 reverse: the operator's click and the stored confirmation would \
                 disagree about whether this is the same mapping. \
                 fingerprint moved: {fingerprint_moved}, same_mapping: {still_same}"
            );
        }
    }

    /// The fingerprint exists to bind a click to a mapping. If it did not move
    /// when the mapping moved, `store::confirm` would go on blessing whatever is
    /// on disk and the guard would be decorative.
    #[test]
    fn the_fingerprint_moves_when_the_mapping_does_and_not_when_the_order_does() {
        let mut a = StoredConfig {
            created_ms: None,
            last_change_ms: None,
            schema_version: store::SCHEMA_VERSION,
            group_id: "Site".into(),
            node_id: "Bridge".into(),
            broker_host: "b".into(),
            broker_port: 1883,
            publish_period_secs: 30,
            api_base: None,
            log_dir: None,
            log_keep: None,
            mapping_confirmed: false,
            ui_port: None,
            meters: vec![
                StoredMeter {
                    priority: false,
                    meter_id: "garage".into(),
                    device_id: "dev-a".into(),
                    serial: "111".into(),
                    enabled: true,
                },
                StoredMeter {
                    priority: false,
                    meter_id: "cellar".into(),
                    device_id: "dev-b".into(),
                    serial: "222".into(),
                    enabled: false,
                },
            ],
        };
        let original = mapping_fingerprint(&a);

        a.meters.swap(0, 1);
        assert_eq!(
            mapping_fingerprint(&a),
            original,
            "reordering rows changes nothing on the wire, and making the operator \
             confirm again for it is how a guard becomes noise"
        );

        a.meters[0].device_id = "dev-c".into();
        assert_ne!(
            mapping_fingerprint(&a),
            original,
            "a device id is the half that is easy to cross-wire — if the click \
             survived a change to it, the operator would be vouching for a \
             mapping they never saw"
        );

        a.meters[0].device_id = "dev-b".into();
        a.node_id = "Other".into();
        assert_ne!(
            mapping_fingerprint(&a),
            original,
            "the node id is part of every topic shown, so it is part of the mapping"
        );
    }
}
