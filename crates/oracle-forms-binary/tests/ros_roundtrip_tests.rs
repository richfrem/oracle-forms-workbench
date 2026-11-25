use std::path::Path;
use oracle_forms_binary::{
    decode_ros60050_module, decode_ros60050_pll, parse_rdf_report, write_rdf_report,
    BinaryFmbParser, RosBinaryWriter,
};
use oracle_forms_core::model::{ModuleKey, ModuleType};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

#[test]
fn ros60050_decodes_all_sample_fmb_mmb_olb_pll_and_rdf() {
    let root = repo_root();
    for stem in [
        "toolbar",
        "styles",
        "dupes",
        "entry",
        "picker",
        "claims",
        "orders",
        "master_details",
        "ofw_demo_employees",
        "customers",
        "ofw_sales",
    ] {
        let bytes = std::fs::read(root.join(format!("samples/{}.fmb", stem))).unwrap();
        let decoded = decode_ros60050_module(&bytes).expect(stem);
        assert!(!decoded.blocks.is_empty(), "{} blocks", stem);
    }

    for stem in ["mainmenu", "ofw_emp_menu"] {
        let bytes = std::fs::read(root.join(format!("samples/{}.mmb", stem))).unwrap();
        let decoded = decode_ros60050_module(&bytes).expect(stem);
        assert!(!decoded.menus.is_empty(), "{} menus", stem);
    }

    let olb = std::fs::read(root.join("samples/objects.olb")).unwrap();
    let decoded_olb = decode_ros60050_module(&olb).expect("objects.olb");
    assert!(!decoded_olb.object_library_tabs.is_empty());

    let pll = std::fs::read(root.join("samples/utils.pll")).unwrap();
    let units = decode_ros60050_pll(&pll).expect("utils.pll");
    assert!(!units.is_empty());

    let rdf = parse_rdf_report(&root.join("samples/demo_employees.rdf")).expect("demo_employees.rdf");
    assert!(!rdf.data.data_sources.is_empty());
    let rdf_bytes = write_rdf_report(&rdf);
    assert!(rdf_bytes.starts_with(b"ROS.60050"));
}

#[test]
fn ros_binary_writer_roundtrips_module_index_cleanly() {
    let root = repo_root();
    let cache = std::env::temp_dir().join("ofw_ros_roundtrip_test");
    let _ = std::fs::create_dir_all(&cache);

    for (file, mtype, stem) in [
        ("samples/toolbar.fmb", ModuleType::Form, "TOOLBAR"),
        ("samples/orders.fmb", ModuleType::Form, "ORDERS"),
        ("samples/ofw_demo_employees.fmb", ModuleType::Form, "OFW_DEMO_EMPLOYEES"),
        ("samples/customers.fmb", ModuleType::Form, "CUSTOMERS"),
        ("samples/ofw_emp_menu.mmb", ModuleType::Menu, "OFW_EMP_MENU"),
    ] {
        let key = ModuleKey::new(stem, mtype);
        let orig = BinaryFmbParser::parse(&key, &root.join(file), &cache).unwrap();
        let bin = RosBinaryWriter::write_module(&orig);
        assert!(bin.starts_with(b"ROS.60050"));
        let decoded = decode_ros60050_module(&bin).expect(file);
        assert_eq!(decoded.blocks.len(), orig.blocks.len(), "{} block count", file);
        assert_eq!(decoded.canvases.len(), orig.canvases.len(), "{} canvas count", file);
        assert_eq!(decoded.windows.len(), orig.windows.len(), "{} window count", file);
        assert_eq!(decoded.menus.len(), orig.menus.len(), "{} menu count", file);
    }
}
