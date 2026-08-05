//! Story 6.2 AC1/AC2/AC3/AC5/AC6/AC7 — a bridge is brought from empty to
//! publishing **without a text editor and without a terminal**.
//!
//! # Why this is one test and not six
//!
//! Every acceptance criterion in Story 6.2 is about a *journey*, and the defect
//! the story exists to remove is only visible along it: before this, an operator
//! could configure and confirm in a browser and still be met by a bridge that
//! kept saying *"nothing is published"* until somebody restarted the container.
//! Each step in isolation was green while the journey was broken.
//!
//! The steps that can be checked in isolation are checked in isolation — the
//! form's decoding, the origin guard and the mapping fingerprint are unit tests
//! in `ui::screens` and `ui::origin`. What is here is what only exists end to
//! end: **the phase actually turns.**
//!
//! # And it drives the real binary over a real socket
//!
//! Not the router in-process. The lifecycle loop lives in `main.rs`, the server
//! is spawned by it, and the transition under test is between two phases of that
//! process. An in-process test would exercise a second wiring of the same parts
//! and prove nothing about the one that ships.
//!
//! # Falsification, 2026-08-05
//!
//! Nine mutations, each asserting its own text changed before anything ran.
//! All went red. Output copied, not written:
//!
//! ```text
//! AC7 the readiness nudge is silenced
//!   panicked at tests/a_first_run_is_completed_in_the_browser.rs:331:9:
//!   AC7: after saving, the bridge must move itself to 'unconfirmed' — the
//!   lifecycle loop is what re-reads the file, and without it the operator would
//!   have to restart the container
//!
//! AC5 the origin guard is opened
//!   AC5: this UI has no login, so any page in the operator's browser could
//!   otherwise reconfigure it
//!
//! AC2 the faults are not rendered
//!   AC2: every fault must be shown at once […] "is missing or empty" is missing
//!
//! AC2 no fault is bound to its field
//!   AC2: three faults were submitted and 1 were rendered; they must appear
//!   together, each against the field it belongs to
//!
//! AC3 the topic leaves the confirmation screen
//!   AC3: the serial must be shown beside the exact topic and the device id […]
//!
//! the click is unbound from the mapping shown
//!   the click must be refused: it was given for a mapping that is no longer the
//!   one on disk, and confirming anyway would vouch for a device id nobody
//!   looked at
//!
//! AC1 the save writes nothing
//!   a valid submission must write the file
//!
//! AC4 every cost collapses to hot
//!   AC4: changing the broker needs a new Sparkplug session, which this process
//!   cannot open ([#49]). A screen that said 'saved' would report a change the
//!   bridge did not make
//!
//! AC4 a hot change is reported as needing a restart
//!   AC4: the publish period is genuinely hot — the poll loop re-reads it each
//!   tick — and the verdict must say so
//! ```
//!
//! # Two of these assertions were HOLLOW, and only the mutations found them
//!
//! **AC2.** It searched the page for `"group_id"`, `"publish_period_secs"` and
//! `"serial"` — which are the form's own input names, present whether a fault was
//! rendered or not — so a mutation that rendered no per-field fault at all left
//! it green. Worse, the green hid a real defect: the screen was binding faults by
//! `Fault::field` (a human label like `"publish period"`) instead of by
//! `Fault::source` (the key, `publish_period_secs`), so **nothing was ever shown
//! beside its field**, which is what AC2 asks for. Fixing the assertion is what
//! exposed the code.
//!
//! **AC4.** It searched for `"in force now"` — which the change *table* prints on
//! every hot row — so a mutation that made the verdict above the table say the
//! exact opposite left it green. Both now assert the full verdict sentence, which
//! nothing else on the page produces.
//!
//! Same family as `log.contains('3')` satisfied by the log's own timestamp. The
//! lesson is not "search for longer strings": it is that an assertion must name
//! something **only the property under test can produce**, and the way to find
//! out whether it does is to break the property.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[path = "common/port_lock.rs"]
mod port_lock;

/// The value the bridge is given, and which must never appear in a page.
const SECRET: &str = "s3cr3t-never-rendered-6-2";

fn state_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("smartme_6_2_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("scratch dir");
    path
}

fn spawn(dir: &std::path::Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_smartme-bridge"))
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("SMARTME_STATE_DIR", dir)
        .env("SMARTME_CLIENT_ID", "client-id")
        .env("SMARTME_CLIENT_SECRET", SECRET)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the bridge binary runs")
}

/// The smallest HTTP client that can answer the question, so the test does not
/// acquire a dependency to prove a page exists. Returns status line + headers +
/// body, because the status code is part of what several criteria assert.
fn request(port: u16, head: &str, body: Option<(&str, &str)>) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .ok()?;
    match body {
        None => write!(stream, "{head}\r\n\r\n").ok()?,
        Some((extra, payload)) => write!(
            stream,
            "{head}\r\n{extra}content-type: application/x-www-form-urlencoded\r\n\
             content-length: {}\r\n\r\n{payload}",
            payload.len()
        )
        .ok()?,
    }
    let mut out = String::new();
    stream.read_to_string(&mut out).ok()?;
    Some(out)
}

fn get(port: u16, path: &str) -> Option<String> {
    request(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close"),
        None,
    )
}

/// A POST as a browser on this UI would send it: `Origin` and `Host` agree.
fn post(port: u16, path: &str, payload: &str) -> Option<String> {
    request(
        port,
        &format!("POST {path} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close"),
        Some((&format!("origin: http://localhost:{port}\r\n"), payload)),
    )
}

fn wait_for_ui(port: u16, child: &mut Child) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Some(answer) = get(port, "/") {
            return Some(answer);
        }
        if child.try_wait().expect("wait").is_some() {
            return None; // it exited; nothing will ever answer
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

/// Poll `/` until it reports `expected`, or give up.
fn wait_for_state(port: u16, expected: &str) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last = String::new();
    while Instant::now() < deadline {
        if let Some(answer) = get(port, "/") {
            if answer.contains(expected) {
                return Some(answer);
            }
            last = answer;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!("last page seen while waiting for {expected:?}:\n{last}");
    None
}

/// A complete, valid submission. TEST-NET-1 (RFC 5737) is unroutable, so the
/// bridge can reach no broker and no meter — which is deliberate: what is under
/// test is the phase turning, not anything reaching a wire.
fn valid_form(ui_port: u16) -> String {
    format!(
        "group_id=Plant&node_id=Bridge01&broker_host=192.0.2.1&broker_port=1883\
         &publish_period_secs=30&api_base=https%3A%2F%2F192.0.2.1&ui_port={ui_port}\
         &meter.0.meter_id=garage\
         &meter.0.device_id=a1a1a1a1-b2b2-c3c3-d4d4-000000000001\
         &meter.0.serial=9202685&meter.0.enabled=1"
    )
}

/// Pull the mapping fingerprint out of the confirmation page.
///
/// The click is bound to it, so a test that invented its own value would be
/// testing a different submission than the one the operator makes.
fn fingerprint(page: &str) -> Option<String> {
    let at = page.find("name=mapping value=\"")? + "name=mapping value=\"".len();
    let rest = &page[at..];
    Some(rest[..rest.find('"')?].to_string())
}

/// **The journey.** Empty directory in, publishing bridge out, nothing but HTTP
/// in between.
#[test]
fn from_an_empty_directory_to_publishing_without_touching_a_terminal() {
    let _lock = port_lock::PortLock::acquire(smartme_bridge::ui::DEFAULT_PORT);
    let port = smartme_bridge::ui::DEFAULT_PORT;
    let dir = state_dir("journey");
    let mut child = spawn(&dir);

    let outcome = (|| -> Result<(), String> {
        // ---- The first run: no file at all.
        let home = wait_for_ui(port, &mut child)
            .ok_or("the UI must answer on the default port with no configuration")?;
        if !home.contains("Not configured yet") {
            return Err(format!("expected the unconfigured state:\n{home}"));
        }
        if std::fs::exists(dir.join("config.toml")).unwrap_or(false) {
            return Err("nothing should have been written yet".into());
        }

        // ---- AC1 + AC6: the form exists, and it has no credential field.
        let form = get(port, "/config").ok_or("/config must answer")?;
        for forbidden in ["client_secret", "client_id", "credential", "password"] {
            if form.contains(forbidden) {
                return Err(format!(
                    "AC6: the configuration form must have NO credential field — \
                     not empty, not masked, absent. Found {forbidden:?}:\n{form}"
                ));
            }
        }

        // ---- AC2: a bad submission is refused, with every fault at once.
        let bad = post(
            port,
            "/config",
            "group_id=&node_id=Bridge01&broker_host=192.0.2.1&broker_port=1883\
             &publish_period_secs=99999&meter.0.meter_id=garage\
             &meter.0.device_id=d&meter.0.serial=0912345&meter.0.enabled=1",
        )
        .ok_or("a refused submission must still answer")?;
        if !bad.contains("400 Bad Request") {
            return Err(format!(
                "a refused submission must not report success:\n{bad}"
            ));
        }
        // The assertion is on the fault MESSAGES, and it is written this way on
        // purpose.
        //
        // The first version searched the page for `"group_id"`,
        // `"publish_period_secs"` and `"serial"` — and a mutation that rendered
        // NO per-field fault at all left it green, because those words are the
        // form's own input names and labels. The page always contains them.
        // Exactly the class of hollow assertion this repository has already paid
        // for twice (`log.contains('3')` satisfied by a timestamp). These
        // fragments occur nowhere but in a `Fault`'s own words.
        let must_say = [
            "is missing or empty", // group_id
            "outside 5s..=300s",   // publish_period_secs = 99999
            "has a leading zero",  // serial = 0912345
        ];
        for phrase in must_say {
            if !bad.contains(phrase) {
                return Err(format!(
                    "AC2: every fault must be shown at once — a form that showed one \
                     at a time would rebuild the six edit-restart cycles Story 5.1 \
                     removed. {phrase:?} is missing:\n{bad}"
                ));
            }
        }
        // And each beside its own field rather than in one lump: three faults
        // means three renderings.
        let rendered = bad.matches("class=fault").count();
        if rendered < 3 {
            return Err(format!(
                "AC2: three faults were submitted and {rendered} were rendered; they \
                 must appear together, each against the field it belongs to:\n{bad}"
            ));
        }
        if std::fs::exists(dir.join("config.toml")).unwrap_or(false) {
            return Err("a refused submission must write nothing".into());
        }

        // ---- AC5: a submission from somewhere else is refused.
        let foreign = request(
            port,
            &format!("POST /config HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close"),
            Some(("origin: http://evil.example\r\n", &valid_form(port))),
        )
        .ok_or("a cross-origin POST must still answer")?;
        if !foreign.contains("403 Forbidden") {
            return Err(format!(
                "AC5: this UI has no login, so any page in the operator's browser \
                 could otherwise reconfigure it:\n{foreign}"
            ));
        }
        if std::fs::exists(dir.join("config.toml")).unwrap_or(false) {
            return Err("a refused cross-origin submission must write nothing".into());
        }

        // ---- AC1: the real save.
        let saved = post(port, "/config", &valid_form(port)).ok_or("the save must answer")?;
        if !saved.contains("200 OK") {
            return Err(format!("a valid submission must be accepted:\n{saved}"));
        }
        if !std::fs::exists(dir.join("config.toml")).unwrap_or(false) {
            return Err("a valid submission must write the file".into());
        }

        // ---- AC7, first half: the phase turned with nobody restarting anything.
        let waiting = wait_for_state(port, "confirm the meter mapping").ok_or(
            "AC7: after saving, the bridge must move itself to 'unconfirmed' — the \
             lifecycle loop is what re-reads the file, and without it the operator \
             would have to restart the container",
        )?;
        if waiting.contains("Not configured yet") {
            return Err("the two silent states must not be confused".into());
        }

        // ---- AC3: the confirmation is its own screen and its own submission.
        let preview = get(port, "/confirm").ok_or("/confirm must answer")?;
        for shown in [
            "9202685",
            "spBv1.0/Plant/DDATA/Bridge01/9202685",
            "a1a1a1a1-b2b2-c3c3-d4d4-000000000001",
        ] {
            if !preview.contains(shown) {
                return Err(format!(
                    "AC3: the serial must be shown beside the exact topic and the \
                     device id — a screen showing only names is a click that proves \
                     nothing, since a name is the part that looks right when it is \
                     wrong. Missing {shown:?}:\n{preview}"
                ));
            }
        }

        // ---- AC7, second half: the click that ends the silence.
        let mapping = fingerprint(&preview).ok_or(
            "the confirmation form must carry the \
             mapping it displayed, or the click would bless whatever is on disk",
        )?;
        let confirmed = post(port, "/confirm", &format!("mapping={mapping}"))
            .ok_or("the confirmation must answer")?;
        if !(confirmed.contains("303 See Other") || confirmed.contains("200 OK")) {
            return Err(format!("the confirmation must be accepted:\n{confirmed}"));
        }

        let health = (|| {
            let deadline = Instant::now() + Duration::from_secs(25);
            while Instant::now() < deadline {
                if let Some(h) = get(port, "/healthz")
                    && h.contains("\"status\":\"running\"")
                {
                    return Some(h);
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            None
        })()
        .ok_or(
            "AC7: confirming must start the publishing bridge with NO human \
             intervention. This is the criterion the story added while being \
             written, because `run_without_publishing` could do nothing but wait \
             to be killed — the operator was met by a bridge still saying \
             'nothing is published' until somebody restarted the container",
        )?;
        if !health.contains("\"intends_to_publish\":true") {
            return Err(format!("a running bridge must say so:\n{health}"));
        }

        // ---- AC4: what a change COSTS, said before the operator believes it
        // happened.
        //
        // This step is only reachable here, with the bridge publishing: a silent
        // bridge has no configuration in force to compare a submission against,
        // so `Plan` has nothing to say. Changing the broker is a `NewSession`,
        // which [#49] means this process cannot carry out — so the one thing the
        // screen must not do is report it as done.
        let hot = post(
            port,
            "/config",
            &valid_form(port).replace("publish_period_secs=30", "publish_period_secs=60"),
        )
        .ok_or("a change to a running bridge must answer")?;
        // The FULL headline, not the fragment. `"in force now"` alone is also
        // what the change table prints on the row, so an assertion on it was
        // satisfied while the verdict above said the opposite — found by
        // falsification, and the second hollow assertion in this one file.
        if !hot.contains("<strong>Saved, and in force now.</strong>") {
            return Err(format!(
                "AC4: the publish period is genuinely hot — the poll loop re-reads it \
                 each tick — and the verdict must say so:\n{hot}"
            ));
        }

        let costly = post(
            port,
            "/config",
            &valid_form(port).replace("broker_host=192.0.2.1", "broker_host=192.0.2.9"),
        )
        .ok_or("a costly change must answer")?;
        if !costly.contains("<strong>Saved — but NOT in force.") {
            return Err(format!(
                "AC4: changing the broker needs a new Sparkplug session, which this \
                 process cannot open ([#49]). A screen that said 'saved' would report \
                 a change the bridge did not make:\n{costly}"
            ));
        }
        if !costly.contains("broker_host") {
            return Err(format!(
                "AC4: and it must name WHICH setting is waiting:\n{costly}"
            ));
        }
        // And the bridge must still be publishing on the broker it actually has:
        // `Control::current()` reports what is in force, not what was posted.
        let after = get(port, "/healthz").ok_or("/healthz must answer")?;
        if !after.contains("\"status\":\"running\"") {
            return Err(format!(
                "a change that was NOT applied must not disturb the session:\n{after}"
            ));
        }

        // ---- AC6: and no page ever carried the credential.
        //
        // The guard pattern `secret_never_reaches_the_log.rs` uses: prove the
        // search FINDS the value when it is there, then prove it is not. Without
        // the first half a typo in the needle would make this pass forever.
        let haystack = format!(
            "{home}{form}{bad}{saved}{waiting}{preview}{confirmed}{health}{hot}{costly}{after}"
        );
        if !format!("{haystack}{SECRET}").contains(SECRET) {
            return Err("the search itself is broken; this test proves nothing".into());
        }
        if haystack.contains(SECRET) {
            return Err("AC6: the credential reached a rendered page".into());
        }
        Ok(())
    })();

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(why) = outcome {
        panic!("{why}");
    }
}

/// AC3, the half a happy path cannot show: **the click is bound to the mapping
/// that was displayed**, not to whatever is on disk when it lands.
///
/// `store::confirm` blesses the file it finds. Without the fingerprint, a write
/// that landed between the preview and the click would be confirmed instead —
/// the operator would have looked at one mapping and vouched for another, which
/// is precisely the mis-map FR25 exists to catch.
#[test]
fn a_confirmation_does_not_carry_over_to_a_mapping_the_operator_never_saw() {
    let _lock = port_lock::PortLock::acquire(18099);
    let port = 18099u16;
    let dir = state_dir("stale_confirmation");
    std::fs::write(
        dir.join("config.toml"),
        format!(
            "schema_version = {}\ngroup_id = \"Plant\"\nnode_id = \"Bridge01\"\n\
             broker_host = \"192.0.2.1\"\nbroker_port = 1883\n\
             publish_period_secs = 30\napi_base = \"https://192.0.2.1\"\n\
             mapping_confirmed = false\nui_port = {port}\n\n\
             [[meters]]\nmeter_id = \"garage\"\n\
             device_id = \"a1a1a1a1-b2b2-c3c3-d4d4-000000000001\"\n\
             serial = \"9202685\"\nenabled = true\n",
            smartme_bridge::app::store::SCHEMA_VERSION
        ),
    )
    .expect("write config");

    let mut child = spawn(&dir);
    let outcome = (|| -> Result<(), String> {
        wait_for_ui(port, &mut child).ok_or("the UI must answer")?;
        let preview = get(port, "/confirm").ok_or("/confirm must answer")?;
        let mapping = fingerprint(&preview).ok_or("the form must carry a fingerprint")?;

        // The interleaved write: a different device, behind the operator's back.
        let on_disk =
            std::fs::read_to_string(dir.join("config.toml")).map_err(|e| e.to_string())?;
        std::fs::write(
            dir.join("config.toml"),
            on_disk.replace("000000000001", "000000000002"),
        )
        .map_err(|e| e.to_string())?;

        let answer = post(port, "/confirm", &format!("mapping={mapping}"))
            .ok_or("the confirmation must answer")?;
        if !answer.contains("409 Conflict") {
            return Err(format!(
                "the click must be refused: it was given for a mapping that is no \
                 longer the one on disk, and confirming anyway would vouch for a \
                 device id nobody looked at:\n{answer}"
            ));
        }
        let after = std::fs::read_to_string(dir.join("config.toml")).map_err(|e| e.to_string())?;
        if after.contains("mapping_confirmed = true") {
            return Err("nothing must have been confirmed".into());
        }
        Ok(())
    })();

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(why) = outcome {
        panic!("{why}");
    }
}

/// **The UI must never describe a configured bridge as unconfigured — not for one
/// request, not for one millisecond.**
///
/// Story 6.2 made the web server outlive every phase, which is what lets a
/// confirmation start publishing without a restart. It also means the server
/// answers *before* `run_with_control` hands back the `Control` — and in that
/// window the phase was still the handle's initial `Unconfigured`. A bridge with a
/// valid, confirmed configuration, busy opening its MQTT session, served a page
/// saying "Not configured yet".
///
/// **It was found by CI, not by this suite**, and not by design: the window never
/// opened on a developer machine and opened on every GitHub run. An operator who
/// met it would reasonably have gone and rewritten a configuration that was
/// already correct — the exact class of lie this project exists to prevent, told
/// by the screen built to prevent it.
///
/// So this asks the question the way it has to be asked: hammer `/` from the
/// instant the socket accepts and require that **no** answer, ever, claims the
/// bridge is unconfigured. A test that waited for a settled state would have
/// watched the window close and reported success.
///
/// FALSIFIED 2026-08-05 by restoring the defect exactly — `Decision::Publish`
/// stores no phase, so the handle keeps its initial value. Output copied:
///
/// ```text
/// test a_confirmed_bridge_is_never_described_as_unconfigured_even_for_one_request ... FAILED
/// panicked at tests/a_first_run_is_completed_in_the_browser.rs:598:9:
/// after 1 request(s) the bridge described itself as UNCONFIGURED while holding
/// a valid, confirmed configuration.
/// ```
///
/// **`after 1 request(s)` is the number worth reading.** The window was not
/// narrow and lucky to hit — it was the very first answer the server gave. It
/// stayed invisible only because every existing test polled until the page said
/// what it wanted, and threw away everything the bridge said on the way.
#[test]
fn a_confirmed_bridge_is_never_described_as_unconfigured_even_for_one_request() {
    let _lock = port_lock::PortLock::acquire(18097);
    let port = 18097u16;
    let dir = state_dir("never_unconfigured");
    std::fs::write(
        dir.join("config.toml"),
        format!(
            "schema_version = {}\ngroup_id = \"Plant\"\nnode_id = \"Bridge01\"\n\
             broker_host = \"192.0.2.1\"\nbroker_port = 1883\n\
             publish_period_secs = 30\napi_base = \"https://192.0.2.1\"\n\
             mapping_confirmed = true\nui_port = {port}\n\n\
             [[meters]]\nmeter_id = \"garage\"\n\
             device_id = \"a1a1a1a1-b2b2-c3c3-d4d4-000000000001\"\n\
             serial = \"9202685\"\nenabled = true\n",
            smartme_bridge::app::store::SCHEMA_VERSION
        ),
    )
    .expect("write config");

    let mut child = spawn(&dir);
    // No sleep between attempts: the whole point is to catch the earliest answer
    // the server is capable of giving.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut answers = 0usize;
    let mut liar: Option<String> = None;
    let mut reached_running = false;
    while Instant::now() < deadline {
        match get(port, "/") {
            Some(page) => {
                answers += 1;
                if page.contains("Not configured yet") {
                    liar = Some(page);
                    break;
                }
                if page.contains("Running") {
                    reached_running = true;
                    break;
                }
            }
            None => {
                if child.try_wait().expect("wait").is_some() {
                    break;
                }
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);

    if let Some(page) = liar {
        panic!(
            "after {answers} request(s) the bridge described itself as UNCONFIGURED \
             while holding a valid, confirmed configuration. An operator meeting \
             this would go and rewrite a file that was already correct:\n{page}"
        );
    }
    assert!(
        reached_running,
        "the premise failed: the bridge never reached Running at all, so this test \
         proved nothing about the window between phases"
    );
}
