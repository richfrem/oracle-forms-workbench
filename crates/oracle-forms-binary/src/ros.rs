use std::collections::HashMap;
use oracle_forms_core::model::{
    BlockInfo, CanvasInfo, GraphicInfo, ItemInfo, RadioButtonInfo, TabPageInfo, WindowInfo,
};
use crate::layout::GeneratedLayout;

#[derive(Debug)]
struct RosRecord {
    pos: usize,
    rec_vptr: u32,
    _obj_id: u32,
    obj_type: u16,
    _parent_vptr: u32,
    prop_count: u16,
    props: Vec<(u16, u32)>,
}

fn read_u16_be(bytes: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([bytes[off], bytes[off + 1]])
}

fn read_u32_be(bytes: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
}

fn is_printable_latin1(b: u8) -> bool {
    (32..127).contains(&b) || matches!(b, b'\t' | b'\n' | b'\r')
}

fn extract_aligned_strings(bytes: &[u8], start: usize, end: usize) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut p = (start + 3) & !3;
    while p < end {
        if (32..127).contains(&bytes[p]) {
            let mut j = p;
            while j < end && is_printable_latin1(bytes[j]) {
                j += 1;
            }
            if j < end && bytes[j] == 0 && j > p {
                let s: String = bytes[p..j].iter().map(|&b| b as char).collect();
                out.push((p, s));
                p = (j + 1 + 3) & !3;
                continue;
            }
        }
        p += 4;
    }
    out
}

fn resolve_record_strings(
    bytes: &[u8],
    rec: &RosRecord,
    next_pos: usize,
    next_vptr: u32,
) -> HashMap<u32, String> {
    let rec_end = rec.pos + 16 + (rec.prop_count as usize) * 6;
    let scan_start = rec_end + 4;
    if scan_start >= next_pos {
        return HashMap::new();
    }

    let strs = extract_aligned_strings(bytes, scan_start, next_pos);
    let mut str_vptrs: Vec<u32> = rec
        .props
        .iter()
        .map(|&(_, v)| v)
        .filter(|&v| v > rec.rec_vptr && v < next_vptr)
        .collect();
    str_vptrs.sort_unstable();
    str_vptrs.dedup();

    if str_vptrs.is_empty() || strs.is_empty() {
        return HashMap::new();
    }

    for &(base_off, _) in &strs {
        let delta = (str_vptrs[0] as i64) - (base_off as i64);
        let mut matched = HashMap::new();
        for &vp in &str_vptrs {
            let target = (vp as i64) - delta;
            if let Some((_, txt)) = strs.iter().find(|&&(off, _)| off as i64 == target) {
                matched.insert(vp, txt.clone());
            }
        }
        if matched.len() == str_vptrs.len() {
            return matched;
        }
    }

    let mut fallback = HashMap::new();
    for (idx, &vp) in str_vptrs.iter().enumerate() {
        if let Some((_, txt)) = strs.get(idx) {
            fallback.insert(vp, txt.clone());
        }
    }
    fallback
}

fn item_type_from_ros(code: u32) -> Option<String> {
    match code {
        1 => Some("Check Box".to_string()),
        2 => Some("Display Item".to_string()),
        5 => Some("List Item".to_string()),
        6 => Some("Push Button".to_string()),
        7 => Some("Radio Group".to_string()),
        8 => Some("Text Item".to_string()),
        _ => None,
    }
}

fn canvas_type_from_ros(code: u32) -> Option<String> {
    match code {
        0 => Some("Content".to_string()),
        1 => Some("Stacked".to_string()),
        2 => Some("Horizontal Toolbar".to_string()),
        3 => Some("Vertical Toolbar".to_string()),
        4 => Some("Tab".to_string()),
        _ => None,
    }
}

fn graphic_type_from_ros(code: u32) -> String {
    match code {
        1 => "Image".to_string(),
        2 => "Line".to_string(),
        4 => "Rectangle".to_string(),
        5 => "Rounded Rectangle".to_string(),
        6 => "Text".to_string(),
        8 => "Frame".to_string(),
        _ => "Rectangle".to_string(),
    }
}

/// Decodes ROS 60050 binary object records (Block, Item, RadioButton, Canvas, TabPage, Graphic, Window)
/// directly from an `.fmb` byte slice. Returns `None` if no valid ROS object table with geometry is found.
pub fn decode_ros60050_layout(bytes: &[u8]) -> Option<GeneratedLayout> {
    if bytes.len() < 0x1000 || !bytes.starts_with(b"ROS.") {
        return None;
    }

    let mut seg_base = None;
    let mut page = 0usize;
    while page + 0x40 < bytes.len() {
        if bytes[page + 0x32..page + 0x34] == [0x00, 0xd3] {
            seg_base = Some(page);
            break;
        }
        page += 0x1000;
    }
    let seg_base = seg_base?;

    let mut records = Vec::new();
    let mut pos = seg_base;
    while pos + 16 <= bytes.len() {
        let rec_vptr = read_u32_be(bytes, pos);
        let obj_id = read_u32_be(bytes, pos + 4);
        let obj_type = read_u16_be(bytes, pos + 8);
        let parent_vptr = read_u32_be(bytes, pos + 10);
        let prop_count = read_u16_be(bytes, pos + 14);

        let rec_bytes = 16 + (prop_count as usize) * 6;
        if (0x10000..0x100000).contains(&rec_vptr)
            && (1..=5000).contains(&obj_id)
            && (1..=100).contains(&obj_type)
            && (1..=80).contains(&prop_count)
            && (parent_vptr == 0 || (0x10000..0x100000).contains(&parent_vptr))
            && pos + rec_bytes <= bytes.len()
        {
            let mut props = Vec::with_capacity(prop_count as usize);
            let mut valid = true;
            for k in 0..(prop_count as usize) {
                let p_off = pos + 16 + k * 6;
                let pid = read_u16_be(bytes, p_off);
                let pval = read_u32_be(bytes, p_off + 2);
                if pid == 0 || pid > 0x600 {
                    valid = false;
                    break;
                }
                props.push((pid, pval));
            }
            if valid && props.iter().any(|&(pid, _)| pid == 0x00d3) {
                records.push(RosRecord {
                    pos,
                    rec_vptr,
                    _obj_id: obj_id,
                    obj_type,
                    _parent_vptr: parent_vptr,
                    prop_count,
                    props,
                });
                pos += rec_bytes;
                continue;
            }
        }
        pos += 2;
    }

    if records.is_empty() {
        return None;
    }

    let mut blocks: Vec<BlockInfo> = Vec::new();
    let mut canvases: Vec<CanvasInfo> = Vec::new();
    let mut windows: Vec<WindowInfo> = Vec::new();
    let mut graphics: Vec<GraphicInfo> = Vec::new();

    for (idx, rec) in records.iter().enumerate() {
        let rec_end = rec.pos + 16 + (rec.prop_count as usize) * 6;
        let next_pos = records
            .get(idx + 1)
            .map(|r| r.pos)
            .unwrap_or_else(|| (rec_end + 1024).min(bytes.len()));
        let next_vptr = records
            .get(idx + 1)
            .map(|r| r.rec_vptr)
            .unwrap_or(rec.rec_vptr + 0x2000);

        let str_map = resolve_record_strings(bytes, rec, next_pos, next_vptr);
        let get_u32 = |pid: u16| -> Option<u32> {
            rec.props.iter().find(|&&(k, _)| k == pid).map(|&(_, v)| v)
        };
        let get_str = |pid: u16| -> Option<String> {
            get_u32(pid).and_then(|v| str_map.get(&v).cloned())
        };

        let Some(name) = get_str(0x00d3) else {
            continue;
        };

        match rec.obj_type {
            0x07 => {
                blocks.push(BlockInfo {
                    name,
                    query_data_source_name: get_str(0x020a),
                    records_displayed: get_u32(0x010c).unwrap_or(1) as i32,
                    ..Default::default()
                });
            }
            0x1e => {
                if let Some(block) = blocks.last_mut() {
                    block.items.push(ItemInfo {
                        name,
                        item_type: get_u32(0x008b).and_then(item_type_from_ros),
                        column_name: get_str(0x0158),
                        canvas_name: get_str(0x0021),
                        tab_page_name: get_str(0x01f2),
                        x_position: get_u32(0x0174).unwrap_or(0) as i32,
                        y_position: get_u32(0x0175).unwrap_or(0) as i32,
                        width: get_u32(0x0169).map(|v| v as i32),
                        height: get_u32(0x0079).map(|v| v as i32),
                        label: get_str(0x0097),
                        prompt: get_str(0x018a),
                        font_name: get_str(0x0067),
                        font_size: get_u32(0x0068).map(|v| v as i32),
                        fore_color: get_str(0x006c),
                        back_color: get_str(0x0013),
                        ..Default::default()
                    });
                }
            }
            0x3e => {
                if let Some(block) = blocks.last_mut() {
                    if let Some(item) = block.items.last_mut() {
                        item.radio_buttons.push(RadioButtonInfo {
                            name,
                            label: get_str(0x0097),
                            x_position: get_u32(0x0174).unwrap_or(0) as i32,
                            y_position: get_u32(0x0175).unwrap_or(0) as i32,
                            width: get_u32(0x0169).map(|v| v as i32),
                            height: get_u32(0x0079).map(|v| v as i32),
                            fore_color: get_str(0x006c),
                            back_color: get_str(0x0013),
                            font_name: get_str(0x0067),
                            font_size: get_u32(0x0068).map(|v| v as i32),
                            ..Default::default()
                        });
                    }
                }
            }
            0x0b => {
                canvases.push(CanvasInfo {
                    name,
                    canvas_type: get_u32(0x002d).and_then(canvas_type_from_ros),
                    window_name: get_str(0x0164).or_else(|| get_str(0x001e)),
                    property_class: None,
                    raise_on_enter: None,
                    width: get_u32(0x0169).unwrap_or(800) as i32,
                    height: get_u32(0x0079).unwrap_or(600) as i32,
                    viewport_width: get_u32(0x015f).map(|v| v as i32),
                    viewport_height: get_u32(0x015e).map(|v| v as i32),
                    viewport_x_position: get_u32(0x004e).unwrap_or(0) as i32,
                    viewport_y_position: get_u32(0x004f).unwrap_or(0) as i32,
                    viewport_x_position_on_canvas: None,
                    viewport_y_position_on_canvas: None,
                    back_color: get_str(0x0013),
                    visual_attribute: None,
                    inherited: None,
                    tab_pages: Vec::new(),
                });
            }
            0x58 => {
                if let Some(canvas) = canvases.last_mut() {
                    canvas.tab_pages.push(TabPageInfo {
                        name,
                        label: get_str(0x0097),
                        ..Default::default()
                    });
                }
            }
            0x09 => {
                if let Some(canvas) = canvases.last() {
                    graphics.push(GraphicInfo {
                        name,
                        canvas_name: Some(canvas.name.clone()),
                        graphics_type: get_u32(0x0182).map(graphic_type_from_ros).unwrap_or_else(|| "Rectangle".to_string()),
                        x_position: get_u32(0x0174).unwrap_or(0) as i32,
                        y_position: get_u32(0x0175).unwrap_or(0) as i32,
                        width: get_u32(0x0169).unwrap_or(0) as i32,
                        height: get_u32(0x0079).unwrap_or(0) as i32,
                        ..Default::default()
                    });
                }
            }
            0x4e => {
                windows.push(WindowInfo {
                    name,
                    title: get_str(0x0133),
                    width: get_u32(0x0169).map(|v| v as i32),
                    height: get_u32(0x0079).map(|v| v as i32),
                    modal: None,
                    horizontal_toolbar_canvas_name: None,
                    vertical_toolbar_canvas_name: None,
                });
            }
            _ => {}
        }
    }

    let has_positioned_item = blocks
        .iter()
        .flat_map(|b| b.items.iter())
        .any(|i| i.x_position > 0 || i.y_position > 0 || i.width.unwrap_or(0) > 0);
    if canvases.is_empty() || !has_positioned_item {
        return None;
    }

    Some(GeneratedLayout {
        blocks,
        canvases,
        windows,
        graphics,
    })
}
