//! Server-side policy for the `HOST` built-in, read from `OFW_HOST_POLICY`:
//! * unset / `deny`: `HOST` fails visibly (default);
//! * `emulate`: the in-runtime shell emulator over the session's virtual files;
//! * `allow:prog1,prog2`: real execution of exactly those programs (no shell, no path
//!   separators in the program name, 10 s timeout, output capped), in the system temp dir.
//!
//! Purpose: Module implementation (host_exec.rs).
//! Key Input Dependencies: OFW_HOST_POLICY environment variable
//! Index: policy_from_setting, policy_from_env, split_args, run_allowed

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use oracle_forms_runtime::HostPolicy;

const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_OUTPUT: usize = 64 * 1024;

pub(crate) fn policy_from_env() -> HostPolicy {
    policy_from_setting(std::env::var("OFW_HOST_POLICY").ok().as_deref())
}

pub(crate) fn policy_from_setting(setting: Option<&str>) -> HostPolicy {
    let s = setting.unwrap_or("").trim();
    if s.eq_ignore_ascii_case("emulate") {
        return HostPolicy::Emulate;
    }
    if let Some(list) = s.strip_prefix("allow:").or_else(|| s.strip_prefix("ALLOW:")) {
        let allowed: Vec<String> = list.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty() && !p.contains(['/', '\\'])).collect();
        if !allowed.is_empty() {
            return HostPolicy::Execute(Arc::new(move |cmd: &str| run_allowed(&allowed, cmd)));
        }
    }
    HostPolicy::Deny
}

/// Splits a command line on spaces, keeping quoted parts together.
fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in line.chars() {
        match (c, quote) {
            ('"' | '\'', None) => quote = Some(c),
            (q, Some(open)) if q == open => quote = None,
            (c, None) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            (c, _) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn run_allowed(allowed: &[String], line: &str) -> Result<String, String> {
    let args = split_args(line);
    let Some(prog) = args.first() else { return Err("empty command".into()) };
    if !allowed.iter().any(|a| a == prog) {
        return Err(format!("program '{prog}' is not in the OFW_HOST_POLICY allowlist"));
    }
    let mut child = Command::new(prog)
        .args(&args[1..])
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot start '{prog}': {e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => {
                let mut out = Vec::new();
                if let Some(so) = child.stdout.take() {
                    let _ = so.take(MAX_OUTPUT as u64).read_to_end(&mut out);
                }
                let text = String::from_utf8_lossy(&out).to_string();
                return if status.success() { Ok(text) } else { Err(format!("'{prog}' exited with {status}")) };
            }
            None if start.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("'{prog}' did not finish within {} s and was stopped", TIMEOUT.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_map_to_policies() {
        assert!(matches!(policy_from_setting(None), HostPolicy::Deny));
        assert!(matches!(policy_from_setting(Some("deny")), HostPolicy::Deny));
        assert!(matches!(policy_from_setting(Some("emulate")), HostPolicy::Emulate));
        assert!(matches!(policy_from_setting(Some("allow:")), HostPolicy::Deny), "an empty allowlist allows nothing");
        assert!(matches!(policy_from_setting(Some("allow:/bin/sh")), HostPolicy::Deny), "paths are not program names");
        assert!(matches!(policy_from_setting(Some("allow:echo,true")), HostPolicy::Execute(_)));
    }

    #[test]
    fn allowlist_runs_only_listed_programs_without_a_shell() {
        let allowed = vec!["echo".to_string()];
        assert_eq!(run_allowed(&allowed, "echo \"hello world\"").unwrap().trim(), "hello world");
        assert!(run_allowed(&allowed, "ls").unwrap_err().contains("allowlist"));
        assert!(run_allowed(&allowed, "echo a; ls").unwrap().contains("; ls"), "no shell: metacharacters stay plain arguments");
        assert!(run_allowed(&allowed, "").is_err());
    }
}
