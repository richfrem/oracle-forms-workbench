use std::collections::{BTreeMap, HashMap};
use oracle_forms_core::model::{
    AlertInfo, AttachedLibraryInfo, BlockInfo, CanvasInfo, GraphicInfo, ItemInfo,
    ListItemElementInfo, LovInfo, MenuInfo, MenuItemInfo, ObjectLibraryEntry,
    ObjectLibraryTabInfo, ParameterInfo, PointInfo, ProgramUnitInfo, ProgramUnitType,
    RadioButtonInfo, RecordGroupInfo, RelationInfo, TabPageInfo, TextEncoding, TriggerInfo,
    TriggerLevel, WindowInfo,
};
use oracle_forms_core::sidecars::{first_code_line, line_count_of};
use crate::layout::GeneratedLayout;

const PAGE_SIZE: usize = 4096;
const VPTR_BASE: u32 = 0x0001_0000;
const OBJ_PAGE_TAG: [u8; 4] = [0xff, 0xff, 0xff, 0xd4];

#[derive(Debug, Clone)]
struct RosRecord {
    self_vptr: u32,
    _obj_id: u32,
    obj_type: u16,
    parent_vptr: u32,
    props: Vec<(u16, u32)>,
}

/// Full module structure decoded directly from a `ROS.60050` binary (`.fmb`, `.mmb`, `.olb`).
pub struct DecodedRosModule {
    pub forms_version: String,
    pub module_name: Option<String>,
    pub menu_module: Option<String>,
    pub blocks: Vec<BlockInfo>,
    pub canvases: Vec<CanvasInfo>,
    pub windows: Vec<WindowInfo>,
    pub graphics: Vec<GraphicInfo>,
    pub triggers: Vec<TriggerInfo>,
    pub program_units: Vec<ProgramUnitInfo>,
    pub alerts: Vec<AlertInfo>,
    pub attached_libraries: Vec<AttachedLibraryInfo>,
    pub lovs: Vec<LovInfo>,
    pub record_groups: Vec<RecordGroupInfo>,
    pub parameters: Vec<ParameterInfo>,
    pub visual_attributes: Vec<String>,
    pub property_classes: Vec<String>,
    pub editors: Vec<String>,
    pub menus: Vec<MenuInfo>,
    pub object_library_tabs: Vec<ObjectLibraryTabInfo>,
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

fn is_known_ros_obj_type(t: u16) -> bool {
    matches!(
        t,
        0x03 | 0x05 | 0x07 | 0x09 | 0x0b | 0x0c | 0x0d | 0x0f | 0x11 | 0x14 | 0x16 | 0x18
            | 0x1e | 0x22 | 0x24 | 0x26 | 0x28 | 0x2a | 0x3a | 0x3c | 0x3e | 0x40 | 0x42
            | 0x44 | 0x4a | 0x4e | 0x52 | 0x58 | 0x62
    )
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

fn param_type_from_ros(code: u32) -> Option<String> {
    match code {
        0 => Some("Char".to_string()),
        1 => Some("Number".to_string()),
        2 => Some("Date".to_string()),
        _ => None,
    }
}

fn find_root_object_page(bytes: &[u8]) -> Option<usize> {
    let mut off = 0usize;
    while off + 0x20 <= bytes.len() {
        if bytes[off + 4..off + 8] == OBJ_PAGE_TAG {
            let oid = read_u32_be(bytes, off + 0x14);
            let otype = read_u16_be(bytes, off + 0x18);
            if oid == 1 && matches!(otype, 0x16 | 0x26 | 0x11) {
                return Some(off);
            }
        }
        off += PAGE_SIZE;
    }
    None
}

fn find_compact_root_offset(bytes: &[u8]) -> Option<usize> {
    let end = bytes.len().min(0x2000);
    if end < 0x1020 {
        return None;
    }
    let mut off = 0x1000usize;
    while off + 16 <= end {
        let oid = read_u32_be(bytes, off + 4);
        let otype = read_u16_be(bytes, off + 8);
        let pvptr = read_u32_be(bytes, off + 10);
        let pcnt = read_u16_be(bytes, off + 14);
        if oid == 1 && matches!(otype, 0x16 | 0x26 | 0x11) && pvptr == 0 && (1..=40).contains(&pcnt) {
            return Some(off);
        }
        off += 2;
    }
    None
}

fn build_vstream(bytes: &[u8], root_page: usize) -> Vec<u8> {
    let mut vbytes = Vec::new();
    let mut off = root_page;
    let mut page_idx = 0usize;
    while off + 8 <= bytes.len() {
        if bytes[off + 4..off + 8] == OBJ_PAGE_TAG {
            let skip = if page_idx == 0 { 0x10 } else { 0x08 };
            let end = (off + PAGE_SIZE).min(bytes.len());
            if off + skip < end {
                vbytes.extend_from_slice(&bytes[off + skip..end]);
            }
            page_idx += 1;
        }
        off += PAGE_SIZE;
    }
    vbytes
}

fn resolve_cstr(vbytes: &[u8], vptr: u32) -> Option<String> {
    let vmax = VPTR_BASE + vbytes.len() as u32;
    if !(VPTR_BASE..vmax).contains(&vptr) {
        return None;
    }
    let mut off = (vptr - VPTR_BASE) as usize;
    if (32..127).contains(&vbytes[off]) {
        let mut back = 0usize;
        while back < 24 && off > 0 && (32..127).contains(&vbytes[off - 1]) {
            off -= 1;
            back += 1;
        }
    } else if vbytes[off] == 0 {
        for pad in 1..=16 {
            if off + pad < vbytes.len() && vbytes[off + pad] != 0 {
                off += pad;
                break;
            }
        }
    }
    let end = vbytes[off..]
        .iter()
        .take(1024)
        .position(|&b| b == 0)
        .map(|rel| off + rel)?;
    if end <= off {
        return None;
    }
    let slice = &vbytes[off..end];
    if slice.iter().all(|&b| is_printable_latin1(b)) {
        Some(slice.iter().map(|&b| b as char).collect())
    } else {
        None
    }
}

/// Extract PL/SQL text blobs (`0x0000004f 0x0084`) from data pages before `limit_off`.
fn extract_plsql_blobs(bytes: &[u8], limit_off: usize) -> HashMap<u32, Vec<String>> {
    let mut blobs = HashMap::new();
    let end = bytes.len().min(limit_off);
    if end < 0x1008 {
        return blobs;
    }
    let mut off = 0x1000usize;
    while off + 8 <= end {
        if bytes[off..off + 6] == [0x00, 0x00, 0x00, 0x4f, 0x00, 0x84] {
            let blob_id = read_u32_be(bytes, off - 4);
            let mut chunks = Vec::new();
            let mut p = off + 0x20;
            let limit = end.min(off + 0x2000);
            while p + 4 < limit {
                let slen = read_u32_be(bytes, p) as usize;
                if (2..=8000).contains(&slen) && p + 4 + slen <= limit {
                    let seg = &bytes[p + 4..p + 4 + slen];
                    if seg.iter().all(|&b| is_printable_latin1(b)) {
                        chunks.push(seg.iter().map(|&b| b as char).collect());
                        p += 4 + slen;
                        continue;
                    }
                }
                if !chunks.is_empty() && bytes[p..p + 4] == [0x00, 0x00, 0x00, 0x0c] {
                    break;
                }
                p += 1;
            }
            if !chunks.is_empty() {
                blobs.insert(blob_id, chunks);
            }
        }
        off += 2;
    }
    blobs
}

fn clean_plsql_chunks(chunks: &[String], is_program_unit: bool) -> String {
    if chunks.is_empty() {
        return String::new();
    }
    // Chunk 0 is typically the internal stamp `P0_YYYY_...` (or unit name).
    let start_idx = if chunks[0].starts_with('P') && chunks[0].contains('_') && chunks.len() > 1 {
        1
    } else {
        0
    };
    let mut body_parts: Vec<&str> = Vec::new();
    for (idx, chunk) in chunks.iter().enumerate().skip(start_idx) {
        // Skip wrapper `BEGIN\n` / `\nEND;` and trailing `TRIGGER_NAME (SCOPE)` tag.
        let trimmed = chunk.trim();
        if !is_program_unit && idx == start_idx && trimmed == "BEGIN" && chunks.len() > start_idx + 2 {
            continue;
        }
        if !is_program_unit && idx + 1 == chunks.len() && (trimmed.contains('(') || trimmed == "END;") {
            continue;
        }
        if !is_program_unit
            && idx + 2 == chunks.len()
            && trimmed == "END;"
            && chunks.last().map(|s| s.contains('(')).unwrap_or(false)
        {
            continue;
        }
        if is_program_unit && idx == start_idx && !trimmed.contains(' ') && chunks.len() > start_idx + 1 {
            continue;
        }
        body_parts.push(chunk.as_str());
    }
    if body_parts.is_empty() {
        chunks.join("")
    } else {
        body_parts.join("")
    }
}

fn scan_records(vbytes: &[u8]) -> Vec<RosRecord> {
    let vmax = VPTR_BASE + vbytes.len() as u32;
    let mut records = Vec::new();
    let mut pos = 0usize;
    while pos + 16 <= vbytes.len() {
        let oid = read_u32_be(vbytes, pos + 4);
        let otype = read_u16_be(vbytes, pos + 8);
        let pvptr = read_u32_be(vbytes, pos + 10);
        let pcnt = read_u16_be(vbytes, pos + 14);
        let rec_bytes = 16 + (pcnt as usize) * 6;
        let is_wrapper = otype == 0x0c;

        if is_known_ros_obj_type(otype)
            && (1..=5000).contains(&oid)
            && (1..=80).contains(&pcnt)
            && (!is_wrapper || (pcnt == 1 && pvptr == 0))
            && (pvptr == 0 || (VPTR_BASE..vmax).contains(&pvptr))
            && pos + rec_bytes <= vbytes.len()
        {
            let mut props = Vec::with_capacity(pcnt as usize);
            let mut valid = true;
            for k in 0..(pcnt as usize) {
                let p_off = pos + 16 + k * 6;
                let pid = read_u16_be(vbytes, p_off);
                let pval = read_u32_be(vbytes, p_off + 2);
                if pid == 0 || pid > 0x600 {
                    valid = false;
                    break;
                }
                props.push((pid, pval));
            }
            let has_key = valid
                && if is_wrapper {
                    props.first().map_or(false, |&(pid, _)| pid == 0x001f)
                } else if otype == 0x22 {
                    props.iter().any(|&(pid, _)| pid == 0x0097 || pid == 0x0147)
                } else if otype == 0x52 {
                    props.iter().any(|&(pid, _)| pid == 0x0174 || pid == 0x0175)
                } else {
                    props.iter().any(|&(pid, _)| pid == 0x00d3)
                };
            if has_key {
                records.push(RosRecord {
                    self_vptr: VPTR_BASE + pos as u32,
                    _obj_id: oid,
                    obj_type: otype,
                    parent_vptr: pvptr,
                    props,
                });
                pos += rec_bytes;
                continue;
            }
        }
        pos += 2;
    }
    records
}

fn prop_u32(rec: &RosRecord, pid: u16) -> Option<u32> {
    rec.props.iter().find(|&&(k, _)| k == pid).map(|&(_, v)| v)
}

fn prop_str(rec: &RosRecord, vbytes: &[u8], pid: u16) -> Option<String> {
    prop_u32(rec, pid).and_then(|v| resolve_cstr(vbytes, v))
}

fn resolve_parent_vptr(
    pvptr: u32,
    wrappers: &HashMap<u32, u32>,
) -> u32 {
    wrappers.get(&pvptr).copied().unwrap_or(pvptr)
}

/// Decodes a full `ROS.60050` module (`.fmb`, `.mmb`, `.olb`) using the contiguous virtual
/// object stream, `parent_vptr` + `0x0c` wrapper resolution, and `0x0000004f 0x0084` PL/SQL blobs.
pub fn decode_ros60050_module(bytes: &[u8]) -> Option<DecodedRosModule> {
    if bytes.len() < 0x1000 || !bytes.starts_with(b"ROS.") {
        return None;
    }

    let (vbytes, blob_limit) = if let Some(root_page) = find_root_object_page(bytes) {
        (build_vstream(bytes, root_page), root_page)
    } else if let Some(root_off) = find_compact_root_offset(bytes) {
        // Compact 8 KB .mmb / .olb files store records starting at the oid=1 root offset.
        let end = bytes.len().min(0x2010);
        (bytes[root_off..end].to_vec(), 0x1000)
    } else {
        return None;
    };

    let records = scan_records(&vbytes);
    if records.is_empty() {
        return None;
    }

    let blobs = extract_plsql_blobs(bytes, blob_limit);
    let wrappers: HashMap<u32, u32> = records
        .iter()
        .filter(|r| r.obj_type == 0x0c)
        .filter_map(|r| r.props.first().map(|&(_, w)| (r.self_vptr, w)))
        .collect();

    // Map record self_vptr -> (obj_type, name, parent_vptr) for parent resolution.
    let mut rec_meta: HashMap<u32, (u16, String, u32)> = HashMap::new();
    for rec in &records {
        if rec.obj_type != 0x0c {
            if let Some(name) = prop_str(rec, &vbytes, 0x00d3) {
                rec_meta.insert(rec.self_vptr, (rec.obj_type, name, rec.parent_vptr));
            }
        }
    }

    let mut module_name: Option<String> = None;
    let mut menu_module: Option<String> = None;
    let mut blocks: Vec<BlockInfo> = Vec::new();
    let mut block_idx_by_vptr: HashMap<u32, usize> = HashMap::new();
    let mut item_idx_by_vptr: HashMap<u32, (usize, usize)> = HashMap::new();
    let mut canvases: Vec<CanvasInfo> = Vec::new();
    let mut canvas_idx_by_vptr: HashMap<u32, usize> = HashMap::new();
    let mut windows: Vec<WindowInfo> = Vec::new();
    let mut graphics: Vec<GraphicInfo> = Vec::new();
    let mut graphic_idx_by_vptr: HashMap<u32, usize> = HashMap::new();
    let mut triggers: Vec<TriggerInfo> = Vec::new();
    let mut program_units: Vec<ProgramUnitInfo> = Vec::new();
    let mut alerts: Vec<AlertInfo> = Vec::new();
    let mut attached_libraries: Vec<AttachedLibraryInfo> = Vec::new();
    let mut lovs: Vec<LovInfo> = Vec::new();
    let mut lov_idx_by_vptr: HashMap<u32, usize> = HashMap::new();
    let mut record_groups: Vec<RecordGroupInfo> = Vec::new();
    let mut rg_idx_by_vptr: HashMap<u32, usize> = HashMap::new();
    let mut parameters: Vec<ParameterInfo> = Vec::new();
    let mut visual_attributes: Vec<String> = Vec::new();
    let mut property_classes: Vec<String> = Vec::new();
    let mut editors: Vec<String> = Vec::new();
    let mut menus: Vec<MenuInfo> = Vec::new();
    let mut menu_idx_by_vptr: HashMap<u32, usize> = HashMap::new();
    let mut olb_tabs: Vec<ObjectLibraryTabInfo> = Vec::new();
    let mut relations_pending: Vec<(u32, RelationInfo)> = Vec::new();
    let mut radio_pending: Vec<(u32, RadioButtonInfo)> = Vec::new();
    let mut tab_pending: Vec<(u32, TabPageInfo)> = Vec::new();
    let mut lov_col_pending: Vec<(u32, String)> = Vec::new();
    let mut rg_col_pending: Vec<(u32, String)> = Vec::new();
    let mut menu_item_pending: Vec<(u32, MenuItemInfo)> = Vec::new();
    let mut list_elem_pending: Vec<(u32, ListItemElementInfo)> = Vec::new();
    let mut point_pending: Vec<(u32, PointInfo)> = Vec::new();
    let mut point_seq: usize = 0;

    for rec in &records {
        if rec.obj_type == 0x0c {
            continue;
        }
        let pvptr = resolve_parent_vptr(rec.parent_vptr, &wrappers);
        if rec.obj_type == 0x22 {
            let label = prop_str(rec, &vbytes, 0x0097);
            let value = prop_str(rec, &vbytes, 0x0147);
            if let Some(name) = label.or_else(|| value.clone()) {
                list_elem_pending.push((pvptr, ListItemElementInfo { name, value }));
            }
            continue;
        }
        if rec.obj_type == 0x52 {
            point_seq += 1;
            let name = prop_str(rec, &vbytes, 0x00d3).unwrap_or_else(|| format!("POINT{}", point_seq));
            point_pending.push((
                pvptr,
                PointInfo {
                    name,
                    x_position: prop_u32(rec, 0x0174).unwrap_or(0) as i32,
                    y_position: prop_u32(rec, 0x0175).unwrap_or(0) as i32,
                },
            ));
            continue;
        }
        let Some(name) = prop_str(rec, &vbytes, 0x00d3) else {
            continue;
        };

        match rec.obj_type {
            0x16 | 0x26 | 0x11 => {
                module_name = Some(name);
                if rec.obj_type == 0x16 {
                    menu_module = prop_str(rec, &vbytes, 0x0061);
                }
            }
            0x03 => {
                alerts.push(AlertInfo {
                    name,
                    message: prop_str(rec, &vbytes, 0x000b),
                    title: prop_str(rec, &vbytes, 0x0133),
                    button1_label: None,
                    button2_label: None,
                    button3_label: None,
                    default_button: None,
                });
            }
            0x05 => {
                attached_libraries.push(AttachedLibraryInfo {
                    name,
                    library_location: prop_str(rec, &vbytes, 0x009c),
                });
            }
            0x07 => {
                let idx = blocks.len();
                block_idx_by_vptr.insert(rec.self_vptr, idx);
                blocks.push(BlockInfo {
                    name,
                    query_data_source_name: prop_str(rec, &vbytes, 0x020a),
                    records_displayed: prop_u32(rec, 0x010c).unwrap_or(1) as i32,
                    ..Default::default()
                });
            }
            0x1e => {
                let b_idx = block_idx_by_vptr
                    .get(&pvptr)
                    .copied()
                    .or_else(|| blocks.len().checked_sub(1));
                if let Some(bi) = b_idx {
                    let ii = blocks[bi].items.len();
                    item_idx_by_vptr.insert(rec.self_vptr, (bi, ii));
                    blocks[bi].items.push(ItemInfo {
                        name,
                        item_type: prop_u32(rec, 0x008b).and_then(item_type_from_ros),
                        column_name: prop_str(rec, &vbytes, 0x0158),
                        canvas_name: prop_str(rec, &vbytes, 0x0021),
                        tab_page_name: prop_str(rec, &vbytes, 0x01f2),
                        x_position: prop_u32(rec, 0x0174).unwrap_or(0) as i32,
                        y_position: prop_u32(rec, 0x0175).unwrap_or(0) as i32,
                        width: prop_u32(rec, 0x0169).map(|v| v as i32),
                        height: prop_u32(rec, 0x0079).map(|v| v as i32),
                        label: prop_str(rec, &vbytes, 0x0097),
                        prompt: prop_str(rec, &vbytes, 0x018a),
                        font_name: prop_str(rec, &vbytes, 0x0067),
                        font_size: prop_u32(rec, 0x0068).map(|v| v as i32),
                        fore_color: prop_str(rec, &vbytes, 0x006c),
                        back_color: prop_str(rec, &vbytes, 0x0013),
                        ..Default::default()
                    });
                }
            }
            0x3e => {
                radio_pending.push((
                    pvptr,
                    RadioButtonInfo {
                        name,
                        label: prop_str(rec, &vbytes, 0x0097),
                        x_position: prop_u32(rec, 0x0174).unwrap_or(0) as i32,
                        y_position: prop_u32(rec, 0x0175).unwrap_or(0) as i32,
                        width: prop_u32(rec, 0x0169).map(|v| v as i32),
                        height: prop_u32(rec, 0x0079).map(|v| v as i32),
                        fore_color: prop_str(rec, &vbytes, 0x006c),
                        back_color: prop_str(rec, &vbytes, 0x0013),
                        font_name: prop_str(rec, &vbytes, 0x0067),
                        font_size: prop_u32(rec, 0x0068).map(|v| v as i32),
                        ..Default::default()
                    },
                ));
            }
            0x0b => {
                let idx = canvases.len();
                canvas_idx_by_vptr.insert(rec.self_vptr, idx);
                canvases.push(CanvasInfo {
                    name,
                    canvas_type: prop_u32(rec, 0x002d).and_then(canvas_type_from_ros),
                    window_name: prop_str(rec, &vbytes, 0x016f).or_else(|| prop_str(rec, &vbytes, 0x0164)),
                    property_class: None,
                    raise_on_enter: None,
                    width: prop_u32(rec, 0x0169).unwrap_or(800) as i32,
                    height: prop_u32(rec, 0x0079).unwrap_or(600) as i32,
                    viewport_width: prop_u32(rec, 0x015f).map(|v| v as i32),
                    viewport_height: prop_u32(rec, 0x015e).map(|v| v as i32),
                    viewport_x_position: prop_u32(rec, 0x004e).unwrap_or(0) as i32,
                    viewport_y_position: prop_u32(rec, 0x004f).unwrap_or(0) as i32,
                    viewport_x_position_on_canvas: None,
                    viewport_y_position_on_canvas: None,
                    back_color: prop_str(rec, &vbytes, 0x0013),
                    visual_attribute: None,
                    inherited: None,
                    tab_pages: Vec::new(),
                });
            }
            0x58 => {
                tab_pending.push((
                    pvptr,
                    TabPageInfo {
                        name,
                        label: prop_str(rec, &vbytes, 0x0097),
                        ..Default::default()
                    },
                ));
            }
            0x09 => {
                let parent_canvas = canvas_idx_by_vptr
                    .get(&pvptr)
                    .copied()
                    .and_then(|ci| canvases.get(ci))
                    .map(|c| c.name.clone())
                    .or_else(|| canvases.last().map(|c| c.name.clone()));
                let g_idx = graphics.len();
                graphic_idx_by_vptr.insert(rec.self_vptr, g_idx);
                graphics.push(GraphicInfo {
                    name,
                    canvas_name: parent_canvas,
                    graphics_type: prop_u32(rec, 0x0182)
                        .map(graphic_type_from_ros)
                        .unwrap_or_else(|| "Rectangle".to_string()),
                    frame_title: prop_str(rec, &vbytes, 0x0133),
                    x_position: prop_u32(rec, 0x0174).unwrap_or(0) as i32,
                    y_position: prop_u32(rec, 0x0175).unwrap_or(0) as i32,
                    width: prop_u32(rec, 0x0169).unwrap_or(0) as i32,
                    height: prop_u32(rec, 0x0079).unwrap_or(0) as i32,
                    ..Default::default()
                });
            }
            0x4e => {
                windows.push(WindowInfo {
                    name,
                    title: prop_str(rec, &vbytes, 0x0133),
                    width: prop_u32(rec, 0x0169).map(|v| v as i32),
                    height: prop_u32(rec, 0x0079).map(|v| v as i32),
                    modal: None,
                    horizontal_toolbar_canvas_name: None,
                    vertical_toolbar_canvas_name: None,
                });
            }
            0x24 => {
                let idx = lovs.len();
                lov_idx_by_vptr.insert(rec.self_vptr, idx);
                lovs.push(LovInfo {
                    name,
                    record_group_name: prop_str(rec, &vbytes, 0x0111),
                    column_mappings: Vec::new(),
                });
            }
            0x0d => {
                lov_col_pending.push((pvptr, name));
            }
            0x40 => {
                let idx = record_groups.len();
                rg_idx_by_vptr.insert(rec.self_vptr, idx);
                record_groups.push(RecordGroupInfo {
                    name,
                    query_text: prop_str(rec, &vbytes, 0x0113),
                    columns: Vec::new(),
                });
            }
            0x0f => {
                rg_col_pending.push((pvptr, name));
            }
            0x18 => {
                parameters.push(ParameterInfo {
                    name,
                    data_type: prop_u32(rec, 0x0073).and_then(param_type_from_ros),
                    default_value: prop_str(rec, &vbytes, 0x0074),
                });
            }
            0x42 => {
                relations_pending.push((
                    pvptr,
                    RelationInfo {
                        name,
                        detail_block: None,
                        join_condition: None,
                        join_encoding: TextEncoding::Original,
                        deferred: None,
                        auto_query: None,
                        prevent_masterless_operations: None,
                        delete_record: None,
                        relation_type: None,
                        inherited: None,
                    },
                ));
            }
            0x4a => visual_attributes.push(name),
            0x3c => property_classes.push(name),
            0x14 => editors.push(name),
            0x28 => {
                let idx = menus.len();
                menu_idx_by_vptr.insert(rec.self_vptr, idx);
                menus.push(MenuInfo {
                    name,
                    items: Vec::new(),
                });
            }
            0x2a => {
                menu_item_pending.push((
                    pvptr,
                    MenuItemInfo {
                        name,
                        label: prop_str(rec, &vbytes, 0x0097),
                        command_type: None,
                        sub_menu_name: None,
                        menu_item_type: None,
                        command_ref: None,
                        text_encoding: TextEncoding::Original,
                    },
                ));
            }
            0x62 => {
                olb_tabs.push(ObjectLibraryTabInfo {
                    name,
                    entries: Vec::<ObjectLibraryEntry>::new(),
                });
            }
            0x44 => {
                let (level, block_name, item_name) = match rec_meta.get(&pvptr) {
                    Some((0x07, bname, _)) => (TriggerLevel::Block, Some(bname.clone()), None),
                    Some((0x1e, iname, item_pv)) => {
                        let resolved_item_pv = resolve_parent_vptr(*item_pv, &wrappers);
                        let bname = rec_meta.get(&resolved_item_pv).map(|(_, n, _)| n.clone());
                        (TriggerLevel::Item, bname, Some(iname.clone()))
                    }
                    Some((0x2a, _, _)) => (TriggerLevel::Menu, None, None),
                    _ => (TriggerLevel::Form, None, None),
                };
                let body = prop_u32(rec, 0x01d1)
                    .or_else(|| prop_u32(rec, 0x01d3))
                    .and_then(|id| blobs.get(&id))
                    .map(|chunks| clean_plsql_chunks(chunks, false))
                    .unwrap_or_default();
                triggers.push(TriggerInfo {
                    execution_hierarchy: None,
                    name,
                    level,
                    block_name,
                    item_name,
                    first_line: first_code_line(&body),
                    line_count: line_count_of(&body),
                    inherited: None,
                    text_encoding: TextEncoding::Original,
                    text_ref: None,
                    xml_ref: None,
                    source_text: (!body.is_empty()).then_some(body),
                });
            }
            0x3a => {
                let body = prop_u32(rec, 0x01d3)
                    .or_else(|| prop_u32(rec, 0x01d1))
                    .and_then(|id| blobs.get(&id))
                    .map(|chunks| clean_plsql_chunks(chunks, true))
                    .unwrap_or_default();
                let upper = body.trim_start().to_ascii_uppercase();
                let unit_type = if upper.starts_with("FUNCTION") {
                    ProgramUnitType::Function
                } else if upper.starts_with("PACKAGE BODY") {
                    ProgramUnitType::PackageBody
                } else if upper.starts_with("PACKAGE") {
                    ProgramUnitType::PackageSpec
                } else {
                    ProgramUnitType::Procedure
                };
                program_units.push(ProgramUnitInfo {
                    name,
                    unit_type,
                    line_count: line_count_of(&body),
                    first_line: first_code_line(&body),
                    inherited: None,
                    text_encoding: TextEncoding::Original,
                    text_ref: None,
                    xml_ref: None,
                    source_text: (!body.is_empty()).then_some(body),
                });
            }
            _ => {}
        }
    }

    for (pvptr, rb) in radio_pending {
        if let Some(&(bi, ii)) = item_idx_by_vptr.get(&pvptr) {
            blocks[bi].items[ii].radio_buttons.push(rb);
        } else if let Some(last_b) = blocks.last_mut() {
            if let Some(last_i) = last_b.items.last_mut() {
                last_i.radio_buttons.push(rb);
            }
        }
    }
    for (pvptr, tp) in tab_pending {
        if let Some(&ci) = canvas_idx_by_vptr.get(&pvptr) {
            canvases[ci].tab_pages.push(tp);
        } else if let Some(last_c) = canvases.last_mut() {
            last_c.tab_pages.push(tp);
        }
    }
    for (pvptr, col) in lov_col_pending {
        if let Some(&li) = lov_idx_by_vptr.get(&pvptr) {
            lovs[li].column_mappings.push(col);
        }
    }
    for (pvptr, col) in rg_col_pending {
        if let Some(&ri) = rg_idx_by_vptr.get(&pvptr) {
            record_groups[ri].columns.push(col);
        }
    }
    for (pvptr, rel) in relations_pending {
        if let Some(&bi) = block_idx_by_vptr.get(&pvptr) {
            blocks[bi].relations.push(rel);
        }
    }
    for (pvptr, mi) in menu_item_pending {
        if let Some(&m_idx) = menu_idx_by_vptr.get(&pvptr) {
            menus[m_idx].items.push(mi);
        } else if let Some(last_m) = menus.last_mut() {
            last_m.items.push(mi);
        }
    }
    for (pvptr, le) in list_elem_pending {
        if let Some(&(bi, ii)) = item_idx_by_vptr.get(&pvptr) {
            blocks[bi].items[ii].list_elements.push(le);
        } else if let Some(last_b) = blocks.last_mut() {
            if let Some(last_i) = last_b.items.last_mut() {
                last_i.list_elements.push(le);
            }
        }
    }
    for (pvptr, pt) in point_pending {
        if let Some(&gi) = graphic_idx_by_vptr.get(&pvptr) {
            graphics[gi].points.push(pt);
        } else if let Some(last_g) = graphics.last_mut() {
            last_g.points.push(pt);
        }
    }

    let header_prefix = String::from_utf8_lossy(&bytes[..16.min(bytes.len())]);
    let forms_version = if header_prefix.starts_with("ROS.60050") {
        "6i (ROS.60050)".to_string()
    } else {
        header_prefix.trim_end_matches('\0').trim().to_string()
    };

    Some(DecodedRosModule {
        forms_version,
        module_name,
        menu_module,
        blocks,
        canvases,
        windows,
        graphics,
        triggers,
        program_units,
        alerts,
        attached_libraries,
        lovs,
        record_groups,
        parameters,
        visual_attributes,
        property_classes,
        editors,
        menus,
        object_library_tabs: olb_tabs,
    })
}

/// Decodes program units from a `ROS.60050` `.pll` library (e.g., `utils.pll`).
pub fn decode_ros60050_pll(bytes: &[u8]) -> Option<Vec<ProgramUnitInfo>> {
    if bytes.len() < 0x1000 || !bytes.starts_with(b"ROS.") {
        return None;
    }
    let mut units = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // Scan for 0x00010000 + u16 name_len + NAME + u32 body_len + BODY
    let mut off = 0x1000usize;
    while off + 16 < bytes.len() {
        if bytes[off..off + 4] == [0x00, 0x01, 0x00, 0x00] {
            let nlen = read_u16_be(bytes, off + 4) as usize;
            if (2..=64).contains(&nlen) && off + 6 + nlen + 4 < bytes.len() {
                let nslice = &bytes[off + 6..off + 6 + nlen];
                if nslice.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$') {
                    let blen_off = off + 6 + nlen;
                    let blen = read_u32_be(bytes, blen_off) as usize;
                    if (10..=32768).contains(&blen) && blen_off + 4 + blen <= bytes.len() {
                        let bslice = &bytes[blen_off + 4..blen_off + 4 + blen];
                        if bslice.iter().all(|&b| is_printable_latin1(b)) {
                            let name: String = nslice.iter().map(|&b| b as char).collect();
                            let body: String = bslice.iter().map(|&b| b as char).collect();
                            let upper = body.trim_start().to_ascii_uppercase();
                            let unit_type = if upper.starts_with("FUNCTION") {
                                ProgramUnitType::Function
                            } else if upper.starts_with("PACKAGE BODY") {
                                ProgramUnitType::PackageBody
                            } else if upper.starts_with("PACKAGE") {
                                ProgramUnitType::PackageSpec
                            } else {
                                ProgramUnitType::Procedure
                            };
                            let sig = format!("{}:{:?}", name, unit_type);
                            if seen.insert(sig) {
                                units.push(ProgramUnitInfo {
                                    name,
                                    unit_type,
                                    line_count: line_count_of(&body),
                                    first_line: first_code_line(&body),
                                    inherited: None,
                                    text_encoding: TextEncoding::Original,
                                    text_ref: None,
                                    xml_ref: None,
                                    source_text: Some(body),
                                });
                                off = blen_off + 4 + blen;
                                continue;
                            }
                        }
                    }
                }
            }
        }
        off += 1;
    }
    (!units.is_empty()).then_some(units)
}

/// Decodes ROS 60050 binary object records (Block, Item, RadioButton, Canvas, TabPage, Graphic, Window)
/// directly from an `.fmb` byte slice. Returns `None` if no valid ROS object table with geometry is found.
pub fn decode_ros60050_layout(bytes: &[u8]) -> Option<GeneratedLayout> {
    let decoded = decode_ros60050_module(bytes)?;
    let has_positioned_item = decoded
        .blocks
        .iter()
        .flat_map(|b| b.items.iter())
        .any(|i| i.x_position > 0 || i.y_position > 0 || i.width.unwrap_or(0) > 0);
    if decoded.canvases.is_empty() || !has_positioned_item {
        return None;
    }
    Some(GeneratedLayout {
        blocks: decoded.blocks,
        canvases: decoded.canvases,
        windows: decoded.windows,
        graphics: decoded.graphics,
    })
}

#[allow(dead_code)]
fn _unused_btree_marker(_: BTreeMap<String, String>) {}
