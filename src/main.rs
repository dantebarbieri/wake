mod net;
mod presets;
mod update;

use std::net::{Ipv4Addr, SocketAddrV4};
use std::process::{Command, ExitCode};
use std::thread;
use std::time::Duration;

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Parser, Subcommand};
use console::{style, Emoji, StyledObject};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

use presets::{format_mac, Preset, PRESETS};

const DEFAULT_WAIT_SECS: &str = "240";
const PRECHECK_TIMEOUT: Duration = Duration::from_millis(800);

static OK: Emoji = Emoji("✔", "+");
static FAIL: Emoji = Emoji("✖", "x");
static UP: Emoji = Emoji("●", "*");
static DOWN: Emoji = Emoji("○", "o");
static SENT: Emoji = Emoji("➜", ">");
static SKIP: Emoji = Emoji("↷", "-");
static WARN: Emoji = Emoji("▲", "!");

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default());

const EXAMPLES: &str = "\x1b[1;32mExamples:\x1b[0m
  wake totoro           Wake Totoro and wait until it answers
  wake totoro -W        Send without waiting for a reply
  wake all -w 60        Wake every other machine, wait up to 60 s
  wake status           Show which machines are up
  wake list             Show the presets";

#[derive(Parser)]
#[command(
    name = "wake",
    version,
    about = "Wake the machines in the house with Wake-on-LAN.",
    styles = STYLES,
    after_help = EXAMPLES,
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Machines to wake: preset names, `all`, or MAC addresses.
    /// The machine running `wake` is always skipped.
    #[arg(value_name = "TARGET")]
    targets: Vec<String>,

    /// Seconds to wait for each machine to answer
    #[arg(short, long, value_name = "SECS", num_args = 0..=1,
          default_value = DEFAULT_WAIT_SECS,
          default_missing_value = DEFAULT_WAIT_SECS, value_parser = parse_secs)]
    wait: Duration,

    /// Skip waiting for a reply after sending
    #[arg(short = 'W', long, conflicts_with = "wait")]
    no_wait: bool,

    /// TCP endpoint that means "awake" (one target only)
    #[arg(long, value_name = "IP:PORT")]
    check: Option<SocketAddrV4>,

    /// Send even if the machine already answers
    #[arg(short, long)]
    force: bool,

    /// Broadcast address for the magic packet
    #[arg(long, value_name = "IPV4", default_value_t = presets::LAN_BROADCAST)]
    broadcast: Ipv4Addr,

    /// UDP port for the magic packet
    #[arg(long, default_value_t = presets::WOL_PORT, value_parser = clap::value_parser!(u16).range(1..))]
    port: u16,

    /// Print only errors
    #[arg(short, long, global = true)]
    quiet: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show the machines wake knows about
    List,
    /// Show which machines are up
    Status,
    /// Update wake to the latest release
    SelfUpdate {
        /// Only report whether an update is available
        #[arg(long)]
        check: bool,
    },
}

fn parse_secs(value: &str) -> Result<Duration, String> {
    match value.parse::<f64>() {
        Ok(s) if s.is_finite() && s > 0.0 && s <= 86_400.0 => Ok(Duration::from_secs_f64(s)),
        _ => Err("must be a number of seconds between 0 and 86400".into()),
    }
}

struct Target {
    name: String,
    macs: Vec<[u8; 6]>,
    check: Option<(SocketAddrV4, &'static str)>,
    bmc: Option<Ipv4Addr>,
    hint: Option<&'static str>,
}

impl Target {
    fn from_preset(p: &Preset) -> Self {
        Target {
            name: p.name.into(),
            macs: p.macs.to_vec(),
            check: Some((SocketAddrV4::new(p.ip, p.check.0), p.check.1)),
            bmc: p.bmc,
            hint: Some(p.hint),
        }
    }
}

/// Returns the targets to wake and the names skipped as this machine.
fn resolve(
    args: &[String],
    local: &std::collections::HashSet<[u8; 6]>,
) -> Result<(Vec<Target>, Vec<String>), String> {
    let mut targets: Vec<Target> = Vec::new();
    let mut skipped = Vec::new();
    let mut add = |t: Target, skipped: &mut Vec<String>| {
        if targets.iter().any(|o| o.name == t.name) || skipped.contains(&t.name) {
            return;
        }
        if t.macs.iter().any(|m| local.contains(m)) {
            skipped.push(t.name);
        } else {
            targets.push(t);
        }
    };
    for arg in args {
        if arg.eq_ignore_ascii_case("all") {
            PRESETS
                .iter()
                .for_each(|p| add(Target::from_preset(p), &mut skipped));
        } else if let Some(p) = presets::find(arg) {
            add(Target::from_preset(p), &mut skipped);
        } else {
            let mac = presets::parse_mac(arg).map_err(|e| {
                let names: Vec<_> = PRESETS.iter().map(|p| p.name).collect();
                format!("{e}\n  presets: {}, all", names.join(", "))
            })?;
            let preset = PRESETS.iter().find(|p| p.macs.contains(&mac));
            match preset {
                Some(p) => add(Target::from_preset(p), &mut skipped),
                None => add(
                    Target {
                        name: format_mac(&mac),
                        macs: vec![mac],
                        check: None,
                        bmc: None,
                        hint: None,
                    },
                    &mut skipped,
                ),
            }
        }
    }
    Ok((targets, skipped))
}

/// Spinners on stderr, permanent result lines that survive them.
struct Ui {
    mp: MultiProgress,
    quiet: bool,
    width: usize,
}

impl Ui {
    fn new(quiet: bool, width: usize) -> Self {
        let mp = MultiProgress::new();
        if quiet {
            mp.set_draw_target(ProgressDrawTarget::hidden());
        }
        Ui { mp, quiet, width }
    }

    fn line(&self, glyph: StyledObject<Emoji>, name: &str, msg: impl std::fmt::Display) {
        if !self.quiet {
            self.emit(glyph, name, msg);
        }
    }

    /// Like `line`, but shown even with --quiet.
    fn error(&self, glyph: StyledObject<Emoji>, name: &str, msg: impl std::fmt::Display) {
        self.emit(glyph, name, msg);
    }

    fn emit(&self, glyph: StyledObject<Emoji>, name: &str, msg: impl std::fmt::Display) {
        let line = format!(
            "{glyph} {} {msg}",
            style(format!("{name:<w$}", w = self.width)).bold()
        );
        if self.mp.is_hidden() {
            if self.quiet {
                eprintln!("{line}")
            } else {
                println!("{line}")
            }
        } else {
            let _ = self.mp.println(line);
        }
    }

    fn spinner(&self, name: &str, msg: String) -> ProgressBar {
        let pb = self.mp.add(ProgressBar::new_spinner());
        pb.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {prefix:.bold} {msg} {elapsed:.dim}")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", " "]),
        );
        pb.set_prefix(format!("{name:<w$}", w = self.width));
        pb.set_message(msg);
        pb.enable_steady_tick(Duration::from_millis(80));
        pb
    }
}

fn secs(d: Duration) -> String {
    let s = d.as_secs_f64();
    if s < 10.0 {
        format!("{s:.1}s")
    } else {
        format!("{s:.0}s")
    }
}

fn wake(cli: &Cli) -> Result<bool, String> {
    let (mut targets, skipped) = resolve(&cli.targets, &net::local_macs())?;
    if let Some(check) = cli.check {
        if targets.len() + skipped.len() != 1 {
            return Err("--check needs exactly one target".into());
        }
        if let Some(t) = targets.first_mut() {
            t.check = Some((check, "port"));
        }
    }
    let width = targets
        .iter()
        .map(|t| t.name.len())
        .chain(skipped.iter().map(String::len))
        .max()
        .unwrap_or(0);
    let ui = Ui::new(cli.quiet, width);
    for name in &skipped {
        ui.line(
            style(SKIP).dim(),
            name,
            style("is this machine, skipping").dim(),
        );
    }
    let results: Vec<bool> = thread::scope(|s| {
        let handles: Vec<_> = targets
            .iter()
            .map(|t| s.spawn(|| wake_one(cli, &ui, t)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or(false))
            .collect()
    });
    Ok(results.into_iter().all(|ok| ok))
}

fn wake_one(cli: &Cli, ui: &Ui, t: &Target) -> bool {
    let name = t.name.as_str();
    if let (Some((addr, service)), false) = (t.check, cli.force) {
        let pb = ui.spinner(name, format!("checking {service} on {addr}"));
        let up = net::probe(addr, PRECHECK_TIMEOUT).is_ok();
        pb.finish_and_clear();
        if up {
            ui.line(
                style(UP).green(),
                name,
                format!(
                    "already awake {}",
                    style(format!("· {service} answered")).dim()
                ),
            );
            return true;
        }
    }

    let dest = SocketAddrV4::new(cli.broadcast, cli.port);
    for mac in &t.macs {
        if let Err(e) = net::send_magic_packet(mac, dest) {
            ui.error(
                style(FAIL).red(),
                name,
                style(format!("couldn't send to {dest}: {e}")).red(),
            );
            return false;
        }
    }
    let macs = t.macs.iter().map(format_mac).collect::<Vec<_>>().join(", ");
    let wait_for = t.check.filter(|_| !cli.no_wait);
    let glyph = if wait_for.is_some() {
        style(SENT).cyan()
    } else {
        style(OK).green()
    };
    ui.line(
        glyph,
        name,
        format!(
            "magic packet sent {}",
            style(format!("· {macs} → {dest}")).dim()
        ),
    );

    if let Some(bmc) = t.bmc {
        ipmi_power_on(ui, name, bmc);
    }

    let Some((addr, service)) = wait_for else {
        return true;
    };
    let pb = ui.spinner(name, format!("waiting for {service} on {addr}"));
    let result = net::wait_until_up(addr, cli.wait);
    pb.finish_and_clear();
    match result {
        Ok(after) => {
            ui.line(
                style(OK).green(),
                name,
                format!(
                    "awake after {} {}",
                    secs(after),
                    style(format!("· {service} answered")).dim()
                ),
            );
            true
        }
        Err(last) => {
            ui.error(
                style(FAIL).red(),
                name,
                format!(
                    "{} {}",
                    style(format!(
                        "no answer from {service} on {addr} within {}",
                        secs(cli.wait)
                    ))
                    .red(),
                    style(format!("({last})")).dim()
                ),
            );
            if let Some(hint) = t.hint {
                ui.error(style(WARN).yellow(), "", style(hint).yellow());
            }
            false
        }
    }
}

/// Optional: asks the BMC to power the machine on, when credentials are set.
fn ipmi_power_on(ui: &Ui, name: &str, bmc: Ipv4Addr) {
    let Ok(user) = std::env::var("WAKE_IPMI_USER") else {
        return;
    };
    if std::env::var_os("IPMI_PASSWORD").is_none() {
        ui.line(
            style(WARN).yellow(),
            name,
            style("WAKE_IPMI_USER is set but IPMI_PASSWORD isn't; skipping IPMI").yellow(),
        );
        return;
    }
    let pb = ui.spinner(name, format!("asking BMC {bmc} to power on"));
    // -E reads the password from IPMI_PASSWORD, keeping it off the command line.
    let out = Command::new("ipmitool")
        .args([
            "-I",
            "lanplus",
            "-H",
            &bmc.to_string(),
            "-U",
            &user,
            "-E",
            "chassis",
            "power",
            "on",
        ])
        .output();
    pb.finish_and_clear();
    match out {
        Ok(o) if o.status.success() => ui.line(
            style(OK).green(),
            name,
            format!("BMC power-on requested {}", style(format!("· {bmc}")).dim()),
        ),
        Ok(o) => ui.error(
            style(FAIL).red(),
            name,
            style(format!(
                "ipmitool: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            ))
            .red(),
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ui.line(
            style(WARN).yellow(),
            name,
            style("ipmitool isn't installed; skipped BMC power-on").yellow(),
        ),
        Err(e) => ui.error(
            style(FAIL).red(),
            name,
            style(format!("ipmitool: {e}")).red(),
        ),
    }
}

fn list() {
    let local = net::local_macs();
    println!("{}", style("Machines").green().bold());
    for p in PRESETS {
        let me = p.macs.iter().any(|m| local.contains(m));
        println!(
            "  {} {}{}",
            style(format!("{:<9}", p.name)).cyan().bold(),
            p.description,
            if me {
                style(" (this machine)").magenta().to_string()
            } else {
                String::new()
            }
        );
        let macs = p.macs.iter().map(format_mac).collect::<Vec<_>>().join(", ");
        println!(
            "  {:9} {}",
            "",
            style(format!(
                "{macs} · {} {}:{} · also {}",
                p.check.1,
                p.ip,
                p.check.0,
                p.aliases.join(", ")
            ))
            .dim()
        );
    }
    println!(
        "\n  {} wake {} {}",
        style("Usage:").green().bold(),
        style("<TARGET>...").cyan(),
        style("(wake --help for more)").dim()
    );
}

fn status(quiet: bool) -> bool {
    let local = net::local_macs();
    let width = PRESETS.iter().map(|p| p.name.len()).max().unwrap_or(0);
    let ui = Ui::new(quiet, width);
    let up: Vec<bool> = thread::scope(|s| {
        let handles: Vec<_> = PRESETS
            .iter()
            .map(|p| {
                let ui = &ui;
                let local = &local;
                s.spawn(move || {
                    let addr = SocketAddrV4::new(p.ip, p.check.0);
                    if p.macs.iter().any(|m| local.contains(m)) {
                        ui.line(style(UP).green(), p.name, style("this machine").magenta());
                        return true;
                    }
                    let pb = ui.spinner(p.name, format!("checking {} on {addr}", p.check.1));
                    let ok = net::probe(addr, Duration::from_secs(2)).is_ok();
                    pb.finish_and_clear();
                    if ok {
                        ui.line(
                            style(UP).green(),
                            p.name,
                            format!(
                                "{} {}",
                                style("up").green(),
                                style(format!("· {} on {addr}", p.check.1)).dim()
                            ),
                        );
                    } else {
                        ui.line(
                            style(DOWN).dim(),
                            p.name,
                            format!(
                                "{} {}",
                                style("down").dim(),
                                style(format!("· no {} on {addr}", p.check.1)).dim()
                            ),
                        );
                    }
                    ok
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or(false))
            .collect()
    });
    up.into_iter().all(|u| u)
}

fn main() -> ExitCode {
    let _ = ctrlc::set_handler(|| {
        let _ = console::Term::stderr().show_cursor();
        std::process::exit(130);
    });
    let cli = Cli::parse();
    let result = match &cli.command {
        Some(Cmd::List) => {
            list();
            Ok(true)
        }
        Some(Cmd::Status) => Ok(status(cli.quiet)),
        Some(Cmd::SelfUpdate { check }) => update::run(*check, cli.quiet),
        None if cli.targets.is_empty() => {
            list();
            Ok(true)
        }
        None => wake(&cli),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("{} {e}", style("error:").red().bold());
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn names(args: &[&str], local: &[[u8; 6]]) -> (Vec<String>, Vec<String>) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let local: HashSet<_> = local.iter().copied().collect();
        let (t, s) = resolve(&args, &local).unwrap();
        (t.into_iter().map(|t| t.name).collect(), s)
    }

    #[test]
    fn all_skips_this_machine_and_dedupes() {
        let totoro = PRESETS[0].macs[0];
        assert_eq!(
            names(&["all", "pc", "totoro", "studio"], &[totoro]),
            (
                vec!["no-face".into(), "calcifer".into()],
                vec!["totoro".into()]
            )
        );
    }

    #[test]
    fn totoro_aliases_and_mac_resolve_to_one_canonical_target() {
        assert_eq!(
            names(&["pc", "Totoro", "windows", "74:56:3C:B4:F5:4E"], &[]),
            (vec!["totoro".into()], vec![])
        );
    }

    #[test]
    fn a_preset_mac_resolves_to_the_preset_and_others_stay_custom() {
        assert_eq!(
            names(&["9C-6B-00-45-2B-C1"], &[]).0,
            vec!["calcifer".to_string()]
        );
        assert_eq!(
            names(&["021122334455"], &[]).0,
            vec!["02:11:22:33:44:55".to_string()]
        );
    }

    #[test]
    fn unknown_target_lists_presets() {
        let err = resolve(&["toaster".into()], &HashSet::new()).err().unwrap();
        assert!(
            err.contains("presets: totoro, no-face, calcifer, all"),
            "{err}"
        );
    }

    #[test]
    fn cli_parses_optional_wait() {
        for args in [vec!["wake", "pc"], vec!["wake", "pc", "-w"]] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(!cli.no_wait);
            assert_eq!(cli.wait, Duration::from_secs(240));
        }
        let cli = Cli::try_parse_from(["wake", "-w", "30", "pc"]).unwrap();
        assert_eq!(cli.wait, Duration::from_secs(30));
        assert!(Cli::try_parse_from(["wake", "pc", "-w", "0"]).is_err());
        assert!(Cli::try_parse_from(["wake", "status"])
            .unwrap()
            .command
            .is_some());
    }

    #[test]
    fn cli_no_wait_conflicts_only_with_explicit_wait() {
        for flag in ["-W", "--no-wait"] {
            assert!(Cli::try_parse_from(["wake", flag, "pc"]).unwrap().no_wait);
            assert_eq!(
                Cli::try_parse_from(["wake", "pc", "--wait", "30", flag])
                    .err()
                    .unwrap()
                    .kind(),
                clap::error::ErrorKind::ArgumentConflict
            );
        }
    }
}
