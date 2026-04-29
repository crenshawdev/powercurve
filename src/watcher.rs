// SPDX-License-Identifier: GPL-3.0-only

use anyhow::Context;
use log::LevelFilter;
use powercurve_zbus::PowerCurveProxy;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::{
    signal::unix::{SignalKind, signal},
    time::sleep,
};

use glob::Pattern;
use regex::Regex;
use serde::Deserialize;

static RUNNING: AtomicBool = AtomicBool::new(true);

// -------------------------------------------------------------------
// Config types
// -------------------------------------------------------------------

#[derive(Deserialize)]
struct WatcherConfig {
    #[serde(default)]
    watcher: WatcherSettings,
    #[serde(default)]
    rule: Vec<RuleDef>,
}

#[derive(Deserialize)]
struct WatcherSettings {
    #[serde(default = "default_poll_interval")]
    poll_interval: u64,
    default_profile: Option<String>,
}

impl Default for WatcherSettings {
    fn default() -> Self {
        Self { poll_interval: default_poll_interval(), default_profile: None }
    }
}

fn default_poll_interval() -> u64 {
    5
}

#[derive(Deserialize)]
struct RuleDef {
    name: String,
    match_exe: Option<String>,
    match_cmd: Option<String>,
    profile: String,
}

// -------------------------------------------------------------------
// Compiled rules
// -------------------------------------------------------------------

enum Matcher {
    Exe(Pattern),
    Cmd(Regex),
}

struct CompiledRule {
    name: String,
    matcher: Matcher,
    profile: String,
}

/// Parse and validate the config, compiling all match patterns up front.
fn compile_rules(config: WatcherConfig) -> anyhow::Result<(WatcherSettings, Vec<CompiledRule>)> {
    let valid_profiles = ["quiet", "balanced", "performance"];

    if let Some(ref dp) = config.watcher.default_profile {
        let lower = dp.to_lowercase();
        if !valid_profiles.contains(&lower.as_str()) {
            anyhow::bail!(
                "invalid default_profile '{}', expected one of: quiet, balanced, performance",
                dp
            );
        }
    }

    let mut rules = Vec::with_capacity(config.rule.len());

    for def in config.rule {
        let lower = def.profile.to_lowercase();
        if !valid_profiles.contains(&lower.as_str()) {
            anyhow::bail!(
                "rule '{}': invalid profile '{}', expected one of: quiet, balanced, performance",
                def.name,
                def.profile
            );
        }

        let matcher = match (def.match_exe, def.match_cmd) {
            (Some(pat), None) => {
                let compiled = Pattern::new(&pat)
                    .with_context(|| format!("rule '{}': invalid glob '{}'", def.name, pat))?;
                Matcher::Exe(compiled)
            }
            (None, Some(pat)) => {
                let compiled = Regex::new(&pat)
                    .with_context(|| format!("rule '{}': invalid regex '{}'", def.name, pat))?;
                Matcher::Cmd(compiled)
            }
            (Some(_), Some(_)) => {
                anyhow::bail!("rule '{}': specify match_exe or match_cmd, not both", def.name);
            }
            (None, None) => {
                anyhow::bail!("rule '{}': must specify match_exe or match_cmd", def.name);
            }
        };

        rules.push(CompiledRule { name: def.name, matcher, profile: lower });
    }

    Ok((config.watcher, rules))
}

// -------------------------------------------------------------------
// Process scanning
// -------------------------------------------------------------------

struct ProcessInfo {
    exe_name: String,
    cmdline: String,
}

/// Scan /proc for running processes, returning each PID's comm and cmdline.
///
/// Silently skips entries that can't be read (kernel threads, permission
/// denied, processes that exited between readdir and open).
fn scan_processes() -> Vec<ProcessInfo> {
    let entries = match fs::read_dir("/proc") {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let name_str = name.to_str()?;

            // Only numeric directory names are PIDs.
            if !name_str.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }

            let base = entry.path();
            let comm = fs::read_to_string(base.join("comm")).ok()?;
            let cmdline = fs::read(base.join("cmdline"))
                .ok()
                .map(|bytes| {
                    bytes.iter().map(|&b| if b == 0 { b' ' } else { b }).collect::<Vec<u8>>()
                })
                .and_then(|v| String::from_utf8(v).ok())
                .unwrap_or_default();

            Some(ProcessInfo {
                exe_name: comm.trim().to_string(),
                cmdline: cmdline.trim().to_string(),
            })
        })
        .collect()
}

// -------------------------------------------------------------------
// Rule evaluation
// -------------------------------------------------------------------

/// Evaluate rules against the process list. First matching rule wins.
fn evaluate_rules<'a>(rules: &'a [CompiledRule], processes: &[ProcessInfo]) -> Option<&'a str> {
    rules.iter().find_map(|rule| {
        let matched = processes.iter().any(|proc| match &rule.matcher {
            Matcher::Exe(pat) => pat.matches(&proc.exe_name),
            Matcher::Cmd(re) => re.is_match(&proc.cmdline),
        });

        if matched {
            log::debug!("rule '{}' matched, target profile: {}", rule.name, rule.profile);
            Some(rule.profile.as_str())
        } else {
            None
        }
    })
}

// -------------------------------------------------------------------
// D-Bus helpers
// -------------------------------------------------------------------

/// Set the power profile via D-Bus. Returns Ok(true) if the call
/// succeeded, Ok(false) if it was a no-op (already set), or Err on
/// D-Bus failure.
async fn set_profile(client: &PowerCurveProxy<'_>, profile: &str) -> anyhow::Result<()> {
    match profile {
        "quiet" => client.quiet().await?,
        "balanced" => client.balanced().await?,
        "performance" => client.performance().await?,
        _ => anyhow::bail!("unknown profile '{}'", profile),
    }
    Ok(())
}

// -------------------------------------------------------------------
// Config path
// -------------------------------------------------------------------

/// Resolve the watcher config path, respecting XDG_CONFIG_HOME.
fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("powercurve").join("watcher.toml"))
}

// -------------------------------------------------------------------
// Entry point
// -------------------------------------------------------------------

/// Watch running processes and auto-switch power profiles based on
/// user-defined rules in ~/.config/powercurve/watcher.toml.
#[tokio::main(flavor = "current_thread")]
pub async fn run() -> anyhow::Result<()> {
    // Set up logging so info/debug messages reach the terminal.
    crate::logging::setup(LevelFilter::Info).ok();

    // Ignore SIGHUP so daemon reload signals don't kill us.
    // SAFETY: SIG_IGN is documented by POSIX as a safe disposition for any
    // signal. We install it once at startup before any other thread exists,
    // so there is no concurrent signal-handler mutation to race against.
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    // Load config. Missing file is fine, we just idle.
    let path = config_path();
    let (settings, rules) = match path {
        Some(ref p) if p.exists() => {
            let text =
                fs::read_to_string(p).with_context(|| format!("failed to read {}", p.display()))?;
            let config: WatcherConfig = toml::from_str(&text)
                .with_context(|| format!("failed to parse {}", p.display()))?;
            compile_rules(config)?
        }
        _ => {
            log::info!(
                "no watcher config found, idling (create ~/.config/powercurve/watcher.toml to add rules)"
            );
            (WatcherSettings::default(), Vec::new())
        }
    };

    let interval = Duration::from_secs(settings.poll_interval.max(1));

    let connection = zbus::Connection::system().await.context("failed to connect to system bus")?;

    let client = PowerCurveProxy::new(&connection).await.context("failed to create D-Bus proxy")?;

    // Seed last_set with the current daemon profile so we don't
    // redundantly re-apply on first tick.
    let mut last_set: Option<String> = client.get_profile().await.ok().map(|p| p.to_lowercase());

    if rules.is_empty() {
        log::info!("no rules defined, watcher will idle");
    } else {
        log::info!("loaded {} rule(s), polling every {}s", rules.len(), interval.as_secs());
    }

    println!("watching processes (ctrl-c to stop)");

    loop {
        tokio::select! {
            _ = sigint.recv() => break,
            _ = sigterm.recv() => break,
            _ = sleep(interval) => {}
        }

        if !RUNNING.load(Ordering::SeqCst) {
            break;
        }

        if rules.is_empty() {
            continue;
        }

        let processes = scan_processes();
        let target = evaluate_rules(&rules, &processes);

        let desired = target
            .map(String::from)
            .or_else(|| settings.default_profile.as_ref().map(|s| s.to_lowercase()));

        if let Some(profile) = &desired {
            let needs_switch = last_set.as_deref() != Some(profile.as_str());

            if needs_switch {
                if let Some(ref matched_rule) = target {
                    log::info!("rule matched '{}', switching to {}", matched_rule, profile);
                } else {
                    log::info!("no rules match, restoring default profile {}", profile);
                }

                match set_profile(&client, profile).await {
                    Ok(()) => {
                        println!("profile: {}", profile);
                        last_set = Some(profile.clone());
                    }
                    Err(e) => {
                        log::warn!("failed to set profile: {}", e);
                        // Reset so we retry next cycle when the daemon comes back.
                        last_set = None;
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn compile_valid_exe_rule() {
        let config = WatcherConfig {
            watcher: WatcherSettings::default(),
            rule: vec![RuleDef {
                name: "gaming".into(),
                match_exe: Some("steam_app_*".into()),
                match_cmd: None,
                profile: "performance".into(),
            }],
        };
        let (_, rules) = compile_rules(config).expect("should compile");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "gaming");
    }

    #[test]
    fn compile_valid_cmd_rule() {
        let config = WatcherConfig {
            watcher: WatcherSettings::default(),
            rule: vec![RuleDef {
                name: "encoding".into(),
                match_exe: None,
                match_cmd: Some("ffmpeg.*x265".into()),
                profile: "performance".into(),
            }],
        };
        let (_, rules) = compile_rules(config).expect("should compile");
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn reject_both_matchers() {
        let config = WatcherConfig {
            watcher: WatcherSettings::default(),
            rule: vec![RuleDef {
                name: "bad".into(),
                match_exe: Some("foo".into()),
                match_cmd: Some("bar".into()),
                profile: "balanced".into(),
            }],
        };
        assert!(compile_rules(config).is_err());
    }

    #[test]
    fn reject_no_matcher() {
        let config = WatcherConfig {
            watcher: WatcherSettings::default(),
            rule: vec![RuleDef {
                name: "bad".into(),
                match_exe: None,
                match_cmd: None,
                profile: "balanced".into(),
            }],
        };
        assert!(compile_rules(config).is_err());
    }

    #[test]
    fn reject_invalid_profile() {
        let config = WatcherConfig {
            watcher: WatcherSettings::default(),
            rule: vec![RuleDef {
                name: "bad".into(),
                match_exe: Some("foo".into()),
                match_cmd: None,
                profile: "turbo".into(),
            }],
        };
        assert!(compile_rules(config).is_err());
    }

    #[test]
    fn reject_invalid_default_profile() {
        let config = WatcherConfig {
            watcher: WatcherSettings { poll_interval: 5, default_profile: Some("ultra".into()) },
            rule: vec![],
        };
        assert!(compile_rules(config).is_err());
    }

    #[test]
    fn evaluate_first_match_wins() {
        let (_, rules) = compile_rules(WatcherConfig {
            watcher: WatcherSettings::default(),
            rule: vec![
                RuleDef {
                    name: "first".into(),
                    match_exe: Some("gameA".into()),
                    match_cmd: None,
                    profile: "performance".into(),
                },
                RuleDef {
                    name: "second".into(),
                    match_exe: Some("gameA".into()),
                    match_cmd: None,
                    profile: "quiet".into(),
                },
            ],
        })
        .expect("should compile");

        let procs =
            vec![ProcessInfo { exe_name: "gameA".into(), cmdline: "gameA --fullscreen".into() }];

        assert_eq!(evaluate_rules(&rules, &procs), Some("performance"));
    }

    #[test]
    fn evaluate_no_match_returns_none() {
        let (_, rules) = compile_rules(WatcherConfig {
            watcher: WatcherSettings::default(),
            rule: vec![RuleDef {
                name: "gaming".into(),
                match_exe: Some("gameA".into()),
                match_cmd: None,
                profile: "performance".into(),
            }],
        })
        .expect("should compile");

        let procs = vec![ProcessInfo { exe_name: "firefox".into(), cmdline: "firefox".into() }];

        assert_eq!(evaluate_rules(&rules, &procs), None);
    }

    #[test]
    fn evaluate_glob_pattern() {
        let (_, rules) = compile_rules(WatcherConfig {
            watcher: WatcherSettings::default(),
            rule: vec![RuleDef {
                name: "steam".into(),
                match_exe: Some("steam_app_*".into()),
                match_cmd: None,
                profile: "performance".into(),
            }],
        })
        .expect("should compile");

        let procs = vec![ProcessInfo {
            exe_name: "steam_app_123456".into(),
            cmdline: "steam_app_123456".into(),
        }];

        assert_eq!(evaluate_rules(&rules, &procs), Some("performance"));
    }

    #[test]
    fn evaluate_regex_on_cmdline() {
        let (_, rules) = compile_rules(WatcherConfig {
            watcher: WatcherSettings::default(),
            rule: vec![RuleDef {
                name: "encode".into(),
                match_exe: None,
                match_cmd: Some("ffmpeg.*x265".into()),
                profile: "performance".into(),
            }],
        })
        .expect("should compile");

        let procs = vec![ProcessInfo {
            exe_name: "ffmpeg".into(),
            cmdline: "ffmpeg -i input.mkv -c:v libx265 output.mkv".into(),
        }];

        assert_eq!(evaluate_rules(&rules, &procs), Some("performance"));
    }

    #[test]
    fn poll_interval_floor() {
        let settings = WatcherSettings { poll_interval: 0, default_profile: None };
        let clamped = settings.poll_interval.max(1);
        assert_eq!(clamped, 1);
    }
}
