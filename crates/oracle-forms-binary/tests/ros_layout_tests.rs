use std::path::Path;
use oracle_forms_binary::decode_ros60050_layout;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

#[test]
fn decodes_ofw_demo_employees_fmb_coordinates_without_synthetic_fallback() {
    let fmb_path = repo_root().join("samples/ofw_demo_employees.fmb");
    let bytes = std::fs::read(&fmb_path).expect("read ofw_demo_employees.fmb");
    let layout = decode_ros60050_layout(&bytes).expect("decode ROS 60050 layout");

    let canvas = layout
        .canvases
        .iter()
        .find(|c| c.name == "CANVAS_MAIN")
        .expect("CANVAS_MAIN decoded");
    assert_eq!(canvas.width, 540);
    assert_eq!(canvas.height, 300);

    let control = layout
        .blocks
        .iter()
        .find(|b| b.name == "CONTROL")
        .expect("CONTROL block decoded");
    let btn_search = control
        .items
        .iter()
        .find(|i| i.name == "BTN_SEARCH")
        .expect("BTN_SEARCH decoded");
    assert_eq!(btn_search.x_position, 300);
    assert_eq!(btn_search.y_position, 18);
    assert_eq!(btn_search.width, Some(80));
    assert_eq!(btn_search.height, Some(24));
    assert_eq!(btn_search.item_type.as_deref(), Some("Push Button"));

    let emp = layout
        .blocks
        .iter()
        .find(|b| b.name == "EMP")
        .expect("EMP block decoded");
    let first_name = emp
        .items
        .iter()
        .find(|i| i.name == "FIRST_NAME")
        .expect("FIRST_NAME decoded");
    assert_eq!(first_name.x_position, 110);
    assert_eq!(first_name.y_position, 100);
    assert_eq!(first_name.width, Some(150));
    assert_eq!(first_name.height, Some(20));
}

#[test]
fn decodes_ofw_sales_fmb_tab_and_multi_record_blocks() {
    let fmb_path = repo_root().join("samples/ofw_sales.fmb");
    let bytes = std::fs::read(&fmb_path).expect("read ofw_sales.fmb");
    let layout = decode_ros60050_layout(&bytes).expect("decode ROS 60050 layout");

    let tab = layout
        .canvases
        .iter()
        .find(|c| c.name == "TAB_DETAIL")
        .expect("TAB_DETAIL decoded");
    assert_eq!(tab.canvas_type.as_deref(), Some("Tab"));
    assert_eq!(tab.viewport_x_position, 10);
    assert_eq!(tab.viewport_y_position, 268);
    assert_eq!(tab.tab_pages.len(), 3);

    let customers = layout
        .blocks
        .iter()
        .find(|b| b.name == "CUSTOMERS")
        .expect("CUSTOMERS block decoded");
    assert_eq!(customers.records_displayed, 6);
}

#[test]
fn decodes_simple_module_fmb_radio_group_and_buttons() {
    let fmb_path = repo_root().join("samples/simple_module.fmb");
    let bytes = std::fs::read(&fmb_path).expect("read simple_module.fmb");
    let layout = decode_ros60050_layout(&bytes).expect("decode ROS 60050 layout");

    let block5 = layout
        .blocks
        .iter()
        .find(|b| b.name == "BLOCK5")
        .expect("BLOCK5 decoded");
    let rg = block5
        .items
        .iter()
        .find(|i| i.name == "RADIO_GROUP288")
        .expect("RADIO_GROUP288 decoded");
    assert_eq!(rg.item_type.as_deref(), Some("Radio Group"));
    assert_eq!(rg.x_position, 236);
    assert_eq!(rg.y_position, 322);
    assert_eq!(rg.radio_buttons.len(), 3);
    assert_eq!(rg.radio_buttons[0].x_position, 21);
    assert_eq!(rg.radio_buttons[0].y_position, 259);
}
