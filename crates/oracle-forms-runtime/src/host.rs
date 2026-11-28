//! `HOST` (run an operating-system command) under an explicit policy.
//!
//! Forms runs `HOST` on the application server. The workbench never runs arbitrary
//! commands by default:
//! * [`HostPolicy::Deny`] (default): the call fails visibly (`FORM_FAILURE`, message).
//! * [`HostPolicy::Emulate`]: a small shell emulator over the session's virtual file system
//!   (the same files `TEXT_IO` uses): `echo TEXT [> file | >> file]`, `touch`, `type`/`cat`,
//!   `del`/`rm`, `copy`/`cp`, `move`/`mv`, `mkdir`, `dir`/`ls`, chained with `&&`, `&` or `;`.
//!   Unknown commands fail visibly and say which one.
//! * [`HostPolicy::Execute`]: the host supplies a callback (the server builds one from an
//!   allowlist of program names, runs them without a shell, with a timeout).
//!
//! Purpose: Module implementation (host.rs).
//! Key Input Dependencies: crate::textio, crate::objects
//! Index: HostPolicy, HostExec, dispatch, run_emulated, run_one, split_commands, redirect

use crate::builtins::BuiltinResult;
use crate::session::FormSession;
use crate::textio::normalize_path;
use crate::value::OracleValue;
use std::sync::Arc;

/// Host-provided command runner: the command line in, the output (or an error) back.
pub type HostExec = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

#[derive(Clone, Default)]
pub enum HostPolicy {
    #[default]
    Deny,
    Emulate,
    Execute(HostExec),
}

impl std::fmt::Debug for HostPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            HostPolicy::Deny => "Deny",
            HostPolicy::Emulate => "Emulate",
            HostPolicy::Execute(_) => "Execute",
        })
    }
}

pub fn dispatch(name: &str, args: &[OracleValue], session: &mut FormSession) -> Option<BuiltinResult> {
    if name != "HOST" {
        return None;
    }
    let command = args.first().map(|v| v.to_string_lossy()).unwrap_or_default();
    if command.trim().is_empty() {
        return Some(BuiltinResult::Failure("HOST requires a command".into()));
    }
    session.trace_log.push(format!("HOST: {command}"));
    Some(match session.host_policy.clone() {
        HostPolicy::Deny => BuiltinResult::Failure("HOST is disabled by policy (set OFW_HOST_POLICY=emulate or allow:<programs> on the server to enable it)".into()),
        HostPolicy::Emulate => match run_emulated(session, &command) {
            Ok(()) => BuiltinResult::Void,
            Err(e) => BuiltinResult::Failure(format!("HOST: {e}")),
        },
        HostPolicy::Execute(exec) => match exec(&command) {
            Ok(out) => {
                if !out.trim().is_empty() {
                    session.trace_log.push(format!("HOST output: {}", out.trim()));
                }
                BuiltinResult::Void
            }
            Err(e) => BuiltinResult::Failure(format!("HOST: {e}")),
        },
    })
}

/// Splits on `&&`, `&` and `;` outside quotes.
fn split_commands(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in line.chars() {
        match (c, quote) {
            ('"' | '\'', None) => {
                quote = Some(c);
                cur.push(c);
            }
            (q, Some(open)) if q == open => {
                quote = None;
                cur.push(c);
            }
            ('&' | ';', None) => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn run_emulated(session: &mut FormSession, line: &str) -> Result<(), String> {
    for cmd in split_commands(line) {
        run_one(session, &cmd)?;
    }
    Ok(())
}

/// `text > file` / `text >> file`: (text, file, append).
fn redirect(rest: &str) -> (String, Option<(String, bool)>) {
    if let Some((l, r)) = rest.split_once(">>") {
        return (l.trim().to_string(), Some((r.trim().to_string(), true)));
    }
    if let Some((l, r)) = rest.split_once('>') {
        return (l.trim().to_string(), Some((r.trim().to_string(), false)));
    }
    (rest.trim().to_string(), None)
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    if t.len() >= 2 && ((t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\''))) {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

fn path_of(raw: &str) -> Result<String, String> {
    normalize_path(&unquote(raw)).ok_or_else(|| format!("path '{raw}' is outside the sandbox"))
}

fn run_one(session: &mut FormSession, cmd: &str) -> Result<(), String> {
    let (prog, rest) = cmd.split_once(char::is_whitespace).map(|(p, r)| (p, r.trim())).unwrap_or((cmd, ""));
    let prog = prog.to_ascii_lowercase();
    match prog.as_str() {
        "echo" => {
            let (text, target) = redirect(rest);
            let text = unquote(&text);
            match target {
                Some((file, append)) => {
                    let key = path_of(&file)?;
                    let entry = session.objects.files.entry(key).or_default();
                    if !append {
                        entry.clear();
                    }
                    entry.extend_from_slice(text.as_bytes());
                    entry.extend_from_slice(b"\n");
                }
                None => session.trace_log.push(format!("HOST output: {text}")),
            }
            Ok(())
        }
        "touch" => {
            let key = path_of(rest)?;
            session.objects.files.entry(key).or_default();
            Ok(())
        }
        "type" | "cat" => {
            let key = path_of(rest)?;
            let content = session.objects.files.get(&key).ok_or_else(|| format!("{rest}: no such file"))?;
            let text = String::from_utf8_lossy(content).to_string();
            session.trace_log.push(format!("HOST output: {}", text.trim_end()));
            Ok(())
        }
        "del" | "rm" => {
            let key = path_of(rest.trim_start_matches("-f ").trim_start_matches("/q "))?;
            session.objects.files.remove(&key).map(|_| ()).ok_or_else(|| format!("{rest}: no such file"))
        }
        "copy" | "cp" | "move" | "mv" => {
            let mut parts = rest.split_whitespace();
            let (Some(from), Some(to)) = (parts.next(), parts.next()) else {
                return Err(format!("{prog} needs a source and a destination"));
            };
            let (from, to) = (path_of(from)?, path_of(to)?);
            let content = session.objects.files.get(&from).cloned().ok_or_else(|| format!("{from}: no such file"))?;
            session.objects.files.insert(to, content);
            if prog == "move" || prog == "mv" {
                session.objects.files.remove(&from);
            }
            Ok(())
        }
        "mkdir" | "md" => path_of(rest).map(|_| ()),
        "dir" | "ls" => {
            let names: Vec<&String> = session.objects.files.keys().collect();
            let listing = names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
            session.trace_log.push(format!("HOST output: {listing}"));
            Ok(())
        }
        other => Err(format!("the emulator does not know the command '{other}' (policy: emulate)")),
    }
}
