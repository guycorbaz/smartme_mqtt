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
        log_keep: field(fields, "log_keep").and_then(|v| v.parse().ok()),
        ui_port: field(fields, "ui_port").and_then(|v| v.parse().ok()),
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
        log_keep: raw.log_keep,
        mapping_confirmed: false,
        ui_port: raw.ui_port,
        meters: meters(fields)
            .into_iter()
            .map(|m| StoredMeter {
                meter_id: m.meter_id.unwrap_or_default(),
                device_id: m.device_id.unwrap_or_default(),
                serial: m.serial.unwrap_or_default(),
                enabled: m.enabled.unwrap_or(false),
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

fn page(title: &str, body: &str) -> Html<String> {
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

/// Render the whole form. `values` is what to show in the boxes — the
/// submission when there was one, the file otherwise, so a refused save never
/// makes the operator retype what they had entered.
fn form(values: &StoredConfig, errors: Option<&ConfigErrors>) -> String {
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
                 Published</label></fieldset>",
                n = i + 1,
                name = escape(&m.meter_id),
                device = escape(&m.device_id),
                serial = escape(&m.serial),
                checked = if m.enabled { "checked" } else { "" },
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

    format!(
        "{orphans}\
         <form method=post action=/config>\
         {group}{node}{host}{port}{period}\
         <fieldset><legend>Meters</legend>{meter_rows}\
         <fieldset><legend>Add a meter</legend>\
         <label for=nb>Name</label><input type=text id=nb name=\"meter.{blank}.meter_id\" value=\"\">\
         <label for=db>smart-me device id</label><input type=text id=db name=\"meter.{blank}.device_id\" value=\"\">\
         <label for=sb>Serial</label><input type=text id=sb name=\"meter.{blank}.serial\" value=\"\">\
         <label><input type=checkbox name=\"meter.{blank}.enabled\" value=1> Published</label>\
         </fieldset></fieldset>\
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
    )
}

/// What the file holds, or a blank slate on a first run.
fn current_or_blank(state_dir: &std::path::Path) -> StoredConfig {
    store::read(state_dir).unwrap_or_else(|_| StoredConfig {
        schema_version: store::SCHEMA_VERSION,
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
        &form(&values, errors.as_ref()),
    )
    .into_response()
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
                    &form(&as_typed(&fields, &raw), Some(&errors)),
                ),
            )
                .into_response();
        }
    };

    // Built from the VALIDATED struct, never from the raw strings — see
    // `as_typed`'s docs for the container-bricking defect that came of the
    // latter.
    if let Err(error) = store::save(state.state_dir(), &StoredConfig::from(&validated)) {
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
    let mut rows: Vec<String> = stored
        .meters
        .iter()
        .map(|m| {
            format!(
                "{}\u{1f}{}\u{1f}{}\u{1f}{}",
                m.meter_id, m.device_id, m.serial, m.enabled
            )
        })
        .collect();
    // Order is not part of the mapping — reordering rows changes nothing about
    // what reaches the wire, and `store::save` takes the same view.
    rows.sort();
    let joined = format!(
        "{}\u{1e}{}\u{1e}{}",
        stored.group_id,
        stored.node_id,
        rows.join("\u{1e}")
    );
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

#[cfg(test)]
mod tests {
    use super::*;

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
        // SAFETY: single-threaded test setup; the variable is read below.
        unsafe {
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
        // SAFETY: single-threaded test setup. `validate` joins the credential
        // from the environment, so the row-dropping claim would otherwise be
        // masked by a missing-credential fault.
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

    /// The fingerprint exists to bind a click to a mapping. If it did not move
    /// when the mapping moved, `store::confirm` would go on blessing whatever is
    /// on disk and the guard would be decorative.
    #[test]
    fn the_fingerprint_moves_when_the_mapping_does_and_not_when_the_order_does() {
        let mut a = StoredConfig {
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
                    meter_id: "garage".into(),
                    device_id: "dev-a".into(),
                    serial: "111".into(),
                    enabled: true,
                },
                StoredMeter {
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
