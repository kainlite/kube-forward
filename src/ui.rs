use std::io::IsTerminal;

use crate::config::ForwardConfig;

/// Snapshot of a single forward's initial outcome, used for the startup summary.
#[derive(Debug, Clone, PartialEq)]
pub struct ForwardStatus {
    pub name: String,
    pub established: bool,
    pub detail: Option<String>,
}

/// Whether ANSI color should be emitted: only when stdout is a terminal and the
/// user has not opted out via the conventional `NO_COLOR` variable.
pub fn should_color() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn paint(text: &str, code: &str, color: bool) -> String {
    if color {
        format!("\x1b[{}m{}\x1b[0m", code, text)
    } else {
        text.to_string()
    }
}

fn green(text: &str, color: bool) -> String {
    paint(text, "32", color)
}
fn red(text: &str, color: bool) -> String {
    paint(text, "31", color)
}
fn yellow(text: &str, color: bool) -> String {
    paint(text, "33", color)
}
fn dim(text: &str, color: bool) -> String {
    paint(text, "2", color)
}
fn bold(text: &str, color: bool) -> String {
    paint(text, "1", color)
}

/// Build the aligned table of configured forwards. Pure (no I/O), so it can be tested.
pub fn forward_table(configs: &[ForwardConfig]) -> String {
    if configs.is_empty() {
        return String::new();
    }

    let addresses: Vec<String> = configs
        .iter()
        .map(|c| {
            format!(
                "127.0.0.1:{} -> {}:{}",
                c.ports.local, c.target, c.ports.remote
            )
        })
        .collect();

    let name_w = configs.iter().map(|c| c.name.len()).max().unwrap_or(0);
    let addr_w = addresses.iter().map(|s| s.len()).max().unwrap_or(0);

    let mut out = String::new();
    for (config, address) in configs.iter().zip(addresses.iter()) {
        let protocol = config
            .ports
            .protocol
            .as_deref()
            .unwrap_or("TCP")
            .to_uppercase();
        out.push_str(&format!(
            "  {:<name_w$}  {:<addr_w$}  {}\n",
            config.name, address, protocol
        ));
    }
    out
}

/// Build the post-startup summary line(s): how many forwards came up and which failed.
pub fn startup_summary(statuses: &[ForwardStatus], color: bool) -> String {
    let total = statuses.len();
    let established = statuses.iter().filter(|s| s.established).count();
    let failures: Vec<&ForwardStatus> = statuses.iter().filter(|s| !s.established).collect();

    let mut line = format!(
        "{} {}/{} established",
        green("\u{2713}", color),
        established,
        total
    );

    if failures.is_empty() {
        return line;
    }

    line.push_str(&format!(
        "  {} {} failed",
        red("\u{2717}", color),
        failures.len()
    ));
    let mut out = line;
    for failure in failures {
        let reason = failure.detail.as_deref().unwrap_or("unknown error");
        out.push_str(&format!(
            "\n    {} {}: {}",
            red("\u{2717}", color),
            failure.name,
            reason
        ));
    }
    out
}

/// Print the startup banner and the configured-forwards table to stdout.
pub fn print_startup(version: &str, configs: &[ForwardConfig]) {
    let color = should_color();
    println!(
        "{} {} \u{2014} {} forward{} configured\n",
        bold("kube-forward", color),
        version,
        configs.len(),
        if configs.len() == 1 { "" } else { "s" }
    );
    print!("{}", forward_table(configs));
    println!();
}

/// Print the startup summary, then the "watching" footer.
pub fn print_summary(statuses: &[ForwardStatus]) {
    let color = should_color();
    println!("{}", startup_summary(statuses, color));
    println!(
        "{}",
        dim("watching for changes, press Ctrl-C to stop", color)
    );
}

/// Print the report produced by config validation. Returns true when valid.
pub fn print_validation(report: &crate::validate::ValidationReport) -> bool {
    let color = should_color();
    for warning in &report.warnings {
        println!("{} {}", yellow("warning:", color), warning);
    }
    for error in &report.errors {
        println!("{} {}", red("error:", color), error);
    }
    if report.is_valid() {
        println!("{} configuration is valid", green("\u{2713}", color));
        true
    } else {
        println!(
            "{} {} error{} found",
            red("\u{2717}", color),
            report.errors.len(),
            if report.errors.len() == 1 { "" } else { "s" }
        );
        false
    }
}
