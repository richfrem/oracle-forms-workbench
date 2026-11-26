//! Purpose: Integration/conformance tests (recovery.rs).
//! Key Input Dependencies: oracle_forms_binary, oracle_forms_core
//! Index: resolve_fmb, test_direct_binary_fmb_parsing, test_simple_module_binary_parsing

use std::fs;
use std::path::PathBuf;
use oracle_forms_binary::BinaryFmbParser;
use oracle_forms_core::model::{ModuleKey, ModuleType};

fn resolve_fmb(name: &str) -> Option<PathBuf> {
    let prefixes = ["", ".", "..", "../..", "../../..", "../../../.."];
    let subdirs = ["temp/example-fmb/internal-testing", "example-fmb/internal-testing", "example-fmb/samples", "example-fmb", "samples"];
    for p in &prefixes {
        for s in &subdirs {
            let candidate = if p.is_empty() {
                PathBuf::from(s).join(name)
            } else {
                PathBuf::from(p).join(s).join(name)
            };
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

// Set OFW_TEST_FMB to the path of any real binary .fmb to run this test; it is skipped otherwise.
#[test]
fn test_direct_binary_fmb_parsing() {
    let fmb_file = match std::env::var_os("OFW_TEST_FMB").map(PathBuf::from) {
        Some(f) if f.is_file() => f,
        _ => {
            eprintln!("Skipping test_direct_binary_fmb_parsing: set OFW_TEST_FMB to a binary .fmb");
            return;
        }
    };
    let temp_cache = std::env::temp_dir().join("test_fmb_cache");
    let _ = fs::create_dir_all(&temp_cache);

    let module_name = fmb_file.file_stem().unwrap().to_string_lossy().to_ascii_uppercase();
    let key = ModuleKey::new(&module_name, ModuleType::Form);
    let index = BinaryFmbParser::parse(&key, &fmb_file, &temp_cache).expect("Should parse the binary .fmb");

    assert_eq!(index.key.name, module_name);
    assert_eq!(index.key.module_type, ModuleType::Form);

    assert!(!index.program_units.is_empty(), "Program units should not be empty");
    let text_ref = index.program_units[0].text_ref.as_ref().expect("Program unit textRef should not be null");
    let pu_file = temp_cache.join(&text_ref.file);
    assert!(pu_file.is_file(), "Program unit sidecar file should exist at {}", pu_file.display());

    assert!(!index.triggers.is_empty(), "Triggers should not be empty");
    let trig_ref = index.triggers[0].text_ref.as_ref().expect("Trigger textRef should not be null");
    let trig_file = temp_cache.join(&trig_ref.file);
    assert!(trig_file.is_file(), "Trigger sidecar file should exist at {}", trig_file.display());

    assert!(!index.blocks.is_empty(), "Blocks should not be empty");
    assert!(!index.canvases.is_empty(), "Canvases should not be empty");
    let canvas = &index.canvases[0];
    assert!(canvas.width > 0 && canvas.height > 0, "Canvas should have valid dimensions");
}

#[test]
fn test_simple_module_binary_parsing() {
    let fmb_file = resolve_fmb("simple_module.fmb")
        .expect("simple_module.fmb must exist in samples/");
    let temp_cache = std::env::temp_dir().join("test_simple_fmb_cache");
    let _ = fs::create_dir_all(&temp_cache);

    let key = ModuleKey::new("SIMPLE_MODULE", ModuleType::Form);
    let index = BinaryFmbParser::parse(&key, &fmb_file, &temp_cache).expect("Should parse simple_module.fmb");

    assert_eq!(index.key.name, "SIMPLE_MODULE");
    assert!(!index.canvases.is_empty());
}
