//! Purpose: Oracle Reports (.rdf) binary parser and Report XML serializer.
//! Key Input Dependencies: oracle_forms_core
//! Index: parse_rdf_report, report_to_xml, write_rdf_report, xml_esc, extract_select_sql, extract_prefixed_ident, extract_title, columns_from_select

use std::fs;
use std::path::Path;
use oracle_forms_core::error::{FormsError, Result};
use oracle_forms_core::report::{
    ReportDataItem, ReportDataModel, ReportDataSource, ReportDefinition, ReportGroup,
    ReportLayout, ReportLayoutNode, ReportSection,
};

const PAGE_SIZE: usize = 4096;

/// Parses a binary Oracle Reports `.rdf` (`ROS.60050`) file into a `ReportDefinition`.
pub fn parse_rdf_report(path: &Path) -> Result<ReportDefinition> {
    let bytes = fs::read(path)?;
    if bytes.len() < 0x1000 || !bytes.starts_with(b"ROS.") {
        return Err(FormsError::InvalidFormat("Not a valid ROS.60050 .rdf binary".to_string()));
    }

    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("REPORT")
        .trim_start_matches("orig_")
        .to_ascii_uppercase();
    let text: String = bytes.iter().map(|&b| b as char).collect();

    let select_sql = extract_select_sql(&text).unwrap_or_default();
    let ds_name = extract_prefixed_ident(&text, "Q_").unwrap_or_else(|| "Q_1".to_string());
    let grp_name = extract_prefixed_ident(&text, "G_").unwrap_or_else(|| {
        ds_name
            .strip_prefix("Q_")
            .map(|s| format!("G_{}", s))
            .unwrap_or_else(|| "G_1".to_string())
    });

    let title = extract_title(&text, &name);
    let data_items = columns_from_select(&select_sql);

    let data_sources = if select_sql.is_empty() {
        Vec::new()
    } else {
        vec![ReportDataSource {
            name: ds_name,
            select_sql,
            groups: vec![ReportGroup {
                name: grp_name,
                data_items,
                summaries: Vec::new(),
                formulas: Vec::new(),
            }],
        }]
    };

    Ok(ReportDefinition {
        name,
        title,
        dtd_version: Some("9.0.4.0.0".to_string()),
        unit_of_measurement: Some("inch".to_string()),
        data: ReportDataModel {
            data_sources,
            links: Vec::new(),
            parameters: Vec::new(),
        },
        layout: ReportLayout {
            sections: vec![ReportSection {
                name: Some("MAIN".to_string()),
                body: Vec::new(),
            }],
        },
    })
}

/// Serializes a `ReportDefinition` to Oracle Reports XML.
pub fn report_to_xml(report: &ReportDefinition) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let dtd = report.dtd_version.as_deref().unwrap_or("9.0.4.0.0");
    let uom = report.unit_of_measurement.as_deref().unwrap_or("inch");
    out.push_str(&format!(
        "<report DTDVersion=\"{}\" name=\"{}\"",
        xml_esc(dtd),
        xml_esc(&report.name)
    ));
    if let Some(ref t) = report.title {
        out.push_str(&format!(" title=\"{}\"", xml_esc(t)));
    }
    out.push_str(&format!(" unitOfMeasurement=\"{}\">\n", xml_esc(uom)));
    out.push_str("  <data>\n");
    for ds in &report.data.data_sources {
        out.push_str(&format!("    <dataSource name=\"{}\">\n", xml_esc(&ds.name)));
        out.push_str(&format!("      <select>{}</select>\n", xml_esc(&ds.select_sql)));
        for grp in &ds.groups {
            out.push_str(&format!("      <group name=\"{}\">\n", xml_esc(&grp.name)));
            for item in &grp.data_items {
                out.push_str(&format!(
                    "        <dataItem name=\"{}\" datatype=\"{}\" width=\"{}\" />\n",
                    xml_esc(&item.name),
                    xml_esc(&item.datatype),
                    item.width.unwrap_or(40)
                ));
            }
            out.push_str("      </group>\n");
        }
        out.push_str("    </dataSource>\n");
    }
    out.push_str("  </data>\n");
    out.push_str("  <layout>\n");
    for sec in &report.layout.sections {
        let sname = sec.name.as_deref().unwrap_or("MAIN");
        out.push_str(&format!("    <section name=\"{}\">\n      <body>\n", xml_esc(sname)));
        for node in &sec.body {
            if let ReportLayoutNode::RepeatingFrame(rf) = node {
                out.push_str(&format!(
                    "        <repeatingFrame name=\"{}\" source=\"{}\" />\n",
                    xml_esc(&rf.name),
                    xml_esc(&rf.source)
                ));
            }
        }
        out.push_str("      </body>\n    </section>\n");
    }
    out.push_str("  </layout>\n</report>\n");
    out
}

/// Writes a `ReportDefinition` into a `ROS.60050` `.rdf` binary container.
pub fn write_rdf_report(report: &ReportDefinition) -> Vec<u8> {
    let mut payload = Vec::new();
    let title = report.title.as_deref().unwrap_or(&report.name);
    payload.extend_from_slice(title.as_bytes());
    payload.push(0);
    for ds in &report.data.data_sources {
        payload.extend_from_slice(ds.name.as_bytes());
        payload.push(0);
        payload.extend_from_slice(ds.select_sql.as_bytes());
        payload.push(0);
        for grp in &ds.groups {
            payload.extend_from_slice(grp.name.as_bytes());
            payload.push(0);
        }
    }
    let xml = report_to_xml(report);
    payload.extend_from_slice(xml.as_bytes());
    payload.push(0);

    let data_pages = (payload.len() + PAGE_SIZE - 1).max(1) / PAGE_SIZE;
    let total_pages = 1 + data_pages;
    let mut out = vec![0u8; total_pages * PAGE_SIZE];
    out[..9].copy_from_slice(b"ROS.60050");
    out[0x0c..0x10].copy_from_slice(&(total_pages as u32).to_be_bytes());
    out[0x14..0x18].copy_from_slice(&((total_pages * PAGE_SIZE) as u32).to_be_bytes());
    out[PAGE_SIZE..PAGE_SIZE + payload.len()].copy_from_slice(&payload);
    out
}

fn xml_esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn extract_select_sql(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let idx = text.find("SELECT ")?;
    let mut end = idx;
    while end < bytes.len() && end - idx < 2000 {
        let b = bytes[end];
        if b == 0 || (b < 32 && !matches!(b, b'\t' | b'\n' | b'\r')) || b >= 127 {
            break;
        }
        end += 1;
    }
    let candidate = text[idx..end].trim().to_string();
    candidate.to_ascii_uppercase().contains(" FROM ").then_some(candidate)
}

fn extract_prefixed_ident(text: &str, prefix: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut start = 0usize;
    while let Some(pos) = text[start..].find(prefix) {
        let abs = start + pos;
        let prev_ok = abs == 0 || !bytes[abs - 1].is_ascii_alphanumeric();
        if prev_ok {
            let mut end = abs + prefix.len();
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            if end > abs + prefix.len() {
                return Some(text[abs..end].to_string());
            }
        }
        start = abs + prefix.len();
    }
    None
}

fn extract_title(text: &str, name: &str) -> Option<String> {
    let marker = format!("{}_REPORT", name);
    if let Some(pos) = text.find(&marker) {
        let before = &text[pos.saturating_sub(80)..pos];
        let cleaned: String = before
            .chars()
            .rev()
            .skip_while(|&c| c == '\0')
            .take_while(|&c| c.is_ascii_graphic() || c == ' ')
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        let trimmed = cleaned.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn columns_from_select(sql: &str) -> Vec<ReportDataItem> {
    let upper = sql.to_ascii_uppercase();
    let Some(sel) = upper.find("SELECT ") else {
        return Vec::new();
    };
    let Some(frm) = upper[sel..].find(" FROM ") else {
        return Vec::new();
    };
    sql[sel + 7..sel + frm]
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|col| {
            let cname = col.split_whitespace().last().unwrap_or(col).to_ascii_uppercase();
            let datatype = if cname.ends_with("_ID") || cname == "SALARY" || cname.ends_with("_COUNT") {
                "number"
            } else if cname.ends_with("_DATE") {
                "date"
            } else {
                "character"
            };
            ReportDataItem {
                name: cname,
                datatype: datatype.to_string(),
                width: Some(40),
            }
        })
        .collect()
}
