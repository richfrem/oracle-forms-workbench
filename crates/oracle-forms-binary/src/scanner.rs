//! Purpose: Binary FMB/MMB/OLB parser combining ROS.60050 decoding with regex fallback.
//! Key Input Dependencies: regex, oracle_forms_core
//! Index: BinaryFmbParser, parse, safe_slice, find_code_in_chunk, extract_complete_code_block, clean_code_string, is_readable_text

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::Path;
use regex::Regex;
use oracle_forms_core::error::Result;
use oracle_forms_core::model::*;
use oracle_forms_core::sidecars::{
    fingerprint_file, first_code_line, line_count_of, PlsqlSidecars,
};
use crate::layout::generate_2column_layout;
use crate::ros::{decode_ros60050_module, decode_ros60050_pll};

pub struct BinaryFmbParser;

impl BinaryFmbParser {
    pub fn parse(key: &ModuleKey, source_file: &Path, module_cache_dir: &Path) -> Result<ModuleIndex> {
        let bytes = fs::read(source_file)?;
        let mut sidecars = PlsqlSidecars::new(module_cache_dir);

        // 1. Detect Forms Version Header from first 16 bytes
        let header_prefix = if bytes.len() >= 16 {
            String::from_utf8_lossy(&bytes[0..16]).to_string()
        } else {
            String::new()
        };
        let forms_version = if header_prefix.starts_with("ROS.60050") {
            "6i (ROS.60050)".to_string()
        } else if header_prefix.starts_with("ROS.") {
            header_prefix.trim_end_matches('\0').trim().to_string()
        } else {
            "Binary .FMB".to_string()
        };

        // Convert whole file bytes into Latin-1 string
        let text_latin1: String = bytes.iter().map(|&b| b as char).collect();

        // 2. Extract Program Units
        let prog_unit_rx = Regex::new(r"(?i)\b(PROCEDURE|FUNCTION|PACKAGE\s+BODY|PACKAGE)\s+([A-Za-z0-9_$]+)\b").unwrap();
        let mut program_units = Vec::new();
        let mut seen_units = HashSet::new();

        let ignore_keywords: HashSet<&str> = [
            "IS", "AS", "BEGIN", "NULL", "THEN", "ELSE", "RETURN"
        ].into_iter().collect();

        for cap in prog_unit_rx.captures_iter(&text_latin1) {
            let m = cap.get(0).unwrap();
            let kind_str = cap.get(1).unwrap().as_str().to_ascii_uppercase();
            let unit_name = cap.get(2).unwrap().as_str().to_ascii_uppercase();

            if seen_units.contains(&unit_name) || ignore_keywords.contains(unit_name.as_str()) {
                continue;
            }

            let start_pos = m.start();
            let chunk = safe_slice(&text_latin1, start_pos, start_pos + 5000);
            let clean_code = extract_complete_code_block(chunk);

            if clean_code.len() > 25 && is_readable_text(&clean_code) {
                seen_units.insert(unit_name.clone());
                let unit_type = ProgramUnitType::from_forms(&kind_str);
                let type_name = format!("{:?}", unit_type).to_ascii_uppercase();
                let text_ref = sidecars.write(
                    PlsqlSidecars::PROGRAM_UNITS,
                    &format!("{}.{}", unit_name, type_name),
                    &clean_code,
                )?;

                program_units.push(ProgramUnitInfo {
                    name: unit_name,
                    unit_type,
                    line_count: line_count_of(&clean_code),
                    first_line: first_code_line(&clean_code),
                    source_text: Some(clean_code.clone()),
                    inherited: None,
                    text_encoding: TextEncoding::Original,
                    text_ref: Some(text_ref),
                    xml_ref: None,
                });
            }
        }

        // 3. Extract Triggers (Scoped: TRIGGER_NAME (BLOCK.ITEM))
        let trigger_scope_rx = Regex::new(r"(?i)((?:WHEN-|KEY-|ON-|PRE-|POST-)[A-Z0-9_-]+)\s*\(([A-Z0-9_$.]+)\)").unwrap();
        let mut triggers = Vec::new();
        let mut seen_triggers = HashSet::new();
        let mut discovered_items_by_block: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

        for cap in trigger_scope_rx.captures_iter(&text_latin1) {
            let m = cap.get(0).unwrap();
            let t_name = cap.get(1).unwrap().as_str().to_ascii_uppercase();
            let scope = cap.get(2).unwrap().as_str().to_ascii_uppercase();
            let tag_pos = m.start();

            // Look up to 1500 chars before tag for the trigger body
            let chunk_before = safe_slice(&text_latin1, tag_pos.saturating_sub(1500), tag_pos);
            let code = find_code_in_chunk(chunk_before);

            let sig = format!("{}.{}", scope, t_name);
            if !seen_triggers.contains(&sig) && !code.trim().is_empty() {
                seen_triggers.insert(sig);

                let (level, b_name, i_name) = if scope.contains('.') {
                    let parts: Vec<&str> = scope.splitn(2, '.').collect();
                    let b = parts[0].to_string();
                    let i = parts[1].to_string();
                    discovered_items_by_block.entry(b.clone()).or_default().insert(i.clone());
                    (TriggerLevel::Item, Some(b), Some(i))
                } else if scope.starts_with("CLS") || scope.contains("MODULE") || scope == key.name {
                    (TriggerLevel::Form, None, None)
                } else {
                    let b = scope.clone();
                    discovered_items_by_block.entry(b.clone()).or_default();
                    (TriggerLevel::Block, Some(b), None)
                };

                let text_ref = sidecars.write(
                    PlsqlSidecars::TRIGGERS,
                    &format!("{}.{}", scope, t_name),
                    &code,
                )?;

                triggers.push(TriggerInfo {
                    execution_hierarchy: None,
                    name: t_name,
                    level,
                    block_name: b_name,
                    item_name: i_name,
                    first_line: first_code_line(&code),
                    source_text: Some(code.clone()),
                    line_count: line_count_of(&code),
                    inherited: None,
                    text_encoding: TextEncoding::Original,
                    text_ref: Some(text_ref),
                    xml_ref: None,
                });
            }
        }

        // 4. Extract Standalone Form-Level Triggers
        let common_form_triggers = [
            ("WHEN-NEW-FORM-INSTANCE", vec!["WhenNewFormInstance"]),
            ("ON-MESSAGE", vec!["FormOnMessage"]),
            ("ON-ERROR", vec!["FormOnError"]),
            ("KEY-EXIT", vec!["exit_form"]),
        ];

        for (trig_name, keywords) in common_form_triggers {
            if !triggers.iter().any(|t| t.level == TriggerLevel::Form && t.name == trig_name) {
                for kw in keywords {
                    let lower_text = text_latin1.to_ascii_lowercase();
                    if let Some(idx) = lower_text.find(&kw.to_ascii_lowercase()) {
                        let chunk = safe_slice(&text_latin1, idx.saturating_sub(200), idx + 800);
                        let code = find_code_in_chunk(chunk);
                        if code.len() > 20 && is_readable_text(&code) {
                            let text_ref = sidecars.write(
                                PlsqlSidecars::TRIGGERS,
                                &format!("FORM.{}", trig_name),
                                &code,
                            )?;
                            triggers.push(TriggerInfo {
                    execution_hierarchy: None,
                                name: trig_name.to_string(),
                                level: TriggerLevel::Form,
                                block_name: None,
                                item_name: None,
                                first_line: first_code_line(&code),
                                source_text: Some(code.clone()),
                                line_count: line_count_of(&code),
                                inherited: None,
                                text_encoding: TextEncoding::Original,
                                text_ref: Some(text_ref),
                                xml_ref: None,
                            });
                            break;
                        }
                    }
                }
            }
        }

        // 5. Discover Additional Blocks and Items from Bind Variables (:BLOCK.ITEM)
        let bind_rx = Regex::new(r":([A-Za-z0-9_$]+)\.([A-Za-z0-9_$]+)").unwrap();
        for cap in bind_rx.captures_iter(&text_latin1) {
            let b = cap.get(1).unwrap().as_str().to_ascii_uppercase();
            let itm = cap.get(2).unwrap().as_str().to_ascii_uppercase();
            if !matches!(b.as_str(), "GLOBAL" | "SYSTEM" | "PARAMETER" | "V") && (2..=32).contains(&itm.len()) {
                discovered_items_by_block.entry(b).or_default().insert(itm);
            }
        }

        // Also check GO_BLOCK('...')
        let go_block_rx = Regex::new(r"(?i)GO_BLOCK\s*\(\s*[']([A-Za-z0-9_$]+)[']").unwrap();
        for cap in go_block_rx.captures_iter(&text_latin1) {
            let b = cap.get(1).unwrap().as_str().to_ascii_uppercase();
            discovered_items_by_block.entry(b).or_default();
        }

        // 6. Extract Embedded SQL Queries for Record Groups without regex lookahead
        let select_rx = Regex::new(r"(?i)\bSELECT\s+").unwrap();
        let ws_rx = Regex::new(r"\s+").unwrap();
        let mut record_groups = Vec::new();
        let mut query_count = 0;

        for m in select_rx.find_iter(&text_latin1) {
            if query_count >= 20 {
                break;
            }
            let start = m.start();
            let slice = safe_slice(&text_latin1, start, start + 2000);

            // Find delimiter: ';', '\0', or "\n\n"
            let mut end_offset = slice.len();
            if let Some(pos) = slice.find(';') {
                end_offset = end_offset.min(pos + 1);
            }
            if let Some(pos) = slice.find('\0') {
                end_offset = end_offset.min(pos);
            }
            if let Some(pos) = slice.find("\n\n") {
                end_offset = end_offset.min(pos);
            }

            let candidate = &slice[..end_offset];
            if candidate.to_ascii_uppercase().contains(" FROM ") {
                let sql = ws_rx.replace_all(candidate.trim(), " ").to_string();
                if sql.len() > 25 && is_readable_text(&sql) {
                    query_count += 1;
                    let rg_name = format!("RG_QUERY_{}", query_count);
                    record_groups.push(RecordGroupInfo {
                        name: rg_name,
                        query_text: Some(sql),
                        columns: Vec::new(),
                        ..Default::default()
                    });
                }
            }
        }

        // 7. Direct ROS 60050 Full Module Decoding, falling back to regex/2-column auto-flow
        if key.module_type == ModuleType::Library {
            if let Some(pll_units) = decode_ros60050_pll(&bytes) {
                for mut pu in pll_units {
                    let type_name = format!("{:?}", pu.unit_type).to_ascii_uppercase();
                    if let Some(body) = pu.source_text.as_deref() {
                        if let Ok(tref) = sidecars.write(
                            PlsqlSidecars::PROGRAM_UNITS,
                            &format!("{}.{}", pu.name, type_name),
                            body,
                        ) {
                            pu.text_ref = Some(tref);
                        }
                    }
                    if let Some(existing) = program_units.iter_mut().find(|p| p.name.eq_ignore_ascii_case(&pu.name)) {
                        *existing = pu;
                    } else {
                        program_units.push(pu);
                    }
                }
            }
        }
        let decoded_ros = decode_ros60050_module(&bytes);
        let (
            blocks,
            windows,
            canvases,
            graphics,
            attached_libraries,
            lovs,
            ros_record_groups,
            alerts,
            parameters,
            visual_attributes,
            property_classes,
            property_class_details,
            editors,
            menu_module,
            menus,
            object_library_tabs,
            coordinate,
            title,
            console_window,
            first_navigation_block_name,
            runtime_comp,
            object_groups,
            extra_attrs,
        ) = if let Some(mut ros_mod) = decoded_ros {
            // Use ROS-decoded triggers directly (never merge regex-scraped binary fragments)
            triggers.clear();
            for mut trig in ros_mod.triggers {
                let scope = match (&trig.block_name, &trig.item_name) {
                    (Some(b), Some(i)) => format!("{}.{}", b, i),
                    (Some(b), None) => b.clone(),
                    _ => "FORM".to_string(),
                };
                let sig = format!("{}.{}", scope, trig.name);
                if let Some(body) = trig.source_text.as_deref() {
                    if !body.trim().is_empty() {
                        if let Ok(tref) = sidecars.write(PlsqlSidecars::TRIGGERS, &sig, body) {
                            trig.text_ref = Some(tref);
                        }
                    }
                }
                triggers.push(trig);
            }

            for pc in &mut ros_mod.property_class_details {
                for trig in &mut pc.triggers {
                    let sig = format!("{}.{}", pc.name, trig.name);
                    if let Some(body) = trig.source_text.as_deref() {
                        if !body.trim().is_empty() {
                            if let Ok(tref) = sidecars.write(PlsqlSidecars::TRIGGERS, &sig, body) {
                                trig.text_ref = Some(tref);
                            }
                        }
                    }
                }
            }

            // Use ROS-decoded program units directly
            program_units.clear();
            for mut pu in ros_mod.program_units {
                let type_name = format!("{:?}", pu.unit_type).to_ascii_uppercase();
                if let Some(body) = pu.source_text.as_deref() {
                    if !body.trim().is_empty() {
                        if let Ok(tref) = sidecars.write(
                            PlsqlSidecars::PROGRAM_UNITS,
                            &format!("{}.{}", pu.name, type_name),
                            body,
                        ) {
                            pu.text_ref = Some(tref);
                        }
                    }
                }
                program_units.push(pu);
            }

            for menu in &mut ros_mod.menus {
                for mi in &mut menu.items {
                    if let Some(body) = mi.command_text.as_deref() {
                        if !body.trim().is_empty() {
                            if let Ok(tref) = sidecars.write(
                                PlsqlSidecars::MENU_ITEMS,
                                &format!("{}.{}", menu.name, mi.name),
                                body,
                            ) {
                                mi.command_ref = Some(tref);
                            }
                        }
                    }
                }
            }

            let has_positioned_item = ros_mod
                .blocks
                .iter()
                .flat_map(|b| b.items.iter())
                .any(|i| i.x_position > 0 || i.y_position > 0 || i.width.unwrap_or(0) > 0);
            let use_synthetic = ros_mod.canvases.is_empty()
                && ros_mod.menus.is_empty()
                && ros_mod.object_library_tabs.is_empty()
                && !has_positioned_item;

            if use_synthetic {
                let synth = generate_2column_layout(&key.name, &discovered_items_by_block, &text_latin1);
                ros_mod.blocks = synth.blocks;
                ros_mod.windows = synth.windows;
                ros_mod.canvases = synth.canvases;
                ros_mod.graphics = synth.graphics;
            }

            (
                ros_mod.blocks,
                ros_mod.windows,
                ros_mod.canvases,
                ros_mod.graphics,
                ros_mod.attached_libraries,
                ros_mod.lovs,
                ros_mod.record_groups,
                ros_mod.alerts,
                ros_mod.parameters,
                ros_mod.visual_attributes,
                ros_mod.property_classes,
                ros_mod.property_class_details,
                ros_mod.editors,
                ros_mod.menu_module,
                ros_mod.menus,
                ros_mod.object_library_tabs,
                ros_mod.coordinate,
                ros_mod.title,
                ros_mod.console_window,
                ros_mod.first_navigation_block_name,
                ros_mod.runtime_comp,
                ros_mod.object_groups,
                ros_mod.extra_attrs,
            )
        } else {
            let layout = generate_2column_layout(&key.name, &discovered_items_by_block, &text_latin1);
            (
                layout.blocks,
                layout.windows,
                layout.canvases,
                layout.graphics,
                Vec::new(),
                Vec::new(),
                record_groups,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                None,
                Vec::new(),
                Vec::new(),
                CoordinateInfo::default(),
                None,
                None,
                None,
                None,
                Vec::new(),
                Default::default(),
            )
        };

        let fingerprint = fingerprint_file(source_file).unwrap_or_default();
        let file_stem = source_file.file_name().and_then(|n| n.to_str()).unwrap_or("module.fmb");
        let converted_file = format!("direct-binary://{}", file_stem);

        use std::time::SystemTime;
        let now = SystemTime::now();
        let dur = now.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
        let parsed_at = format!("{}Z", dur.as_secs());

        Ok(ModuleIndex {
            key: key.clone(),
            forms_version: Some(forms_version),
            index_version: oracle_forms_core::CURRENT_INDEX_VERSION,
            source_file: source_file.to_string_lossy().to_string(),
            fingerprint,
            converted_file,
            parsed_at,
            coordinate,
            blocks,
            triggers,
            program_units,
            attached_libraries,
            lovs,
            record_groups: ros_record_groups,
            windows,
            canvases,
            graphics,
            alerts,
            parameters,
            visual_attributes,
            visual_attribute_details: Vec::new(),
            property_classes,
            property_class_details,
            editors,
            menu_module,
            menus,
            object_library_tabs,
            object_refs: Vec::new(),
            first_navigation_block_name,
            runtime_comp,
            title,
            console_window,
            object_groups,
            form_reports: Vec::new(),
            extra_attrs,
            extra_children: Vec::new(),
        })
    }
}

fn safe_slice(s: &str, mut start: usize, mut end: usize) -> &str {
    start = start.min(s.len());
    end = end.min(s.len()).max(start);
    while start > 0 && !s.is_char_boundary(start) {
        start -= 1;
    }
    while end < s.len() && !s.is_char_boundary(end) {
        end += 1;
    }
    &s[start..end]
}

fn find_code_in_chunk(chunk: &str) -> String {
    let plsql_block_rx = Regex::new(r"(?is)\b(DECLARE|BEGIN)\b.*?\bEND\s*;").unwrap();
    let mut last_match = "";
    for m in plsql_block_rx.find_iter(chunk) {
        last_match = m.as_str().trim();
    }
    if !last_match.is_empty() {
        return clean_code_string(last_match);
    }

    // Fallback: search for lines containing semicolons
    let lines: Vec<&str> = chunk
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && l.contains(';'))
        .collect();

    if !lines.is_empty() {
        lines.join("\n")
    } else {
        String::new()
    }
}

fn extract_complete_code_block(chunk: &str) -> String {
    let upper = chunk.to_ascii_uppercase();
    if let Some(end_idx) = upper.find("END;") {
        let candidate = &chunk[0..end_idx + 4];
        return clean_code_string(candidate.trim());
    }

    let end_named_rx = Regex::new(r"(?i)\bEND\s+[A-Za-z0-9_$]+\s*;").unwrap();
    if let Some(m) = end_named_rx.find(chunk) {
        return clean_code_string(chunk[0..m.end()].trim());
    }

    let limit = 300.min(chunk.len());
    let sub = safe_slice(chunk, 0, limit);
    clean_code_string(sub)
}

fn clean_code_string(raw: &str) -> String {
    let ctrl_re_start = Regex::new(r"^[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\xff]+").unwrap();
    let ctrl_re_end = Regex::new(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f-\xff]+$").unwrap();

    raw.lines()
        .map(|l| {
            let s1 = ctrl_re_start.replace(l, "");
            ctrl_re_end.replace(&s1, "").to_string()
        })
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_readable_text(s: &str) -> bool {
    if s.trim().is_empty() {
        return false;
    }
    let printable = s.chars().filter(|&c| (' '..='~').contains(&c) || c == '\n' || c == '\r' || c == '\t').count();
    (printable as f64 / s.len() as f64) > 0.90
}
