use std::time::Duration;

use serde::Serialize;

use crate::{App, ToolProfile};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DiagnosticCheck {
    pub name: String,
    pub status: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DoctorReport {
    pub name: String,
    pub version: String,
    pub tool_profile: String,
    pub online_checks: bool,
    pub ok: bool,
    pub checks: Vec<DiagnosticCheck>,
}

impl DoctorReport {
    #[must_use]
    pub fn human(&self) -> String {
        let mut lines = vec![format!(
            "{} {} — profile: {}",
            self.name, self.version, self.tool_profile
        )];
        for check in &self.checks {
            let marker = match check.status.as_str() {
                "pass" => "PASS",
                "skip" => "SKIP",
                _ => "FAIL",
            };
            lines.push(format!("[{marker}] {}: {}", check.name, check.message));
        }
        lines.push(if self.ok {
            "Result: ready".to_owned()
        } else {
            "Result: one or more checks failed".to_owned()
        });
        lines.join("\n")
    }
}

pub async fn run_doctor(profile: ToolProfile, online: bool) -> DoctorReport {
    let mut report = DoctorReport {
        name: env!("CARGO_PKG_NAME").to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        tool_profile: profile.to_string(),
        online_checks: online,
        ok: false,
        checks: Vec::new(),
    };

    let app = match App::new() {
        Ok(app) => app,
        Err(error) => {
            report.checks.push(fail("configuration", error.to_string()));
            return report;
        }
    };
    let status = app.status();
    report.checks.push(pass(
        "configuration",
        format!(
            "CLOB protocol {} and valid endpoint configuration",
            status.clob_protocol
        ),
    ));
    report.checks.push(pass(
        "sqlite",
        format!("database initialized at {}", status.database_path),
    ));

    if !online {
        report.checks.push(skip(
            "upstreams",
            "network checks disabled by --offline".to_owned(),
        ));
        report.ok = true;
        let _ = app.shutdown().await;
        return report;
    }

    let (clob, data, gamma) = tokio::join!(
        app.clob_health(),
        app.data_health(),
        app.search_markets("bitcoin".to_owned(), Some(1))
    );
    match clob {
        Ok(message) => report.checks.push(pass("clob-rest", message)),
        Err(error) => report.checks.push(fail("clob-rest", error.to_string())),
    }
    match data {
        Ok(message) => report.checks.push(pass("data-api", message)),
        Err(error) => report.checks.push(fail("data-api", error.to_string())),
    }

    let token_id = match gamma {
        Ok(result) => {
            let token_id = result
                .markets
                .iter()
                .flat_map(|market| &market.outcomes)
                .find_map(|outcome| outcome.token_id.clone());
            if result.count == 0 {
                report.checks.push(fail(
                    "gamma-api",
                    "search returned no active markets".to_owned(),
                ));
            } else {
                report.checks.push(pass(
                    "gamma-api",
                    format!("search returned {} active market(s)", result.count),
                ));
            }
            token_id
        }
        Err(error) => {
            report.checks.push(fail("gamma-api", error.to_string()));
            None
        }
    };

    if let Some(token_id) = token_id {
        diagnose_websocket(&app, token_id, &mut report).await;
    } else {
        report.checks.push(skip(
            "clob-websocket",
            "no active outcome token was available for a subscription".to_owned(),
        ));
    }

    if let Err(error) = app.shutdown().await {
        report.checks.push(fail("shutdown", error.to_string()));
    }
    report.ok = report.checks.iter().all(|check| check.status != "fail");
    report
}

async fn diagnose_websocket(app: &App, token_id: String, report: &mut DoctorReport) {
    let watch = match app.watch_markets(vec![token_id]).await {
        Ok(watch) => watch,
        Err(error) => {
            report
                .checks
                .push(fail("clob-websocket", error.to_string()));
            return;
        }
    };
    let mut connected = false;
    let mut last_error = None;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let status = app.get_realtime_status().await;
        if let Some(state) = status
            .watches
            .iter()
            .find(|state| state.watch_id == watch.watch_id)
        {
            last_error.clone_from(&state.last_error);
            if state.connection_state == "Connected" || state.websocket_update_count > 0 {
                connected = true;
                break;
            }
        }
    }
    let _ = app.stop_watching(watch.watch_id).await;
    if connected {
        report.checks.push(pass(
            "clob-websocket",
            "connected and subscribed to a live outcome token".to_owned(),
        ));
    } else {
        report.checks.push(fail(
            "clob-websocket",
            last_error.unwrap_or_else(|| {
                "connection did not become ready within 20 seconds; if a VPN or system proxy is active, set POLYMARKET_WS_PROXY=socks5://host:port explicitly".to_owned()
            }),
        ));
    }
}

fn pass(name: &str, message: String) -> DiagnosticCheck {
    DiagnosticCheck {
        name: name.to_owned(),
        status: "pass".to_owned(),
        message,
    }
}

fn fail(name: &str, message: String) -> DiagnosticCheck {
    DiagnosticCheck {
        name: name.to_owned(),
        status: "fail".to_owned(),
        message,
    }
}

fn skip(name: &str, message: String) -> DiagnosticCheck {
    DiagnosticCheck {
        name: name.to_owned(),
        status: "skip".to_owned(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_report_is_scannable() {
        let report = DoctorReport {
            name: "test".to_owned(),
            version: "1.0.0".to_owned(),
            tool_profile: "research".to_owned(),
            online_checks: false,
            ok: true,
            checks: vec![pass("sqlite", "ready".to_owned())],
        };
        let output = report.human();
        assert!(output.contains("[PASS] sqlite: ready"));
        assert!(output.ends_with("Result: ready"));
    }
}
