use std::fs;
use std::path::Path;
use oracle_forms_core::model::{ModuleIndex, ModuleType, TriggerLevel};

const PAGE_SIZE: usize = 4096;
const VPTR_BASE: u32 = 0x0001_0000;

/// Native `ROS.60050` binary writer (`.fmb`, `.mmb`, `.olb`) from a `ModuleIndex`.
///
/// Emits a big-endian 4 KB-paged `ROS.60050` container with PL/SQL text blobs (`0x0000004f 0x0084`),
/// allocation bitmap page, and contiguous `0xffffffd4` object pages readable by both
/// `decode_ros60050_module` and the experiment parity tools.
pub struct RosBinaryWriter;

#[derive(Clone)]
enum PropVal {
    U32(u32),
    Str(String),
}

struct BlobTable {
    bytes: Vec<u8>,
    next_id: u32,
}

impl BlobTable {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            next_id: 1,
        }
    }

    fn push_plsql(&mut self, text: &str) -> u32 {
        let id = self.next_id;
        self.next_id += 1;

        self.bytes.extend_from_slice(&id.to_be_bytes());
        self.bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x4f, 0x00, 0x84]);
        // Pad to 0x20 bytes from blob header before length-prefixed text chunk
        self.bytes.resize(self.bytes.len() + 26, 0);

        let raw: Vec<u8> = text.bytes().collect();
        self.bytes.extend_from_slice(&(raw.len() as u32).to_be_bytes());
        self.bytes.extend_from_slice(&raw);
        self.bytes.extend_from_slice(&0x0000_000cu32.to_be_bytes());
        while self.bytes.len() % 4 != 0 {
            self.bytes.push(0);
        }
        id
    }
}

struct PendingRecord {
    obj_type: u16,
    parent_idx: Option<usize>,
    props: Vec<(u16, PropVal)>,
}

struct VStreamBuilder {
    records: Vec<PendingRecord>,
}

impl VStreamBuilder {
    fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// Queues a record and returns its 0-based record index (for parent linking).
    fn emit_record(&mut self, obj_type: u16, parent_idx: Option<usize>, props: Vec<(u16, PropVal)>) -> usize {
        let idx = self.records.len();
        self.records.push(PendingRecord {
            obj_type,
            parent_idx,
            props,
        });
        idx
    }

    /// Materializes Pass 1 (all contiguous RosRecords) followed by Pass 2 (post-record string pool).
    fn finish(self) -> Vec<u8> {
        let mut rec_vptrs: Vec<u32> = Vec::with_capacity(self.records.len());
        let mut cur_off = 0usize;
        for r in &self.records {
            rec_vptrs.push(VPTR_BASE + cur_off as u32);
            cur_off += 16 + r.props.len() * 6;
        }
        let str_pool_start = cur_off;
        let mut buf = vec![0u8; str_pool_start];
        let mut write_off = 0usize;

        for (idx, r) in self.records.iter().enumerate() {
            let oid = (idx + 1) as u32;
            let pvptr = r.parent_idx.map(|pi| rec_vptrs[pi]).unwrap_or(0);
            let pcnt = r.props.len() as u16;
            let rec_end_vptr = VPTR_BASE + (write_off + 16 + r.props.len() * 6) as u32;

            buf[write_off..write_off + 4].copy_from_slice(&rec_end_vptr.to_be_bytes());
            buf[write_off + 4..write_off + 8].copy_from_slice(&oid.to_be_bytes());
            buf[write_off + 8..write_off + 10].copy_from_slice(&r.obj_type.to_be_bytes());
            buf[write_off + 10..write_off + 14].copy_from_slice(&pvptr.to_be_bytes());
            buf[write_off + 14..write_off + 16].copy_from_slice(&pcnt.to_be_bytes());

            for (pidx, (pid, val)) in r.props.iter().enumerate() {
                let slot_off = write_off + 16 + pidx * 6;
                buf[slot_off..slot_off + 2].copy_from_slice(&pid.to_be_bytes());
                match val {
                    PropVal::U32(n) => {
                        buf[slot_off + 2..slot_off + 6].copy_from_slice(&n.to_be_bytes());
                    }
                    PropVal::Str(s) => {
                        while buf.len() % 4 != 0 {
                            buf.push(0);
                        }
                        let str_vptr = VPTR_BASE + buf.len() as u32;
                        for b in s.bytes() {
                            buf.push(if b == 0 { b' ' } else { b });
                        }
                        buf.push(0);
                        while buf.len() % 4 != 0 {
                            buf.push(0);
                        }
                        buf[slot_off + 2..slot_off + 6].copy_from_slice(&str_vptr.to_be_bytes());
                    }
                }
            }
            write_off += 16 + r.props.len() * 6;
        }
        buf
    }
}

fn item_type_to_ros(item_type: Option<&str>) -> u32 {
    match item_type.unwrap_or("Text Item") {
        "Check Box" => 1,
        "Display Item" => 2,
        "List Item" => 5,
        "Push Button" => 6,
        "Radio Group" => 7,
        _ => 8,
    }
}

fn canvas_type_to_ros(canvas_type: Option<&str>) -> u32 {
    match canvas_type.unwrap_or("Content") {
        "Stacked" => 1,
        "Horizontal Toolbar" => 2,
        "Vertical Toolbar" => 3,
        "Tab" => 4,
        _ => 0,
    }
}

fn graphic_type_to_ros(gtype: &str) -> u32 {
    match gtype {
        "Image" => 1,
        "Line" => 2,
        "Rounded Rectangle" => 5,
        "Text" => 6,
        "Frame" => 8,
        _ => 4,
    }
}

fn param_type_to_ros(dtype: Option<&str>) -> u32 {
    match dtype.unwrap_or("Char") {
        "Number" => 1,
        "Date" => 2,
        _ => 0,
    }
}

impl RosBinaryWriter {
    /// Serializes a `ModuleIndex` into a `ROS.60050` 4 KB-paged binary buffer.
    pub fn write_module(module: &ModuleIndex) -> Vec<u8> {
        let mut blobs = BlobTable::new();
        let mut vs = VStreamBuilder::new();

        let root_type = match module.key.module_type {
            ModuleType::Menu => 0x26,
            ModuleType::ObjectLibrary => 0x11,
            ModuleType::Library => 0x20,
            _ => 0x16,
        };
        let mut root_props = vec![(0x00d3, PropVal::Str(module.key.name.clone()))];
        if let Some(ref mm) = module.menu_module {
            root_props.push((0x0061, PropVal::Str(mm.clone())));
        }
        if let Some(title) = module.windows.first().and_then(|w| w.title.as_ref()) {
            root_props.push((0x0133, PropVal::Str(title.clone())));
        }
        let root_idx = vs.emit_record(root_type, None, root_props);

        for alert in &module.alerts {
            let mut props = vec![(0x00d3, PropVal::Str(alert.name.clone()))];
            if let Some(ref msg) = alert.message {
                props.push((0x000b, PropVal::Str(msg.clone())));
            }
            if let Some(ref ttl) = alert.title {
                props.push((0x0133, PropVal::Str(ttl.clone())));
            }
            vs.emit_record(0x03, Some(root_idx), props);
        }

        for lib in &module.attached_libraries {
            let mut props = vec![(0x00d3, PropVal::Str(lib.name.clone()))];
            if let Some(ref loc) = lib.library_location {
                props.push((0x009c, PropVal::Str(loc.clone())));
            }
            vs.emit_record(0x05, Some(root_idx), props);
        }

        for ed in &module.editors {
            vs.emit_record(0x14, Some(root_idx), vec![(0x00d3, PropVal::Str(ed.clone()))]);
        }
        for pc in &module.property_classes {
            vs.emit_record(0x3c, Some(root_idx), vec![(0x00d3, PropVal::Str(pc.clone()))]);
        }
        for va in &module.visual_attributes {
            vs.emit_record(0x4a, Some(root_idx), vec![(0x00d3, PropVal::Str(va.clone()))]);
        }
        for param in &module.parameters {
            let mut props = vec![
                (0x00d3, PropVal::Str(param.name.clone())),
                (0x0073, PropVal::U32(param_type_to_ros(param.data_type.as_deref()))),
            ];
            if let Some(ref dv) = param.default_value {
                props.push((0x0074, PropVal::Str(dv.clone())));
            }
            vs.emit_record(0x18, Some(root_idx), props);
        }

        for rg in &module.record_groups {
            let mut props = vec![(0x00d3, PropVal::Str(rg.name.clone()))];
            if let Some(ref q) = rg.query_text {
                props.push((0x0113, PropVal::Str(q.clone())));
            }
            let rg_idx = vs.emit_record(0x40, Some(root_idx), props);
            for col in &rg.columns {
                vs.emit_record(0x0f, Some(rg_idx), vec![(0x00d3, PropVal::Str(col.clone()))]);
            }
        }

        for lov in &module.lovs {
            let mut props = vec![(0x00d3, PropVal::Str(lov.name.clone()))];
            if let Some(ref rg) = lov.record_group_name {
                props.push((0x0111, PropVal::Str(rg.clone())));
            }
            let lov_idx = vs.emit_record(0x24, Some(root_idx), props);
            for col in &lov.column_mappings {
                vs.emit_record(0x0d, Some(lov_idx), vec![(0x00d3, PropVal::Str(col.clone()))]);
            }
        }

        for win in &module.windows {
            let mut props = vec![(0x00d3, PropVal::Str(win.name.clone()))];
            if let Some(ref t) = win.title {
                props.push((0x0133, PropVal::Str(t.clone())));
            }
            if let Some(w) = win.width {
                props.push((0x0169, PropVal::U32(w as u32)));
            }
            if let Some(h) = win.height {
                props.push((0x0079, PropVal::U32(h as u32)));
            }
            vs.emit_record(0x4e, Some(root_idx), props);
        }

        for canvas in &module.canvases {
            let mut props = vec![
                (0x00d3, PropVal::Str(canvas.name.clone())),
                (0x002d, PropVal::U32(canvas_type_to_ros(canvas.canvas_type.as_deref()))),
                (0x0169, PropVal::U32(canvas.width as u32)),
                (0x0079, PropVal::U32(canvas.height as u32)),
                (0x004e, PropVal::U32(canvas.viewport_x_position as u32)),
                (0x004f, PropVal::U32(canvas.viewport_y_position as u32)),
            ];
            if let Some(ref wname) = canvas.window_name {
                props.push((0x016f, PropVal::Str(wname.clone())));
            }
            if let Some(vw) = canvas.viewport_width {
                props.push((0x015f, PropVal::U32(vw as u32)));
            }
            if let Some(vh) = canvas.viewport_height {
                props.push((0x015e, PropVal::U32(vh as u32)));
            }
            if let Some(ref bc) = canvas.back_color {
                props.push((0x0013, PropVal::Str(bc.clone())));
            }
            let c_idx = vs.emit_record(0x0b, Some(root_idx), props);
            for tp in &canvas.tab_pages {
                let mut tprops = vec![(0x00d3, PropVal::Str(tp.name.clone()))];
                if let Some(ref lbl) = tp.label {
                    tprops.push((0x0097, PropVal::Str(lbl.clone())));
                }
                vs.emit_record(0x58, Some(c_idx), tprops);
            }
            for g in module.graphics.iter().filter(|g| {
                g.canvas_name
                    .as_deref()
                    .map(|c| c.eq_ignore_ascii_case(&canvas.name))
                    .unwrap_or(false)
            }) {
                let mut gprops = vec![
                    (0x00d3, PropVal::Str(g.name.clone())),
                    (0x0182, PropVal::U32(graphic_type_to_ros(&g.graphics_type))),
                    (0x0174, PropVal::U32(g.x_position as u32)),
                    (0x0175, PropVal::U32(g.y_position as u32)),
                    (0x0169, PropVal::U32(g.width as u32)),
                    (0x0079, PropVal::U32(g.height as u32)),
                ];
                if let Some(ref ft) = g.frame_title {
                    gprops.push((0x0133, PropVal::Str(ft.clone())));
                }
                let g_idx = vs.emit_record(0x09, Some(c_idx), gprops);
                for pt in &g.points {
                    vs.emit_record(
                        0x52,
                        Some(g_idx),
                        vec![
                            (0x0174, PropVal::U32(pt.x_position as u32)),
                            (0x0175, PropVal::U32(pt.y_position as u32)),
                        ],
                    );
                }
            }
        }

        for block in &module.blocks {
            let mut bprops = vec![
                (0x00d3, PropVal::Str(block.name.clone())),
                (0x010c, PropVal::U32(block.records_displayed.max(1) as u32)),
            ];
            if let Some(ref qds) = block.query_data_source_name {
                bprops.push((0x020a, PropVal::Str(qds.clone())));
            }
            let b_idx = vs.emit_record(0x07, Some(root_idx), bprops);

            for rel in &block.relations {
                vs.emit_record(0x42, Some(b_idx), vec![(0x00d3, PropVal::Str(rel.name.clone()))]);
            }

            for trig in module.triggers.iter().filter(|t| {
                t.level == TriggerLevel::Block
                    && t.block_name.as_deref().map(|b| b.eq_ignore_ascii_case(&block.name)).unwrap_or(false)
            }) {
                let body = trig.source_text.as_deref().unwrap_or(&trig.first_line);
                let bid = blobs.push_plsql(body);
                vs.emit_record(
                    0x44,
                    Some(b_idx),
                    vec![
                        (0x00d3, PropVal::Str(trig.name.clone())),
                        (0x01d1, PropVal::U32(bid)),
                    ],
                );
            }

            for item in &block.items {
                let mut iprops = vec![
                    (0x00d3, PropVal::Str(item.name.clone())),
                    (0x008b, PropVal::U32(item_type_to_ros(item.item_type.as_deref()))),
                    (0x0174, PropVal::U32(item.x_position as u32)),
                    (0x0175, PropVal::U32(item.y_position as u32)),
                ];
                if let Some(w) = item.width {
                    iprops.push((0x0169, PropVal::U32(w as u32)));
                }
                if let Some(h) = item.height {
                    iprops.push((0x0079, PropVal::U32(h as u32)));
                }
                if let Some(ref cn) = item.column_name {
                    iprops.push((0x0158, PropVal::Str(cn.clone())));
                }
                if let Some(ref cnv) = item.canvas_name {
                    iprops.push((0x0021, PropVal::Str(cnv.clone())));
                }
                if let Some(ref tp) = item.tab_page_name {
                    iprops.push((0x01f2, PropVal::Str(tp.clone())));
                }
                if let Some(ref lbl) = item.label {
                    iprops.push((0x0097, PropVal::Str(lbl.clone())));
                }
                if let Some(ref pr) = item.prompt {
                    iprops.push((0x018a, PropVal::Str(pr.clone())));
                }
                if let Some(ref fnm) = item.font_name {
                    iprops.push((0x0067, PropVal::Str(fnm.clone())));
                }
                if let Some(fs) = item.font_size {
                    iprops.push((0x0068, PropVal::U32(fs as u32)));
                }
                if let Some(ref fc) = item.fore_color {
                    iprops.push((0x006c, PropVal::Str(fc.clone())));
                }
                if let Some(ref bc) = item.back_color {
                    iprops.push((0x0013, PropVal::Str(bc.clone())));
                }
                let i_idx = vs.emit_record(0x1e, Some(b_idx), iprops);

                for rb in &item.radio_buttons {
                    let mut rprops = vec![
                        (0x00d3, PropVal::Str(rb.name.clone())),
                        (0x0174, PropVal::U32(rb.x_position as u32)),
                        (0x0175, PropVal::U32(rb.y_position as u32)),
                    ];
                    if let Some(ref lbl) = rb.label {
                        rprops.push((0x0097, PropVal::Str(lbl.clone())));
                    }
                    if let Some(w) = rb.width {
                        rprops.push((0x0169, PropVal::U32(w as u32)));
                    }
                    if let Some(h) = rb.height {
                        rprops.push((0x0079, PropVal::U32(h as u32)));
                    }
                    vs.emit_record(0x3e, Some(i_idx), rprops);
                }

                for le in &item.list_elements {
                    let mut lprops = vec![(0x0097, PropVal::Str(le.name.clone()))];
                    if let Some(ref val) = le.value {
                        lprops.push((0x0147, PropVal::Str(val.clone())));
                    }
                    vs.emit_record(0x22, Some(i_idx), lprops);
                }

                for trig in module.triggers.iter().filter(|t| {
                    t.level == TriggerLevel::Item
                        && t.block_name.as_deref().map(|b| b.eq_ignore_ascii_case(&block.name)).unwrap_or(false)
                        && t.item_name.as_deref().map(|i| i.eq_ignore_ascii_case(&item.name)).unwrap_or(false)
                }) {
                    let body = trig.source_text.as_deref().unwrap_or(&trig.first_line);
                    let bid = blobs.push_plsql(body);
                    vs.emit_record(
                        0x44,
                        Some(i_idx),
                        vec![
                            (0x00d3, PropVal::Str(trig.name.clone())),
                            (0x01d1, PropVal::U32(bid)),
                        ],
                    );
                }
            }
        }

        for trig in module.triggers.iter().filter(|t| t.level == TriggerLevel::Form) {
            let body = trig.source_text.as_deref().unwrap_or(&trig.first_line);
            let bid = blobs.push_plsql(body);
            vs.emit_record(
                0x44,
                Some(root_idx),
                vec![
                    (0x00d3, PropVal::Str(trig.name.clone())),
                    (0x01d1, PropVal::U32(bid)),
                ],
            );
        }

        for pu in &module.program_units {
            let body = pu.source_text.as_deref().unwrap_or(&pu.first_line);
            let bid = blobs.push_plsql(body);
            vs.emit_record(
                0x3a,
                Some(root_idx),
                vec![
                    (0x00d3, PropVal::Str(pu.name.clone())),
                    (0x01d3, PropVal::U32(bid)),
                ],
            );
        }

        for menu in &module.menus {
            let m_idx = vs.emit_record(0x28, Some(root_idx), vec![(0x00d3, PropVal::Str(menu.name.clone()))]);
            for mi in &menu.items {
                let mut mprops = vec![(0x00d3, PropVal::Str(mi.name.clone()))];
                if let Some(ref lbl) = mi.label {
                    mprops.push((0x0097, PropVal::Str(lbl.clone())));
                }
                vs.emit_record(0x2a, Some(m_idx), mprops);
            }
        }

        for tab in &module.object_library_tabs {
            vs.emit_record(0x62, Some(root_idx), vec![(0x00d3, PropVal::Str(tab.name.clone()))]);
        }

        let vstream = vs.finish();

        // Assemble 4 KB pages: [Header 0x0000] [Blob pages 0x1000..] [Bitmap page] [Object pages 0xffffffd4]
        let mut out: Vec<u8> = Vec::new();
        // Page 0: Header
        let mut hdr_page = vec![0u8; PAGE_SIZE];
        hdr_page[..9].copy_from_slice(b"ROS.60050");
        out.extend_from_slice(&hdr_page);

        // Blob pages
        let blob_pages = (blobs.bytes.len() + PAGE_SIZE - 1).max(1) / PAGE_SIZE;
        let blob_pages = blob_pages.max(1);
        for p in 0..blob_pages {
            let mut page = vec![0u8; PAGE_SIZE];
            let start = p * PAGE_SIZE;
            if start < blobs.bytes.len() {
                let end = (start + PAGE_SIZE).min(blobs.bytes.len());
                page[..end - start].copy_from_slice(&blobs.bytes[start..end]);
            }
            out.extend_from_slice(&page);
        }

        // Bitmap page
        let mut bmap = vec![0u8; PAGE_SIZE];
        bmap[..12].copy_from_slice(&[0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0xf0, 0x0f]);
        out.extend_from_slice(&bmap);

        // Object pages (root page skips 0x10 bytes, subsequent pages skip 0x08 bytes)
        let mut v_off = 0usize;
        let mut obj_idx = 0usize;
        while v_off < vstream.len() || obj_idx == 0 {
            let page_off = out.len() as u32;
            let mut page = vec![0u8; PAGE_SIZE];
            page[..4].copy_from_slice(&page_off.to_be_bytes());
            page[4..8].copy_from_slice(&[0xff, 0xff, 0xff, 0xd4]);
            let hdr_skip = if obj_idx == 0 { 0x10 } else { 0x08 };
            let cap = PAGE_SIZE - hdr_skip;
            let take = cap.min(vstream.len().saturating_sub(v_off));
            if take > 0 {
                page[hdr_skip..hdr_skip + take].copy_from_slice(&vstream[v_off..v_off + take]);
                v_off += take;
            }
            out.extend_from_slice(&page);
            obj_idx += 1;
        }

        let total_pages = (out.len() / PAGE_SIZE) as u32;
        let file_size = out.len() as u32;
        out[0x0c..0x10].copy_from_slice(&total_pages.to_be_bytes());
        out[0x14..0x18].copy_from_slice(&file_size.to_be_bytes());
        out
    }

    /// Writes a `ModuleIndex` as a `ROS.60050` binary file.
    pub fn write_to_file(module: &ModuleIndex, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, Self::write_module(module))
    }
}
