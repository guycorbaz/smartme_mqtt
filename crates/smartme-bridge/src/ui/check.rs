//! The end-to-end check (FR37, story 6.6) — three links, one screen, and nothing
//! published to light them.
//!
//! # What this module refuses to do, and why it is the whole design
//!
//! **It does not judge the reading it fetches.** `Policy::step_remembering` judges
//! against a per-meter memory — `energy_reference`, `last_http_date`,
//! `last_value_date` — that three oracles read. A check that moved that memory would
//! make the NEXT real tick report a counter going backwards, and one that judged
//! without it would be a second assembly of judgements: the defect AR19's *"UI
//! consumes this state, never recomputes it"* exists to prevent.
//!
//! So the three links report three facts, each from whoever owns it:
//!
//! | Link | Fact | Owner |
//! |---|---|---|
//! | source | a real `GET /Devices/{id}`, now, with how long it took | this module's own call |
//! | value | the verdict, cause and culprit **in force** | the poll loop, via `FleetState` |
//! | sink | connected / never / unreachable-since, and this meter's drops | the driver, via `SinkHealth` |
//!
//! **And nothing is published to light the third link.** A DDATA carrying a test
//! value is, in the historian, indistinguishable from a measurement — the button
//! would be manufacturing the exact lie this bridge exists to refuse — and a topic
//! outside the Sparkplug grammar is refused by `EdgeNode::device_topic` anyway.

use super::UiState;
use crate::core::source::SourceError;
use crate::domain::{MeterId, UtcMillis};
use axum::extract::{Form, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect, Response};
use smart_me_client::{Credentials, SmartMeClient};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The same bound discovery uses (story 6.6 AC6). A check that outlived it would
/// leave the page saying "running" about a request nobody is waiting for.
const CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// What the source answered, and nothing more than that.
///
/// **No variant carries a judgement.** `Answered` holds the numbers exactly as the
/// account sent them — unconverted, unjudged, unpublished — and the screen says so
/// in those words. The failure variants carry the taxonomy's own `Cause`, which is
/// a fact about the FETCH: `SourceError::cause` is the table the poll loop reads.
#[derive(Debug, Clone)]
pub(super) enum SourceLink {
    /// The account answered, in `took_ms` milliseconds.
    Answered {
        took_ms: i64,
        /// The device serial the ACCOUNT reports, beside which the screen shows
        /// the configured one. Two facts side by side — the identity oracle's
        /// verdict is the middle link's business, not this one's.
        serial: i64,
        value_date: String,
        power: f64,
        power_unit: String,
        energy: f64,
        energy_unit: String,
    },
    /// The account could not be asked, or refused. `what` is [`SmartMeError`]'s own
    /// wording — story 2.6 AC5 wrote a repair into each variant's `Display`, and
    /// this module adds no second opinion.
    Refused {
        cause: crate::core::oracle::Cause,
        what: String,
    },
    /// No credential in the environment. Named by its variables, never as a box to
    /// type into ([ADR 0023]: there is no credential field to render).
    ///
    /// [ADR 0023]: ../../../docs/adr/0023-the-file-is-the-configuration-the-credential-stays-in-the-environment.md
    NoCredential,
}

/// One meter's check, in the three states FR32 keeps apart.
#[derive(Debug, Clone)]
pub(super) enum Check {
    /// Started, no answer yet. The page renders this and refreshes itself; it does
    /// not hang waiting (story 6.6 AC6).
    Running { started: UtcMillis },
    /// Finished, with what the source said.
    Done { at: UtcMillis, source: SourceLink },
}

/// Every meter's last check. The UI writes it; nothing else reads it.
///
/// **Deliberately NOT in the fleet state.** `FleetState` is what the poll loop
/// wrote and what `/healthz` reports; a check result living there would be a fact
/// about a button inside the record of what was published (story 6.6 AC1).
#[derive(Clone, Default)]
pub(super) struct Checks(Arc<Mutex<HashMap<MeterId, Check>>>);

impl Checks {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn get(&self, meter: &MeterId) -> Option<Check> {
        self.0.lock().ok()?.get(meter).cloned()
    }

    pub(super) fn set(&self, meter: &MeterId, check: Check) {
        if let Ok(mut map) = self.0.lock() {
            map.insert(meter.clone(), check);
        }
    }
}

/// Why a check was not started. Rendered in words, never swallowed (AC5).
pub(super) enum Refusal {
    NotConfigured,
    AlreadyRunning,
    TooSoon { wait_secs: i64 },
}

/// The rate rule, as a pure function so a test can walk it.
///
/// **A check calls smart-me on a click**, and [#77] found that a 429 on the token
/// endpoint arms no wait — so the bridge's own restraint is the only restraint
/// there is. One in flight per meter, and no sooner than the poll period, which is
/// the cadence the account already sees from this bridge.
pub(super) fn refusal_for(
    check: Option<&Check>,
    now: UtcMillis,
    period_ms: i64,
) -> Option<Refusal> {
    match check {
        Some(Check::Running { .. }) => Some(Refusal::AlreadyRunning),
        Some(Check::Done { at, .. }) => {
            let elapsed = now.0 - at.0;
            (elapsed < period_ms).then(|| Refusal::TooSoon {
                wait_secs: (period_ms - elapsed).div_euclid(1_000) + 1,
            })
        }
        None => None,
    }
}

/// `GET /check` — the form, and whatever the last check for the chosen meter said.
///
/// **The query string is read from the `Uri` rather than through `Query`**, which
/// axum gates behind a feature this workspace does not enable. One parameter, one
/// decoder, no dependency added for it.
pub(super) async fn check_view(
    State(state): State<Arc<UiState>>,
    uri: axum::http::Uri,
) -> Response {
    let chosen = uri.query().and_then(|q| {
        q.split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(k, _)| *k == "meter")
            .map(|(_, v)| urldecode(v))
            .filter(|v| !v.is_empty())
    });
    render(&state, chosen.as_deref(), None)
}

/// The inverse of [`urlencode`], for the one parameter this page carries.
///
/// A byte it cannot decode is dropped rather than guessed at: the result is looked
/// up against the configuration's meters, so a mangled name finds nothing and the
/// page says exactly that.
fn urldecode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                match u8::from_str_radix(&value[i + 1..i + 3], 16) {
                    Ok(byte) => out.push(byte),
                    Err(_) => out.push(b'%'),
                }
                i += 3;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `POST /check` — start one, then redirect to the page that reports it.
///
/// **Redirect rather than render**, so the result has a URL an operator can
/// refresh and re-read, and so a reload does not re-ask smart-me.
pub(super) async fn run_check(
    State(state): State<Arc<UiState>>,
    headers: HeaderMap,
    Form(fields): Form<Vec<(String, String)>>,
) -> Response {
    // Mutating nothing, and guarded anyway ([ADR 0024], story 6.2 AC5): the guard
    // is about who may make this bridge call smart-me.
    if let Some(refusal) = super::origin::refusal(&headers) {
        return refusal;
    }
    let Some(name) = fields
        .iter()
        .find(|(k, _)| k == "meter")
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
    else {
        return render(&state, None, Some(Refusal::NotConfigured));
    };
    let meter = MeterId::new(name.clone());

    let phase = state.phase();
    let Some(control) = phase.control() else {
        return render(&state, Some(&name), Some(Refusal::NotConfigured));
    };
    let config = control.current();
    let Some(configured) = config.meters.iter().find(|m| m.meter == meter) else {
        return render(&state, Some(&name), Some(Refusal::NotConfigured));
    };

    let now = control.clock().wall();
    let period_ms = config.poll.interval.as_millis() as i64;
    if let Some(refusal) = refusal_for(state.checks().get(&meter).as_ref(), now, period_ms) {
        return render(&state, Some(&name), Some(refusal));
    }

    state.checks().set(&meter, Check::Running { started: now });

    let checks = state.checks().clone();
    let clock = control.clock();
    let device_id = configured.device_id.clone();
    let base = config.api_base.clone();
    let started = clock.monotonic();
    tokio::spawn(async move {
        let source = ask(&base, &device_id).await;
        let took_ms = clock.monotonic().0 - started.0;
        let source = match source {
            SourceLink::Answered {
                serial,
                value_date,
                power,
                power_unit,
                energy,
                energy_unit,
                ..
            } => SourceLink::Answered {
                took_ms,
                serial,
                value_date,
                power,
                power_unit,
                energy,
                energy_unit,
            },
            other => other,
        };
        checks.set(
            &meter,
            Check::Done {
                at: clock.wall(),
                source,
            },
        );
    });

    Redirect::to(&format!("/check?meter={}", urlencode(&name))).into_response()
}

/// The one network call this module makes.
///
/// **The base is the RUNNING configuration's**, not the saved file's — which is the
/// one difference from `fetch_listing`, and it is deliberate: discovery runs before
/// a configuration is in force and must read the file, while a check only exists
/// once there is a poll loop, and the base it must exercise is the one the loop is
/// actually using. There is therefore no unreadable-file case here.
///
/// Same shape as `screens::fetch_listing` otherwise — base from the configuration,
/// credential from the environment, its own timeout, `SmartMeError`'s wording on
/// failure — with `get_device` in place of `get_devices`, because the operator
/// pointed at one meter.
async fn ask(base: &str, device_id: &str) -> SourceLink {
    let id = std::env::var("SMARTME_CLIENT_ID")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let secret = std::env::var("SMARTME_CLIENT_SECRET")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let (Some(client_id), Some(client_secret)) = (id, secret) else {
        return SourceLink::NoCredential;
    };
    let client = match SmartMeClient::new(
        base,
        Credentials::ClientCredentials {
            client_id,
            client_secret,
        },
        CHECK_TIMEOUT,
    ) {
        Ok(client) => client,
        Err(error) => return refused(&error),
    };
    let token = match client.fetch_token().await {
        Ok(token) => token,
        Err(error) => return refused(&error),
    };
    match client.get_device(device_id, Some(&token)).await {
        Ok(capture) => SourceLink::Answered {
            took_ms: 0,
            serial: capture.device.serial,
            value_date: capture.device.value_date,
            power: capture.device.active_power,
            power_unit: capture.device.active_power_unit,
            energy: capture.device.counter_reading,
            energy_unit: capture.device.counter_reading_unit,
        },
        Err(error) => refused(&error),
    }
}

/// The client's error, classified by the table the poll loop reads.
///
/// `map_error` is the adapter's and private to it; what is shared is the mapping
/// from a `SourceError` to a `Cause` (`SourceError::cause`, extracted for this
/// story). Here the two client faults this call can meet are named the same way the
/// adapter names them, and everything else lands on `SourceUnreachable` — the honest
/// answer for "the account did not give us the reading", which is all a check knows.
fn refused(error: &smart_me_client::SmartMeError) -> SourceLink {
    SourceLink::Refused {
        cause: cause_of(error),
        what: error.to_string(),
    }
}

/// The cause the poll loop would publish for this client error.
///
/// Goes through [`SourceError`] rather than mapping the client's variants straight
/// to a `Cause`: that intermediate is where the taxonomy lives (story 2.6), and
/// `SourceError::cause` is the table `Policy::step_remembering` reads. Two callers,
/// one table.
pub(super) fn cause_of(error: &smart_me_client::SmartMeError) -> crate::core::oracle::Cause {
    let as_source_error = if error.is_fatal() {
        SourceError::Fatal {
            refusal: match error {
                smart_me_client::SmartMeError::AuthRejected { .. } => {
                    crate::core::source::Refusal::Credential
                }
                smart_me_client::SmartMeError::UnknownDevice { .. } => {
                    crate::core::source::Refusal::DeviceNotInAccount
                }
                _ => crate::core::source::Refusal::Configuration,
            },
            reason: error.to_string(),
        }
    } else if matches!(error, smart_me_client::SmartMeError::RateLimited { .. }) {
        SourceError::RateLimited { retry_after: None }
    } else {
        SourceError::Transient {
            reason: error.to_string(),
        }
    };
    as_source_error.cause()
}

fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// The page, in every state it can be in.
///
/// **The three links are rendered from three owners' facts**, and the words say
/// which is which. A reader who takes the middle link for a judgement of the top
/// one would be reading the very confusion this story was shaped to avoid, so the
/// page states the relation rather than implying it.
fn render(state: &Arc<UiState>, chosen: Option<&str>, refusal: Option<Refusal>) -> Response {
    use super::screens::{ago, escape, page, repair, sink_health_line};

    let phase = state.phase();
    let Some(control) = phase.control() else {
        return page(
            "End-to-end check",
            "<h1>End-to-end check</h1><p>The bridge is not publishing, so there is \
             nothing to check end to end. The configuration comes first.</p>\
             <p><a href=/config>Configure it</a></p>",
        )
        .into_response();
    };
    let config = control.current();
    let now = control.clock().wall();
    let fleet = phase.fleet();

    let mut chooser = String::from(
        "<form method=post action=/check><label for=meter>Meter</label>\
         <select id=meter name=meter>",
    );
    for meter in &config.meters {
        let selected = if Some(meter.meter.as_str()) == chosen {
            " selected"
        } else {
            ""
        };
        chooser.push_str(&format!(
            "<option value=\"{0}\"{selected}>{0}</option>",
            escape(meter.meter.as_str())
        ));
    }
    chooser.push_str("</select><button type=submit>Check it now</button></form>");

    let refusal_line = match refusal {
        None => String::new(),
        Some(Refusal::NotConfigured) => "<p class=fault>That meter is not in the \
             configuration in force, so there is nothing to ask about it.</p>"
            .to_string(),
        Some(Refusal::AlreadyRunning) => "<p class=fault>A check for that meter is \
             already running. This page refreshes itself; nothing was asked twice.</p>"
            .to_string(),
        Some(Refusal::TooSoon { wait_secs }) => format!(
            "<p class=fault>That meter was checked less than one poll period ago, \
             and the result below is that check. A button that asked smart-me on \
             every click would rate-limit the bridge itself. Try again in \
             {wait_secs} s.</p>"
        ),
    };

    let Some(name) = chosen else {
        return page(
            "End-to-end check",
            &format!(
                "<h1>End-to-end check</h1>\
                 <p>Pick a meter and the bridge will ask smart-me for it now, then \
                 show you the three links of the chain beside what it is currently \
                 publishing. <strong>Nothing is published by this check</strong> — a \
                 test value on the wire would be indistinguishable from a \
                 measurement.</p>{chooser}{refusal_line}\
                 <p><a href=/>State of the bridge</a> · <a href=/meters>Meters</a></p>"
            ),
        )
        .into_response();
    };
    let meter = MeterId::new(name);
    let configured = config.meters.iter().find(|m| m.meter == meter);

    // --- link 1: what the source just said, unjudged ---
    let (source_line, refresh) = match state.checks().get(&meter) {
        None => (
            "<p><strong>1. The source.</strong> Not checked since this bridge \
             started. The button above asks."
                .to_string(),
            "",
        ),
        Some(Check::Running { started }) => (
            format!(
                "<p><strong>1. The source.</strong> Asking smart-me — started {}. \
                 This page refreshes itself.",
                ago(now, started)
            ),
            "<meta http-equiv=refresh content=2>",
        ),
        Some(Check::Done { at, source }) => {
            let body = match source {
                SourceLink::Answered {
                    took_ms,
                    serial,
                    value_date,
                    power,
                    power_unit,
                    energy,
                    energy_unit,
                } => {
                    let declared = configured.map_or_else(
                        || "no row in the configuration".to_string(),
                        |m| m.serial.to_string(),
                    );
                    format!(
                        "<strong>answered</strong> in {took_ms} ms, {}. It reported \
                         serial {serial} (the configuration declares {}), measured at \
                         {}, {power} {power_unit} and {energy} {energy_unit}. \
                         <em>Those numbers are what the account sent: not converted, \
                         not judged, and not published — the bridge's own reading is \
                         the next link.</em>",
                        ago(now, at),
                        escape(&declared),
                        escape(&value_date),
                        power = power,
                        power_unit = escape(&power_unit),
                        energy = energy,
                        energy_unit = escape(&energy_unit),
                    )
                }
                SourceLink::Refused { cause, what } => format!(
                    "<strong>did not answer</strong>, {}: {} ({}). {}",
                    ago(now, at),
                    escape(&what),
                    cause.as_str(),
                    // The cause's own gesture (story 6.8), not the culprit's
                    // three-way one — this page exists to be acted on.
                    cause.gesture()
                ),
                SourceLink::NoCredential => {
                    "<strong>was not asked</strong>: there is no credential in the \
                     environment. Set SMARTME_CLIENT_ID and SMARTME_CLIENT_SECRET \
                     where the container reads them — there is no field for it on \
                     any screen, deliberately."
                        .to_string()
                }
            };
            (format!("<p><strong>1. The source.</strong> It {body}"), "")
        }
    };

    // --- link 2: what the bridge PUBLISHES, which is the poll loop's fact ---
    let state_of = fleet
        .as_ref()
        .and_then(|f| f.meters.iter().find(|m| m.meter == meter));
    let value_line = match state_of {
        None => "<p><strong>2. The value.</strong> This meter has no poll task — it \
                 is not enabled, or the runtime has not been rebuilt since it was \
                 added.</p>"
            .to_string(),
        Some(entry) => match entry.published {
            None => "<p><strong>2. The value.</strong> Nothing published yet: the \
                     first poll cycle has not completed. This is not silence, and it \
                     is not an error.</p>"
                .to_string(),
            Some(verdict) => format!(
                "<p><strong>2. The value.</strong> What the host is being told right \
                 now is <strong>{:?}</strong>{}, last published {}. This is the poll \
                 loop's verdict, reached at its own tick — <em>this check did not \
                 re-judge anything</em>, because the oracles judge against a memory a \
                 button must not move.{}</p>",
                verdict.quality(),
                verdict
                    .cause()
                    .map_or_else(String::new, |c| format!(" ({})", c.as_str())),
                entry
                    .last_published_at
                    .map_or_else(|| "never".to_string(), |at| ago(now, at)),
                entry.culprit.map_or_else(String::new, |c| format!(
                    " Whose fault: {} — {}.",
                    c.as_str(),
                    verdict.cause().map_or_else(
                        || repair(c).to_string(),
                        |cause| cause.gesture().to_string()
                    )
                )),
            ),
        },
    };

    // --- link 3: the sink, observed and never inferred (story 6.5) ---
    let drops: Vec<String> = fleet
        .as_ref()
        .map(|f| {
            f.dropped()
                .into_iter()
                .filter(|lost| *lost.meter == meter)
                .map(|lost| {
                    format!(
                        "{} × {}{}",
                        lost.count,
                        lost.reason.as_str(),
                        // The count of a disabled meter cannot rise, and saying
                        // so is the whole of [#90]: without it the operator
                        // reads their own deliberate gesture as an unexplained
                        // loss that simply stopped getting worse.
                        if lost.retired {
                            " (before you disabled it)"
                        } else {
                            ""
                        }
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let sink_line = format!(
        "<p><strong>3. The sink.</strong> {}{}</p>",
        sink_health_line(control.sink(), now),
        if drops.is_empty() {
            " No reading for this meter has been dropped.".to_string()
        } else {
            format!(
                " Readings lost for this meter: {}. A drop is counted, never \
                 silent — but it is a reading the host does not have.",
                escape(&drops.join(", "))
            )
        }
    );

    // The relation between the links, said rather than left to be inferred: this is
    // the sentence that stops link 2 being read as a judgement of link 1.
    let legend = "<p>The first link is what smart-me answered <em>just now</em>. The \
                  second is what this bridge is publishing, judged at its own last \
                  tick. <strong>They can disagree, and the disagreement is the \
                  useful part</strong>: a source that answers while the bridge still \
                  publishes a fault is a meter latched by something that already \
                  happened, and the fault names what clears it.</p>";

    page(
        "End-to-end check",
        &format!(
            "{refresh}<h1>End-to-end check</h1>{chooser}{refusal_line}\
             <h2>{}</h2>{source_line}</p>{value_line}{sink_line}{legend}\
             <p><a href=/>State of the bridge</a> · <a href=/meters>Meters</a></p>",
            escape(meter.as_str())
        ),
    )
    .into_response()
}
