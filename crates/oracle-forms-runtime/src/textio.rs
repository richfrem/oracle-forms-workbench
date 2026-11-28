//! `TEXT_IO` server-side file I/O on a per-session virtual file system.
//!
//! Files live in memory in `RuntimeObjects::files` (path -> bytes), so form code that writes
//! a file and reads or "shells out" to it works end to end without touching the real disk.
//! The host can pre-load input files and read written output (the state lists the files).
//! Paths are normalised into the sandbox: separators become `/`, a drive prefix and leading
//! separators are dropped, and any `..` segment is refused (`INVALID_PATH`).
//!
//! Supported: `FOPEN(path, 'r'|'w'|'a')` (a `b` suffix is ignored), `FCLOSE`, `FCLOSE_ALL`,
//! `IS_OPEN`, `GET_LINE(file, OUT buffer)`, `PUT`, `PUT_LINE`, `NEW_LINE(file [, n])`,
//! `PUTF(file, format [, arg1..arg5])` (`%s` and `\n`), `FFLUSH`. Errors raise the UTL_FILE
//! style exceptions `INVALID_PATH`, `INVALID_MODE`, `INVALID_FILEHANDLE`, `INVALID_OPERATION`;
//! reading past the end raises `NO_DATA_FOUND`. (Exception names are not verified against 14c.)
//! `CLIENT_TEXT_IO` (the user's PC) is a separate, still unsupported, WebUtil feature.
//!
//! Purpose: Module implementation (textio.rs).
//! Key Input Dependencies: crate::objects, crate::builtins
//! Index: OpenFile, normalize_path, dispatch, fopen, handle_of, put, get_line, format_putf

use crate::builtins::BuiltinResult;
use crate::session::FormSession;
use crate::value::OracleValue;

/// An open `TEXT_IO` file handle.
#[derive(Debug, Clone)]
pub struct OpenFile {
    pub path: String,
    pub readable: bool,
    pub writable: bool,
    /// Index of the next line to read.
    pub next_line: usize,
}

fn raise(name: &str, code: i64, msg: impl Into<String>) -> BuiltinResult {
    BuiltinResult::Raise { name: name.into(), code, message: msg.into() }
}

fn text(args: &[OracleValue], idx: usize) -> String {
    args.get(idx).map(|v| v.to_string_lossy()).unwrap_or_default()
}

/// Maps a client-style path into the sandbox key, or `None` when it escapes it.
pub fn normalize_path(raw: &str) -> Option<String> {
    let mut p = raw.trim().replace('\\', "/");
    if p.len() >= 2 && p.as_bytes()[1] == b':' && p.as_bytes()[0].is_ascii_alphabetic() {
        p = p[2..].to_string();
    }
    let mut parts = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => return None,
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

pub fn dispatch(name: &str, args: &[OracleValue], session: &mut FormSession) -> Option<BuiltinResult> {
    let op = name.strip_prefix("TEXT_IO.")?;
    Some(match op {
        "FOPEN" => fopen(args, session),
        "IS_OPEN" => BuiltinResult::Value(OracleValue::boolean(handle_of(session, args).is_ok())),
        "FCLOSE" => match handle_of(session, args) {
            Ok(h) => {
                session.objects.open_files.remove(&h);
                BuiltinResult::Void
            }
            Err(e) => e,
        },
        "FCLOSE_ALL" => {
            session.objects.open_files.clear();
            BuiltinResult::Void
        }
        "FFLUSH" => match handle_of(session, args) {
            Ok(_) => BuiltinResult::Void,
            Err(e) => e,
        },
        "PUT" | "PUT_LINE" | "NEW_LINE" | "PUTF" => put(op, args, session),
        "GET_LINE" => get_line(args, session),
        _ => return None,
    })
}

fn fopen(args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let Some(path) = normalize_path(&text(args, 0)) else {
        return raise("INVALID_PATH", -29280, "ORA-29280: invalid directory path");
    };
    let mode = text(args, 1).to_ascii_lowercase();
    let (readable, writable, create, truncate) = match mode.trim_end_matches('b') {
        "r" => (true, false, false, false),
        "w" => (false, true, true, true),
        "a" => (false, true, true, false),
        _ => return raise("INVALID_MODE", -29281, "ORA-29281: invalid mode"),
    };
    let exists = session.objects.files.contains_key(&path);
    if !exists && !create {
        return raise("INVALID_OPERATION", -29283, format!("ORA-29283: invalid file operation: {path} does not exist"));
    }
    if truncate || !exists {
        session.objects.files.insert(path.clone(), Vec::new());
    }
    let handle = session.objects.next_anonymous_name("TEXTIO");
    session.objects.open_files.insert(handle.clone(), OpenFile { path, readable, writable, next_line: 0 });
    BuiltinResult::Value(OracleValue::varchar2(handle))
}

/// The key of an open handle given as the first argument.
fn handle_of(session: &FormSession, args: &[OracleValue]) -> Result<String, BuiltinResult> {
    let h = text(args, 0);
    if session.objects.open_files.contains_key(&h) {
        Ok(h)
    } else {
        Err(raise("INVALID_FILEHANDLE", -29282, "ORA-29282: invalid file ID"))
    }
}

fn put(op: &str, args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let h = match handle_of(session, args) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let file = session.objects.open_files[&h].clone();
    if !file.writable {
        return raise("INVALID_OPERATION", -29283, "ORA-29283: invalid file operation: file is not open for writing");
    }
    let out = match op {
        "PUT" => text(args, 1),
        "PUT_LINE" => format!("{}\n", text(args, 1)),
        "NEW_LINE" => "\n".repeat(args.get(1).and_then(|v| v.as_i64()).filter(|n| *n > 0).unwrap_or(1) as usize),
        _ => format_putf(&text(args, 1), &args[2.min(args.len())..]),
    };
    session.objects.files.entry(file.path).or_default().extend_from_slice(out.as_bytes());
    BuiltinResult::Void
}

/// `PUTF`: each `%s` takes the next argument (up to five), `\n` is a newline, `\\` a backslash.
fn format_putf(format: &str, args: &[OracleValue]) -> String {
    let mut out = String::new();
    let mut next = 0;
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('%', Some('s')) => {
                chars.next();
                out.push_str(&args.get(next).map(|v| v.to_string_lossy()).unwrap_or_default());
                next += 1;
            }
            ('\\', Some('n')) => {
                chars.next();
                out.push('\n');
            }
            ('\\', Some('\\')) => {
                chars.next();
                out.push('\\');
            }
            _ => out.push(c),
        }
    }
    out
}

fn get_line(args: &[OracleValue], session: &mut FormSession) -> BuiltinResult {
    let h = match handle_of(session, args) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let file = session.objects.open_files[&h].clone();
    if !file.readable {
        return raise("INVALID_OPERATION", -29283, "ORA-29283: invalid file operation: file is not open for reading");
    }
    let content = String::from_utf8_lossy(session.objects.files.get(&file.path).map(Vec::as_slice).unwrap_or(&[])).to_string();
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    match lines.get(file.next_line) {
        Some(l) => {
            let line = l.trim_end_matches(['\n', '\r']).to_string();
            session.objects.open_files.get_mut(&h).expect("checked").next_line += 1;
            session.objects.pending_out = vec![OracleValue::varchar2(line)];
            BuiltinResult::Void
        }
        None => raise("NO_DATA_FOUND", 1403, "ORA-01403: no data found"),
    }
}
