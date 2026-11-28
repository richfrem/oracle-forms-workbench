//! Optional Track 1 bridge. Native parsing and runtime never depend on this toolchain.
//!
//! Purpose: Optional Oracle 14c container conversion, compilation, and FSAL bridge.
//! Key Input Dependencies: tokio
//! Index: Job, jobs, python_program, setting, expand_home, oracle_home, forms_container, work_dir, fsal_jar, wsl_distro, podman_command, validate_compile_target, check_build_mount, start, normalize_fmx_name, deployed_fmx_dirs, find_deployed_fmx, list_deployed_fmx, parse_launch_params, launch_precompiled, status, readiness, command, podman_output, container (+6 more)

use std::{collections::HashMap, fs, path::PathBuf, process::{Child, Command, Stdio}, sync::{Mutex, OnceLock, atomic::{AtomicU64, Ordering}}};

struct Job { child: Child, log: PathBuf }
static JOBS: OnceLock<Mutex<HashMap<String, Job>>> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(1);
fn jobs() -> &'static Mutex<HashMap<String, Job>> { JOBS.get_or_init(|| Mutex::new(HashMap::new())) }

/// Interpreter for the Python launcher: `python` on Windows (there is no `python3` command), `python3` elsewhere.
fn python_program() -> &'static str { if cfg!(windows) { "python" } else { "python3" } }

/// A setting from the environment, then the git-ignored `.env` at the repo root.
fn setting(root: &std::path::Path, name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty()).or_else(|| {
        fs::read_to_string(root.join(".env")).ok().and_then(|text| {
            text.lines().find_map(|line| {
                let val = line.trim().strip_prefix(name)?.strip_prefix('=')?.trim().trim_matches('"').trim_matches('\'');
                (!val.is_empty()).then(|| val.to_string())
            })
        })
    })
}

fn expand_home(value: String) -> PathBuf {
    match value.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME").map(|home| PathBuf::from(home).join(rest)).unwrap_or_else(|| PathBuf::from(value)),
        None => PathBuf::from(value),
    }
}

/// Oracle Home folder on the host: `OFW_ORACLE_HOME` (env or `.env`), falling back to the repo default.
fn oracle_home(root: &std::path::Path) -> PathBuf {
    setting(root, "OFW_ORACLE_HOME").map(expand_home).unwrap_or_else(|| root.join("oracle-reference-stack/oracle_home_14c"))
}

fn forms_container(root: &std::path::Path) -> String {
    setting(root, "OFW_FORMS_CONTAINER").unwrap_or_else(|| "forms-server-14c".into())
}

/// Folder for staged Oracle preview jobs: `OFW_WORK_DIR`, else `<Oracle Home>/ofw-work`. Never inside the repository.
pub(crate) fn work_dir(root: &std::path::Path) -> PathBuf {
    setting(root, "OFW_WORK_DIR").map(expand_home).unwrap_or_else(|| oracle_home(root).join("ofw-work"))
}

/// Resolves `frmsal.jar` from the Oracle Home.
fn fsal_jar(root: &std::path::Path) -> PathBuf {
    oracle_home(root).join("forms/java/frmsal.jar")
}

/// WSL distribution that runs Podman on Windows. Override with OFW_WSL_DISTRO.
fn wsl_distro() -> String {
    std::env::var("OFW_WSL_DISTRO").ok().filter(|name| !name.is_empty()).unwrap_or_else(|| "Ubuntu".into())
}

/// Builds a Podman call. On Windows, Podman runs inside WSL, so the call goes through `wsl`.
fn podman_command(args: &[&str]) -> tokio::process::Command {
    if cfg!(windows) {
        let quoted: Vec<String> = args.iter().map(|arg| format!("'{}'", arg.replace('\'', "'\\''"))).collect();
        let script = format!("export XDG_RUNTIME_DIR=/run/user/$(id -u); podman {}", quoted.join(" "));
        let mut command = tokio::process::Command::new("wsl");
        command.arg("-d").arg(wsl_distro()).args(["--", "bash", "-lc"]).arg(script);
        command
    } else {
        let mut command = tokio::process::Command::new("podman");
        command.args(args);
        command
    }
}

/// Validates the Oracle compile target (TNS alias or `host:port/service`), defaulting to `FREEPDB1`.
pub(crate) fn validate_compile_target(target: Option<&str>) -> Result<String, String> {
    let trimmed = target.unwrap_or("").trim();
    if trimmed.is_empty() {
        return Ok("FREEPDB1".into());
    }
    if trimmed.len() > 256
        || !trimmed.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':' | b'/'))
    {
        return Err("Invalid target database. Use a TNS alias (e.g. FREEPDB1) or host:port/service.".into());
    }
    Ok(trimmed.into())
}

/// Reject a stale bind mount before Build & Compile saves the form.
pub(crate) async fn check_build_mount() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let container = forms_container(&root);
    let home = oracle_home(&root).canonicalize().map_err(|error| {
        format!("Cannot open configured OFW_ORACLE_HOME: {error}")
    })?;
    let mut command = podman_command(&[
        "inspect", "--format", "{{range .Mounts}}{{if eq .Destination \"/oracle_home_14c\"}}{{.Source}}{{end}}{{end}}",
        &container,
    ]);
    command.kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(10), command.output())
        .await.map_err(|_| "Timed out checking the Forms container mount".to_string())?
        .map_err(|error| format!("Cannot check the Forms container mount: {error}"))?;
    if !output.status.success() {
        return Err(format!("Cannot inspect {container}. Check the Podman testbed setup."));
    }
    let source = String::from_utf8_lossy(&output.stdout);
    let mounted = std::path::Path::new(source.trim());
    if mounted == home || mounted.canonicalize().is_ok_and(|path| path == home) {
        return Ok(());
    }
    Err(format!("Forms container mount mismatch: /oracle_home_14c is mounted from {}, but OFW_ORACLE_HOME is {}. Recreate {container} with the configured Oracle Home before building.", source.trim(), home.display()))
}

pub fn start(
    saved_xml: &std::path::Path,
    filename: &str,
    credentials: Option<(String, String)>,
    target_db: Option<&str>,
    compile_only: bool,
) -> Result<String, String> {
    if !filename.ends_with(".xml") || !filename.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.')) {
        return Err("Invalid saved form filename".into());
    }
    let compile_target = if credentials.is_some() {
        Some(validate_compile_target(target_db)?)
    } else {
        None
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().map_err(|e| e.to_string())?;
    let script = root.join("plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/run_form.py");
    if !fsal_jar(&root).is_file() {
        return Err("FSAL client is missing. Set up the Oracle 14c reference environment first.".into());
    }
    let mut jobs = jobs().lock().unwrap();
    let mut active = 0;
    jobs.retain(|_, job| match job.child.try_wait() { Ok(None) => { active += 1; true }, _ => true });
    if active >= 4 { return Err("Close an existing Oracle preview before opening another".into()); }
    if jobs.len() >= 32 {
        let completed: Vec<_> = jobs.iter_mut().filter_map(|(id, job)| job.child.try_wait().ok().flatten().map(|_| id.clone())).collect();
        for id in completed { if let Some(job) = jobs.remove(&id) { let _ = fs::remove_file(job.log); } }
    }
    let id = format!("{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    // Oracle converters may choose output names from the internal module name.
    // Stage in a job directory in the work folder (outside the repository) so conversion can never replace a source FMB.
    let staging = work_dir(&root).join("oracle-preview").join(&id);
    if fs::create_dir_all(&staging).is_err() {
        // The Oracle Home belongs to the container user; the compile script creates the work folder through Podman.
        let init = Command::new(python_program()).arg(&script).arg("--init-work-dir").current_dir(&root).output().map_err(|e| e.to_string())?;
        if !init.status.success() {
            return Err(format!("Cannot create the work folder: {}", String::from_utf8_lossy(&init.stderr).trim()));
        }
        fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    let staged_xml = staging.join(filename);
    fs::copy(saved_xml, &staged_xml).map_err(|e| e.to_string())?;
    let log = std::env::temp_dir().join(format!("ofw-oracle-preview-{}.log", id));
    let output = fs::OpenOptions::new().write(true).create_new(true).open(&log).map_err(|e| e.to_string())?;
    let stderr = output.try_clone().map_err(|e| e.to_string())?;
    let mut command = Command::new(python_program());
    // Windows only: without this, each launch opens its own console window when the server has no console.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command.arg("-u").arg(script).arg(staged_xml).args([if compile_only { "--no-launch" } else { "--interactive" }, "--userid", "none"]);
    if let Some(app_name) = setting(&root, "OFW_APP_NAME") {
        command.args(["--app-name", &app_name]);
    }
    if let Some(forms_config) = setting(&root, "OFW_FORMS_CONFIG") {
        command.args(["--config", &forms_config]);
    }
    if let Some((username, password)) = credentials {
        let target = compile_target.unwrap_or_else(|| "FREEPDB1".into());
        command.arg("--compile-from-env").env("OFW_PREVIEW_DB_LOGON", format!("{}/{}@{}", username, password, target));
    } else { command.arg("--offline"); }
    let child = command.current_dir(&root).stdin(Stdio::null()).stdout(output).stderr(stderr).spawn().map_err(|e| e.to_string())?;
    jobs.insert(id.clone(), Job { child, log });
    Ok(id)
}

/// Validates a plain module or `.fmx` filename (no path separators or traversal).
pub(crate) fn normalize_fmx_name(module_name: &str) -> Result<String, String> {
    let trimmed = module_name.trim();
    if trimmed.is_empty()
        || !trimmed.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
        || trimmed.contains("..")
    {
        return Err("Invalid form module name".into());
    }
    let stem = trimmed.strip_suffix(".fmx").or_else(|| trimmed.strip_suffix(".FMX")).unwrap_or(trimmed);
    if stem.is_empty() || stem.contains('.') {
        return Err("Invalid form module name".into());
    }
    Ok(format!("{}.fmx", stem))
}

/// Resolves deployed `.fmx` search directories from `.env` / environment
/// (`OFW_FORMS_APPS_DIR`, `OFW_APP_NAME`, and `OFW_ORACLE_HOME/forms`).
pub(crate) fn deployed_fmx_dirs(root: &std::path::Path) -> Vec<PathBuf> {
    let home = oracle_home(root);
    let apps_base = setting(root, "OFW_FORMS_APPS_DIR")
        .map(expand_home)
        .unwrap_or_else(|| home.join("forms_apps"));
    let mut dirs = Vec::new();
    if let Some(app_name) = setting(root, "OFW_APP_NAME") {
        dirs.push(apps_base.join(app_name));
    } else if apps_base.is_dir() {
        dirs.push(apps_base.clone());
        if let Ok(entries) = fs::read_dir(&apps_base) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    dirs.push(p);
                }
            }
        }
    }
    dirs.push(home.join("forms"));
    dirs
}

/// Locates an already-compiled `.fmx` in `OFW_FORMS_APPS_DIR` / `<Oracle Home>/forms_apps/<OFW_APP_NAME>/`
/// or `<Oracle Home>/forms/`. Checks both exact casing and lowercase on case-sensitive filesystems.
pub(crate) fn find_deployed_fmx(root: &std::path::Path, module_name: &str) -> Result<(PathBuf, String), String> {
    let fmx = normalize_fmx_name(module_name)?;
    let fmx_lower = fmx.to_ascii_lowercase();
    let search_dirs = deployed_fmx_dirs(root);
    for dir in &search_dirs {
        for candidate in [&fmx, &fmx_lower] {
            let p = dir.join(candidate);
            if p.is_file() {
                return Ok((p, candidate.clone()));
            }
        }
    }
    let searched: Vec<String> = search_dirs.iter().map(|d| d.display().to_string()).collect();
    Err(format!("Compiled {} not found in {}. Compile the form first or select a compilation mode.", fmx, searched.join(" or ")))
}

/// Lists already-compiled `.fmx` files in `OFW_FORMS_APPS_DIR` / `<Oracle Home>/forms_apps/<OFW_APP_NAME>/`
/// and `<Oracle Home>/forms/`, plus configured `OFW_FORMS_CONFIG`, `OFW_APP_NAME`, and `searchDirs`.
pub fn list_deployed_fmx() -> serde_json::Value {
    let root = match PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize() {
        Ok(r) => r,
        Err(_) => return serde_json::json!({ "forms": [], "formsConfig": null, "appName": null, "searchDirs": [] }),
    };
    let app_name = setting(&root, "OFW_APP_NAME");
    let forms_config = setting(&root, "OFW_FORMS_CONFIG");
    let dirs = deployed_fmx_dirs(&root);
    let mut seen = std::collections::BTreeSet::new();
    let mut active_dirs = Vec::new();
    for dir in dirs {
        if dir.is_dir() {
            active_dirs.push(dir.display().to_string());
        }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                        if name.to_ascii_lowercase().ends_with(".fmx") {
                            seen.insert(name.to_string());
                        }
                    }
                }
            }
        }
    }
    let forms: Vec<String> = seen.into_iter().collect();
    serde_json::json!({
        "forms": forms,
        "formsConfig": forms_config,
        "appName": app_name,
        "searchDirs": active_dirs
    })
}

/// Validates space- or `&`-separated `NAME=VALUE` form parameters for `launch_fsal.py --param`.
pub(crate) fn parse_launch_params(raw: Option<&str>) -> Result<Vec<String>, String> {
    let text = raw.unwrap_or("").trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for token in text.split(|c: char| c.is_ascii_whitespace() || c == '&') {
        let tok = token.trim();
        if tok.is_empty() {
            continue;
        }
        let (k, v) = tok.split_once('=').ok_or_else(|| format!("Invalid parameter '{}'. Expected NAME=VALUE.", tok))?;
        if k.is_empty()
            || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || !v.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/' | b':' | b'@'))
        {
            return Err(format!("Invalid characters in parameter '{}'.", tok));
        }
        out.push(format!("{}={}", k, v));
    }
    Ok(out)
}

/// Launches an already-compiled `.fmx` directly via `launch_fsal.py` without running `XML2Forms` or `frmcmp_batch`.
pub fn launch_precompiled(module_name: &str, forms_config: Option<&str>, launch_params: Option<&str>) -> Result<(String, String), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().map_err(|e| e.to_string())?;
    let script = root.join("plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/launch_fsal.py");
    if !fsal_jar(&root).is_file() {
        return Err("FSAL client is missing. Set up the Oracle 14c reference environment first.".into());
    }
    if !script.is_file() {
        return Err("FSAL launcher script is missing from this checkout.".into());
    }
    let (_fmx_path, fmx_name) = find_deployed_fmx(&root, module_name)?;
    let cfg = forms_config
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| setting(&root, "OFW_FORMS_CONFIG"));
    if let Some(ref c) = cfg {
        if !c.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')) {
            return Err("Invalid Forms config name. Use letters, digits, underscores, or hyphens.".into());
        }
    }
    let extra_params = parse_launch_params(launch_params)?;
    let mut jobs = jobs().lock().unwrap();
    let mut active = 0;
    jobs.retain(|_, job| match job.child.try_wait() { Ok(None) => { active += 1; true }, _ => true });
    if active >= 4 { return Err("Close an existing Oracle preview before opening another".into()); }
    if jobs.len() >= 32 {
        let completed: Vec<_> = jobs.iter_mut().filter_map(|(id, job)| job.child.try_wait().ok().flatten().map(|_| id.clone())).collect();
        for id in completed { if let Some(job) = jobs.remove(&id) { let _ = fs::remove_file(job.log); } }
    }
    let id = format!("{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    let log = std::env::temp_dir().join(format!("ofw-oracle-preview-{}.log", id));
    let output = fs::OpenOptions::new().write(true).create_new(true).open(&log).map_err(|e| e.to_string())?;
    let stderr = output.try_clone().map_err(|e| e.to_string())?;
    let mut command = Command::new(python_program());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.arg("-u").arg(script).arg(&fmx_name).arg("--interactive");
    if let Some(ref c) = cfg {
        command.args(["--config", c]);
    }
    for param in &extra_params {
        command.args(["--param", param]);
    }
    let child = command.current_dir(&root).stdin(Stdio::null()).stdout(output).stderr(stderr).spawn().map_err(|e| e.to_string())?;
    jobs.insert(id.clone(), Job { child, log });
    Ok((id, fmx_name))
}

pub fn status(id: &str) -> Option<serde_json::Value> {
    let mut jobs = jobs().lock().unwrap();
    let job = jobs.get_mut(id)?;
    let exit = job.child.try_wait().ok().flatten();
    let bytes = fs::read(&job.log).unwrap_or_default();
    let log = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(64000)..]);
    Some(serde_json::json!({ "running": exit.is_none(), "success": exit.map(|e| e.success()), "log": log }))
}

/// Read-only readiness probes. No container is started and no software is installed.
pub async fn readiness() -> serde_json::Value {
    async fn command(program: &str, args: &[&str]) -> bool {
        let mut command = tokio::process::Command::new(program);
        command.args(args).kill_on_drop(true).stdin(Stdio::null());
        match tokio::time::timeout(std::time::Duration::from_secs(4), command.output()).await {
            Ok(Ok(output)) if output.status.success() => {
                if program == "java" {
                    let version = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
                    version.split('"').nth(1).and_then(|value| value.split(['.', '-']).next())
                        .and_then(|major| major.parse::<u32>().ok()).is_some_and(|major| major >= 17)
                } else { true }
            }
            _ => false,
        }
    }
    /// Runs Podman (through WSL on Windows) and returns stdout when it succeeds. WSL start-up is slower, so allow more time.
    async fn podman_output(args: &[&str]) -> Option<String> {
        let mut command = podman_command(args);
        command.kill_on_drop(true).stdin(Stdio::null());
        match tokio::time::timeout(std::time::Duration::from_secs(10), command.output()).await {
            Ok(Ok(output)) if output.status.success() => Some(String::from_utf8_lossy(&output.stdout).into_owned()),
            _ => None,
        }
    }
    async fn container(name: &str) -> bool {
        podman_output(&["inspect", "--format", "{{.State.Running}}", name]).await.is_some_and(|out| out.trim() == "true")
    }
    async fn servlet() -> bool {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let probe = async {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", 7001)).await?;
            stream.write_all(b"GET /forms/frmservlet HTTP/1.0\r\nHost: localhost:7001\r\n\r\n").await?;
            let mut buffer = [0; 256];
            let n = stream.read(&mut buffer).await?;
            let status = String::from_utf8_lossy(&buffer[..n]).split_whitespace().nth(1).unwrap_or("").to_string();
            Ok::<bool, std::io::Error>(matches!(status.as_str(), "200" | "301" | "302"))
        };
        matches!(tokio::time::timeout(std::time::Duration::from_secs(4), probe).await, Ok(Ok(true)))
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let forms_name = forms_container(&root);
    let (python, java, podman, forms, database, listener) = tokio::join!(
        command(python_program(), &["--version"]), command("java", &["-version"]), async { podman_output(&["--version"]).await.is_some() },
        container(&forms_name), container("oracle23ai-lite"), servlet()
    );
    let jar = fsal_jar(&root).is_file();
    let tooling = root.join("plugins/oracle-forms/.apm/skills/compiling-and-running-oracle-forms/scripts/run_form.py").is_file();
    serde_json::json!({ "ready": python && java && podman && forms && listener && jar && tooling,
        "checks": [
            { "label": "Python 3 on the server host", "ready": python, "required": true },
            { "label": "Java 17 or newer on the server host", "ready": java, "required": true },
            { "label": "Podman CLI", "ready": podman, "required": true },
            { "label": "Forms 14c container running", "ready": forms, "required": true },
            { "label": "Forms servlet responding on localhost:7001", "ready": listener, "required": true },
            { "label": "FSAL launcher JAR in the reference installation", "ready": jar, "required": true },
            { "label": "Reference launcher script in this checkout", "ready": tooling, "required": true },
            { "label": "Oracle Free container running (needed for the local database workflow)", "ready": database, "required": false }
        ], "note": "Checks run on the Rust server's computer. Container availability does not verify database credentials, schema contents, compiler success or visual parity." })
}

/// Converts a binary Oracle artifact (.fmb, .mmb, .olb, .rdf, .rex, .pll) into XML (or .pld)
/// using the optional containerized Oracle 14c converter scripts.
pub async fn convert_to_xml(source_path: &std::path::Path) -> Result<(PathBuf, String), String> {
    let abs_source = source_path.canonicalize().map_err(|e| format!("Source file not found: {}", e))?;
    let ext = abs_source
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let stem = abs_source
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "Invalid source file name".to_string())?;
    let parent = abs_source
        .parent()
        .ok_or_else(|| "Invalid source file directory".to_string())?;

    let (script_name, expected_name) = match ext.as_str() {
        "fmb" | "mmb" | "olb" => ("convert_fmb.py", format!("{}_{}.xml", stem, ext)),
        "rdf" | "rex" => ("convert_rpt.py", format!("{}_rdf.xml", stem)),
        "pll" => ("convert_pll.py", format!("{}.pld", stem)),
        _ => return Err(format!("Unsupported binary extension '.{}' for Oracle XML conversion", ext)),
    };

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let script = root
        .join("plugins/oracle-forms/.apm/skills/converting-oracle-artifacts/scripts")
        .join(script_name);
    if !script.is_file() {
        return Err(format!("Converter script missing: {}", script.display()));
    }

    let mut command = tokio::process::Command::new(python_program());
    #[cfg(windows)]
    {
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = command
        .arg("-u")
        .arg(&script)
        .arg(&abs_source)
        .current_dir(&root)
        .output()
        .await
        .map_err(|e| format!("Failed to launch converter: {}", e))?;

    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let expected_path = parent.join(&expected_name);
    if !output.status.success() || !expected_path.is_file() {
        return Err(format!(
            "Oracle conversion failed (exit {:?}):\n{}",
            output.status.code(),
            log.trim()
        ));
    }
    Ok((expected_path, log))
}

#[cfg(test)]
mod tests {
    use super::{normalize_fmx_name, validate_compile_target};

    #[test]
    fn compile_target_defaults_and_accepts_tns_or_easy_connect() {
        assert_eq!(validate_compile_target(None).unwrap(), "FREEPDB1");
        assert_eq!(validate_compile_target(Some("   ")).unwrap(), "FREEPDB1");
        assert_eq!(validate_compile_target(Some("MY_ALIAS")).unwrap(), "MY_ALIAS");
        assert_eq!(
            validate_compile_target(Some("db.example.com:1521/ORCLPDB1")).unwrap(),
            "db.example.com:1521/ORCLPDB1"
        );
    }

    #[test]
    fn compile_target_rejects_shell_or_logon_delimiters() {
        for bad in ["user@FREEPDB1", "MY_ALIAS;rm -rf /", "host:1521/svc name", "MY_ALIAS'"] {
            assert!(validate_compile_target(Some(bad)).is_err(), "should reject {bad}");
        }
    }

    #[test]
    fn normalize_fmx_name_accepts_stem_or_fmx_and_rejects_paths() {
        assert_eq!(normalize_fmx_name("ORDERS").unwrap(), "ORDERS.fmx");
        assert_eq!(normalize_fmx_name("orders.fmx").unwrap(), "orders.fmx");
        assert_eq!(normalize_fmx_name("my_form_01.FMX").unwrap(), "my_form_01.fmx");
        for bad in ["", "../orders", "dir/orders", "orders.xml", "orders;rm"] {
            assert!(normalize_fmx_name(bad).is_err(), "should reject {bad}");
        }
    }

    #[test]
    fn parse_launch_params_validates_key_value_pairs() {
        assert_eq!(super::parse_launch_params(None).unwrap(), Vec::<String>::new());
        assert_eq!(super::parse_launch_params(Some("FLAG=Y MODE=TEST")).unwrap(), vec!["FLAG=Y", "MODE=TEST"]);
        assert!(super::parse_launch_params(Some("BAD_TOKEN")).is_err());
        assert!(super::parse_launch_params(Some("A=1;rm")).is_err());
    }
}
