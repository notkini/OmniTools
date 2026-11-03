//! Pure rename planning for Batch Rename — zero filesystem access, zero Tauri
//! types (PLAN §M5 tool 2). The planner consumes a list of file names (in the
//! exact displayed order) plus a sequence of operations, and emits one
//! proposal per file. All filesystem interaction — existence checks, actual
//! renames, undo — lives in `commands/rename.rs`; this module only ever
//! decides *what the names would become*.
//!
//! Safety rules encoded here:
//! - Windows-reserved device names (`CON`, `NUL`, `COM1`…) never survive.
//! - Illegal filename characters and control characters become `_`.
//! - Trailing dots and spaces are trimmed (invisible Windows breakage).
//! - Two files never plan to the same target when compared case-insensitively
//!   (Windows filesystems are case-insensitive; the rule applies on every
//!   platform so behaviour is identical in CI and on a real disk).
//! - A target may take *another* file's current name only while that other
//!   file is itself renamed away — the apply layer then vacates sources
//!   before writing targets.

use serde::{Deserialize, Serialize};

/// Case transforms (`case` operation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub enum CaseMode {
    Upper,
    Lower,
    Title,
}

/// One renaming operation. Wire format is a tagged camelCase union, e.g.
/// `{"type":"replace","find":"img","replaceWith":"photo","caseSensitive":false}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[allow(dead_code)]
pub enum RenameOperation {
    /// Prepend literal text to the stem.
    Prefix { value: String },
    /// Append literal text to the stem.
    Suffix { value: String },
    /// Text substitution inside the stem.
    Replace {
        find: String,
        replace_with: String,
        case_sensitive: bool,
    },
    /// Append a counter (per displayed order). `padding` = minimum digit
    /// width, zero-padded (`1` → `007` when padding is 3).
    Numbering { start: u64, step: u64, padding: u8 },
    /// Case transform on the stem.
    Case { mode: CaseMode },
    /// Set the extension (leading dots in `value` are stripped; an empty
    /// value removes the extension).
    Extension { value: String },
    /// Template replacing the stem; keeps the current extension. Tokens:
    /// `{n}` file number (1-based displayed order), `{orig}` original stem,
    /// `{date}` the caller-supplied date token (YYYYMMDD).
    Pattern { template: String },
}

/// What happened to one planned file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub enum ProposalKind {
    /// Rename proposed; free to apply.
    Ok,
    /// Cannot be applied — `reason` says why.
    Conflict,
    /// Target equals the current name exactly — nothing to do.
    Unchanged,
}

/// One per-file plan row, serialized camelCase over the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct RenameProposal {
    pub from_name: String,
    pub to_name: String,
    pub kind: ProposalKind,
    /// Empty for `unchanged` and clean `ok` rows; on `ok` it may carry a
    /// sanitization notice, on `conflict` it always carries the cause.
    #[serde(default)]
    pub reason: String,
}

/// Plans new names for `file_names` (displayed order matters: numbering and
/// `{n}` follow it). `date_token` is supplied by the caller (the command
/// shell) to keep this function pure and testable.
#[allow(dead_code)]
pub fn plan(
    operations: &[RenameOperation],
    file_names: &[String],
    date_token: &str,
) -> Vec<RenameProposal> {
    // Numbering counters live at plan level so they advance across files;
    // each `numbering` operation counts on its own.
    let mut counters: Vec<u64> = operations
        .iter()
        .map(|op| match op {
            RenameOperation::Numbering { start, .. } => *start,
            _ => 0,
        })
        .collect();

    let generated: Vec<(String, String)> = file_names
        .iter()
        .enumerate()
        .map(|(index, name)| generate_name(operations, name, index, date_token, &mut counters))
        .collect();

    let resolutions = resolve_targets(&generated, file_names);

    file_names
        .iter()
        .zip(&generated)
        .zip(&resolutions)
        .map(|((name, (to, generate_notice)), (kind, conflict_reason))| {
            let reason = conflict_reason
                .as_deref()
                .unwrap_or(generate_notice)
                .to_string();
            RenameProposal {
                from_name: name.clone(),
                to_name: to.clone(),
                kind: *kind,
                reason,
            }
        })
        .collect()
}

/// Shared decision logic over generated targets:
/// 1. exact equality with the current name ⇒ `unchanged`;
/// 2. case-insensitive duplicate targets ⇒ later rows (displayed order)
///    conflict with a "duplicate target" reason;
/// 3. target equal to another file's *current* name is only allowed when
///    that occupier is itself renamed away (target differs from its own
///    current name case-insensitively) — otherwise conflict.
///
/// Returns one `(kind, conflict_reason?)` per row.
#[allow(dead_code)]
pub(crate) fn resolve_targets(
    generated: &[(String, String)],
    file_names: &[String],
) -> Vec<(ProposalKind, Option<String>)> {
    // First owner of each (lowercased) target keeps the name.
    let mut first_owner: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for (index, (target, _)) in generated.iter().enumerate() {
        first_owner.entry(target.to_lowercase()).or_insert(index);
    }

    let mut result = Vec::with_capacity(file_names.len());
    for (index, name) in file_names.iter().enumerate() {
        let target = &generated[index].0;
        let from_lower = name.to_lowercase();
        let to_lower = target.to_lowercase();

        if from_lower == to_lower && name == target {
            result.push((ProposalKind::Unchanged, None));
            continue;
        }

        if let Some(&owner) = first_owner.get(&to_lower)
            && owner != index
        {
            result.push((
                ProposalKind::Conflict,
                Some(format!(
                    "duplicate target: '{}' plans the same name",
                    file_names[owner]
                )),
            ));
            continue;
        }

        // Occupied by another file's current name?
        let mut occupied_by: Option<&String> = None;
        for (other, other_current) in file_names.iter().enumerate() {
            if other == index || other_current.to_lowercase() != to_lower {
                continue;
            }
            let other_target_lower = generated[other].0.to_lowercase();
            if other_target_lower == other_current.to_lowercase() {
                occupied_by = Some(other_current);
            }
            break;
        }
        if let Some(occupier) = occupied_by {
            result.push((
                ProposalKind::Conflict,
                Some(format!("target name in use by '{occupier}'")),
            ));
            continue;
        }

        result.push((ProposalKind::Ok, None));
    }
    result
}

/// Applies one operation sequence to a single file name. Returns the new
/// full name plus a sanitization notice (empty when nothing was sanitized).
#[allow(dead_code)]
fn generate_name(
    operations: &[RenameOperation],
    original: &str,
    index: usize,
    date_token: &str,
    counters: &mut [u64],
) -> (String, String) {
    let (mut stem, mut ext) = split_stem_extension(original);

    for (op_index, op) in operations.iter().enumerate() {
        match op {
            RenameOperation::Prefix { value } => {
                stem = format!("{value}{stem}");
            }
            RenameOperation::Suffix { value } => {
                stem = format!("{stem}{value}");
            }
            RenameOperation::Replace {
                find,
                replace_with,
                case_sensitive,
            } => {
                stem = replace_in(&stem, find, replace_with, *case_sensitive);
            }
            RenameOperation::Numbering { step, padding, .. } => {
                let value = counters[op_index];
                counters[op_index] = value.saturating_add(*step);
                stem = format!("{stem}{}", format_number(value, *padding));
            }
            RenameOperation::Case { mode } => {
                stem = match mode {
                    CaseMode::Upper => stem.to_uppercase(),
                    CaseMode::Lower => stem.to_lowercase(),
                    CaseMode::Title => to_title_case(&stem),
                };
            }
            RenameOperation::Extension { value } => {
                ext = sanitize_extension(value);
            }
            RenameOperation::Pattern { template } => {
                stem = template
                    .replace("{n}", &format_number(index as u64 + 1, 0))
                    .replace("{orig}", &stem)
                    .replace("{date}", date_token);
            }
        }
    }

    let candidate = assemble_name(&stem, &ext);
    sanitize_result(&candidate)
}

/// Splits a file name at its last dot. A leading dot (hidden files such as
/// `.gitignore`) never counts as an extension separator; such names keep
/// everything in the stem.
#[allow(dead_code)]
fn split_stem_extension(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(0) | None => (name.to_string(), String::new()),
        Some(pos) => (name[..pos].to_string(), name[pos + 1..].to_string()),
    }
}

#[allow(dead_code)]
fn assemble_name(stem: &str, ext: &str) -> String {
    if ext.is_empty() {
        stem.to_string()
    } else {
        format!("{stem}.{ext}")
    }
}

/// Zero-pads `value` to `padding` digits when it is shorter.
#[allow(dead_code)]
fn format_number(value: u64, padding: u8) -> String {
    let digits = value.to_string();
    let width = padding as usize;
    if digits.len() >= width {
        digits
    } else {
        format!("{}{}", "0".repeat(width - digits.len()), digits)
    }
}

/// Case-sensitive or case-insensitive stem substitution. The insensitive
/// path matches per character windows (windows lowercased and compared with
/// the lowered needle), which stays correct when lowercasing changes byte
/// offsets — filenames are short, so the O(n·m) scan costs nothing.
#[allow(dead_code)]
fn replace_in(subject: &str, find: &str, replace_with: &str, case_sensitive: bool) -> String {
    if find.is_empty() {
        return subject.to_string();
    }
    if case_sensitive {
        return subject.replace(find, replace_with);
    }
    let find_chars: Vec<char> = find.chars().collect();
    let find_lower = find.to_lowercase();
    let subject_chars: Vec<char> = subject.chars().collect();
    let mut out = String::with_capacity(subject.len());
    let mut index = 0usize;
    while index < subject_chars.len() {
        if index + find_chars.len() <= subject_chars.len() {
            let window: String = subject_chars[index..index + find_chars.len()]
                .iter()
                .collect();
            if window.to_lowercase() == find_lower {
                out.push_str(replace_with);
                index += find_chars.len();
                continue;
            }
        }
        out.push(subject_chars[index]);
        index += 1;
    }
    out
}

/// Unicode-aware title case: uppercase after any non-alphanumeric boundary,
/// lowercase elsewhere.
#[allow(dead_code)]
fn to_title_case(subject: &str) -> String {
    let mut out = String::with_capacity(subject.len());
    let mut at_boundary = true;
    for ch in subject.chars() {
        if ch.is_alphanumeric() {
            if at_boundary {
                out.extend(ch.to_uppercase());
            } else {
                out.extend(ch.to_lowercase());
            }
            at_boundary = false;
        } else {
            out.push(ch);
            at_boundary = true;
        }
    }
    out
}

/// Strips leading dots from an extension value, sanitizes it, and trims
/// trailing dots/spaces. Empty input (or a value that sanitizes away)
/// removes the extension entirely.
#[allow(dead_code)]
fn sanitize_extension(value: &str) -> String {
    let stripped = value.trim_start_matches('.');
    strip_illegal(stripped)
        .trim_end_matches(['.', ' '])
        .to_string()
}

/// Characters Windows forbids in file names. Enforced on every platform so
/// behaviour is identical in CI and on a real Windows disk.
const ILLEGAL_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

#[allow(dead_code)]
fn strip_illegal(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ILLEGAL_CHARS.contains(&ch) || ch.is_control() {
                '_'
            } else {
                ch
            }
        })
        .collect()
}

/// Windows reserved device names (case-insensitive, as the stem or the whole
/// name).
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Final validation/sanitization pass over a candidate name. Returns the
/// (name, notice) pair; a name that cannot be made legal is returned empty
/// with an explanatory notice — callers surface the notice honestly.
#[allow(dead_code)]
fn sanitize_result(candidate: &str) -> (String, String) {
    let mut notice = String::new();

    let cleaned = strip_illegal(candidate);
    if cleaned != candidate {
        notice = "illegal characters replaced with '_'; ".to_string();
    }

    // Trailing dots/spaces are typeable but break Windows metadata.
    let trimmed = cleaned.trim_end_matches(['.', ' ']).to_string();
    if trimmed != cleaned && !trimmed.is_empty() {
        notice = format!("{notice}trailing dots/spaces removed; ");
    }

    if trimmed.is_empty() || trimmed.chars().all(|ch| ch == '.') {
        return (String::new(), "resulting name is empty".to_string());
    }

    // Reserved device names: the stem (before the first dot) is what Windows
    // treats specially; append an underscore to make it a normal file.
    let stem = trimmed.split('.').next().unwrap_or("");
    if RESERVED_NAMES
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        return (
            format!("{trimmed}_"),
            format!("{notice}reserved Windows name — '_' appended"),
        );
    }

    // 255 characters is the universal component limit (NTFS/ext4).
    if trimmed.chars().count() > 255 {
        return (
            String::new(),
            "resulting name too long (over 255)".to_string(),
        );
    }

    (trimmed, notice.trim_end_matches("; ").to_string())
}

/// True when two names equate on a case-insensitive filesystem.
#[allow(dead_code)]
pub(crate) fn case_insensitive_eq(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// Days since 1970-01-01 → date token `YYYYMMDD` for the `{date}` pattern
/// (proleptic Gregorian, Howard Hinnant's `civil_from_days`). Pure and testable;
/// the command shell supplies today's day count from the system clock.
#[allow(dead_code)]
pub fn date_token_from_days(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe.div_euclid(1460) + doe.div_euclid(36_524) - doe.div_euclid(146_096)) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = y + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}")
}

/// Day count for the system clock right now (UTC).
#[allow(dead_code)]
pub fn days_since_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| (duration.as_secs() / 86_400) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn plan_names(operations: &[RenameOperation], values: &[&str]) -> Vec<RenameProposal> {
        plan(operations, &names(values), "20250101")
    }

    // ---------- wire contracts ----------

    #[test]
    fn operation_serializes_camel_case_tagged() {
        let op = RenameOperation::Replace {
            find: "img".to_string(),
            replace_with: "photo".to_string(),
            case_sensitive: false,
        };
        let json = serde_json::to_value(&op).expect("serialize");
        assert_eq!(
            json,
            serde_json::json!({
                "type": "replace",
                "find": "img",
                "replaceWith": "photo",
                "caseSensitive": false
            })
        );
    }

    #[test]
    fn operation_deserializes_frontends_tagged_payload() {
        let json = r#"{"type":"numbering","start":1,"step":3,"padding":4}"#;
        let op: RenameOperation = serde_json::from_str(json).expect("deserialize");
        assert_eq!(
            op,
            RenameOperation::Numbering {
                start: 1,
                step: 3,
                padding: 4
            }
        );
    }

    #[test]
    fn proposal_serializes_camel_case() {
        let proposal = RenameProposal {
            from_name: "a.txt".to_string(),
            to_name: "b.txt".to_string(),
            kind: ProposalKind::Ok,
            reason: String::new(),
        };
        let json = serde_json::to_value(&proposal).expect("serialize");
        assert_eq!(json["fromName"], "a.txt");
        assert_eq!(json["toName"], "b.txt");
        assert_eq!(json["kind"], "ok");
        assert_eq!(json["reason"], "");
    }

    #[test]
    fn kind_wire_values_are_exact() {
        assert_eq!(
            serde_json::to_value(ProposalKind::Ok).unwrap(),
            serde_json::json!("ok")
        );
        assert_eq!(
            serde_json::to_value(ProposalKind::Conflict).unwrap(),
            serde_json::json!("conflict")
        );
        assert_eq!(
            serde_json::to_value(ProposalKind::Unchanged).unwrap(),
            serde_json::json!("unchanged")
        );
    }

    // ---------- basic operations ----------

    #[test]
    fn empty_operations_plan_unchanged() {
        let proposals = plan_names(&[], &["a.txt", "b.jpg"]);
        assert!(proposals.iter().all(|p| p.kind == ProposalKind::Unchanged));
        assert_eq!(proposals[0].to_name, "a.txt");
    }

    #[test]
    fn prefix_and_suffix_compose() {
        let proposals = plan_names(
            &[
                RenameOperation::Prefix {
                    value: "IMG_".to_string(),
                },
                RenameOperation::Suffix {
                    value: "_copy".to_string(),
                },
            ],
            &["holiday.png"],
        );
        assert_eq!(proposals[0].to_name, "IMG_holiday_copy.png");
        assert_eq!(proposals[0].kind, ProposalKind::Ok);
        assert_eq!(proposals[0].reason, "");
    }

    #[test]
    fn replace_case_sensitive_vs_insensitive() {
        let sensitive = plan_names(
            &[RenameOperation::Replace {
                find: "IMG".to_string(),
                replace_with: "photo".to_string(),
                case_sensitive: true,
            }],
            &["IMG_0001.jpg", "img_0002.jpg"],
        );
        assert_eq!(sensitive[0].to_name, "photo_0001.jpg");
        assert_eq!(sensitive[1].to_name, "img_0002.jpg", "case-sensitive miss");

        let loose = plan_names(
            &[RenameOperation::Replace {
                find: "img".to_string(),
                replace_with: "photo".to_string(),
                case_sensitive: false,
            }],
            &["IMG_0001.jpg"],
        );
        assert_eq!(loose[0].to_name, "photo_0001.jpg");
    }

    #[test]
    fn numbering_increments_per_displayed_order() {
        let proposals = plan_names(
            &[RenameOperation::Numbering {
                start: 5,
                step: 10,
                padding: 3,
            }],
            &["a.png", "b.png", "c.png"],
        );
        let targets: Vec<String> = proposals.iter().map(|p| p.to_name.clone()).collect();
        assert_eq!(targets, vec!["a005.png", "b015.png", "c025.png"]);
    }

    #[test]
    fn numbering_without_padding_is_plain() {
        let proposals = plan_names(
            &[RenameOperation::Numbering {
                start: 1,
                step: 1,
                padding: 0,
            }],
            &["a.txt", "b.txt"],
        );
        assert_eq!(proposals[0].to_name, "a1.txt");
        assert_eq!(proposals[1].to_name, "b2.txt");
    }

    #[test]
    fn two_numbering_operations_count_independently() {
        let proposals = plan_names(
            &[
                RenameOperation::Numbering {
                    start: 1,
                    step: 1,
                    padding: 0,
                },
                RenameOperation::Numbering {
                    start: 100,
                    step: 1,
                    padding: 0,
                },
            ],
            &["a.txt"],
        );
        // First op appends "1", second appends "100".
        assert_eq!(proposals[0].to_name, "a1100.txt");
    }

    #[test]
    fn case_modes_transform_stem_only() {
        let upper = plan_names(
            &[RenameOperation::Case {
                mode: CaseMode::Upper,
            }],
            &["My Photo.PNG"],
        );
        assert_eq!(upper[0].to_name, "MY PHOTO.PNG", "extension untouched");

        let title = plan_names(
            &[RenameOperation::Case {
                mode: CaseMode::Title,
            }],
            &["my holiday-PHOTO report.txt"],
        );
        assert_eq!(title[0].to_name, "My Holiday-Photo Report.txt");

        let lower = plan_names(
            &[RenameOperation::Case {
                mode: CaseMode::Lower,
            }],
            &["MYFILE.TXT"],
        );
        assert_eq!(lower[0].to_name, "myfile.TXT");
    }

    #[test]
    fn extension_value_strips_dots_and_sanitizes() {
        let set = plan_names(
            &[RenameOperation::Extension {
                value: ".jpeg".to_string(),
            }],
            &["a.png", "b"],
        );
        assert_eq!(set[0].to_name, "a.jpeg");
        assert_eq!(set[1].to_name, "b.jpeg");

        let clear = plan_names(
            &[RenameOperation::Extension {
                value: String::new(),
            }],
            &["a.png"],
        );
        assert_eq!(clear[0].to_name, "a");
    }

    #[test]
    fn pattern_tokens_expand() {
        let proposals = plan_names(
            &[RenameOperation::Pattern {
                template: "{orig}_{n}_{date}".to_string(),
            }],
            &["holiday photo.png", "holiday photo (1).png"],
        );
        assert_eq!(proposals[0].to_name, "holiday photo_1_20250101.png");
        assert_eq!(
            proposals[1].to_name, "holiday photo (1)_2_20250101.png",
            "extension kept; tokens grow per displayed order"
        );
    }

    #[test]
    fn pattern_keeps_extension_and_leaves_unknown_tokens() {
        let known = plan_names(
            &[RenameOperation::Pattern {
                template: "pic_{n}".to_string(),
            }],
            &["a.jpeg"],
        );
        assert_eq!(known[0].to_name, "pic_1.jpeg");

        let unknown = plan_names(
            &[RenameOperation::Pattern {
                template: "{weird}".to_string(),
            }],
            &["a.png"],
        );
        assert_eq!(unknown[0].to_name, "{weird}.png");
    }

    // ---------- Windows safety: sanitization, reserved names ----------

    #[test]
    fn illegal_characters_are_sanitized_with_notice() {
        let proposals = plan_names(
            &[RenameOperation::Prefix {
                value: "a?b<>".to_string(),
            }],
            &["file.txt"],
        );
        assert_eq!(proposals[0].to_name, "a_b__file.txt");
        assert!(proposals[0].reason.contains("illegal characters"));
        assert_eq!(proposals[0].kind, ProposalKind::Ok);
    }

    #[test]
    fn trailing_dots_and_spaces_trimmed() {
        // Clearing the extension of "my...file" (a trailing-dotted stem)
        // triggers the Windows footgun; trimming fixes it silently.
        let with_dots = plan_names(
            &[RenameOperation::Extension {
                value: String::new(),
            }],
            &["my...file"],
        );
        assert_eq!(with_dots[0].to_name, "my");
        assert!(with_dots[0].reason.contains("trailing"));

        // Trailing spaces get the same treatment.
        let with_space = plan_names(
            &[RenameOperation::Suffix {
                value: " ".to_string(),
            }],
            &["file"],
        );
        assert_eq!(with_space[0].to_name, "file");
        assert!(with_space[0].reason.contains("trailing"));

        // Dots *mid-stem* are never touched.
        let interior = plan_names(
            &[RenameOperation::Extension {
                value: "txt".to_string(),
            }],
            &["my...file"],
        );
        assert_eq!(interior[0].to_name, "my...txt");
    }

    #[test]
    fn control_characters_are_sanitized() {
        let proposals = plan_names(
            &[RenameOperation::Prefix {
                value: "\u{7}".to_string(),
            }],
            &["f.txt"],
        );
        assert_eq!(proposals[0].to_name, "_f.txt");
    }

    #[test]
    fn windows_reserved_names_get_trailing_underscore() {
        for name in ["CON", "nul", "Com1", "LPT9", "aux"] {
            let proposals = plan_names(
                &[RenameOperation::Extension {
                    value: "txt".to_string(),
                }],
                &[name],
            );
            assert_eq!(
                proposals[0].to_name,
                format!("{name}.txt_"),
                "reserved: {name}"
            );
            assert!(proposals[0].reason.contains("reserved"));
        }
        // Without an extension, the underscore lands at the end — and even a
        // no-op plan sanitizes a bare reserved name (safer to rename it).
        let bare = plan_names(&[], &["CON"]);
        assert_eq!(bare[0].to_name, "CON_");
        assert_eq!(bare[0].kind, ProposalKind::Ok);
        assert!(bare[0].reason.contains("reserved"));

        // Reserved only as a prefix of a longer stem: "concert.txt" is NOT
        // reserved (Windows treats the whole stem).
        let long = plan_names(&[], &["concert.txt"]);
        assert_eq!(long[0].kind, ProposalKind::Unchanged);
    }

    #[test]
    fn unsanitizable_results_report_an_empty_name_and_reason() {
        // 300-char stem: too long → rejected with reason, empty name.
        let long_stem: String = "f".repeat(300);
        let proposals = plan_names(&[], &[long_stem.as_str()]);
        assert_eq!(proposals[0].to_name, String::new());
        assert!(proposals[0].reason.contains("too long"));

        // Direct sanitize_result behaviour for the all-dots case.
        let (name, notice) = sanitize_result("... ...");
        assert_eq!(name, "");
        assert!(notice.contains("empty"));
    }

    // ---------- conflicts ----------

    #[test]
    fn duplicate_targets_conflict_for_later_rows_only() {
        let proposals = plan_names(
            &[RenameOperation::Pattern {
                template: "same".to_string(),
            }],
            &["a.txt", "b.txt"],
        );
        assert_eq!(proposals[0].kind, ProposalKind::Ok);
        assert_eq!(proposals[0].to_name, "same.txt");
        assert_eq!(proposals[1].kind, ProposalKind::Conflict);
        assert!(proposals[1].reason.contains("duplicate target"));
        assert!(proposals[1].reason.contains("'a.txt'"));
    }

    #[test]
    fn duplicate_targets_detected_case_insensitively() {
        // Targets "photo.txt" vs "PHOTO.txt" — identical on a case-insensitive
        // filesystem although the strings differ; the earlier row wins.
        let generated = vec![
            ("photo.txt".to_string(), String::new()),
            ("PHOTO.txt".to_string(), String::new()),
        ];
        let file_names = names(&["a.txt", "b.txt"]);
        let resolutions = resolve_targets(&generated, &file_names);
        assert_eq!(resolutions[0], (ProposalKind::Ok, None));
        assert_eq!(resolutions[1].0, ProposalKind::Conflict);
        assert!(
            resolutions[1]
                .1
                .as_deref()
                .unwrap()
                .contains("duplicate target")
        );
    }

    #[test]
    fn target_equal_to_unchanged_neighbors_name_conflicts() {
        // Row 0 plans "b.txt" while row 1 keeps "b.txt" unchanged: occupied.
        let proposals = plan_names(
            &[RenameOperation::Pattern {
                template: "b".to_string(),
            }],
            &["a.txt", "b.txt"],
        );
        assert_eq!(proposals[0].to_name, "b.txt");
        assert_eq!(proposals[0].kind, ProposalKind::Conflict);
        assert!(
            proposals[0]
                .reason
                .contains("target name in use by 'b.txt'")
        );
        assert_eq!(proposals[1].kind, ProposalKind::Unchanged);
    }

    #[test]
    fn target_may_take_a_name_being_vacated() {
        // Row 0's target equals row 1's *current* name, but row 1 itself
        // renames away — allow it (apply layer vacates sources first).
        let generated = vec![
            ("b.txt".to_string(), String::new()),
            ("c.txt".to_string(), String::new()),
        ];
        let file_names = names(&["a.txt", "b.txt"]);
        let resolutions = resolve_targets(&generated, &file_names);
        assert_eq!(resolutions[0], (ProposalKind::Ok, None));
        assert_eq!(resolutions[1], (ProposalKind::Ok, None));
    }

    #[test]
    fn full_swap_pair_of_names_is_free_of_conflicts() {
        // Two files exchanging names via two-step renames: neither planner
        // row conflicts; the swap happens through temp names at apply.
        let generated = vec![
            ("b.txt".to_string(), String::new()),
            ("a.txt".to_string(), String::new()),
        ];
        let file_names = names(&["a.txt", "b.txt"]);
        let resolutions = resolve_targets(&generated, &file_names);
        assert_eq!(resolutions[0], (ProposalKind::Ok, None));
        assert_eq!(resolutions[1], (ProposalKind::Ok, None));
    }

    #[test]
    fn case_only_own_rename_is_ok_not_unchanged() {
        let stem_swap = plan(
            &[RenameOperation::Case {
                mode: CaseMode::Lower,
            }],
            &names(&["Holiday.PNG"]),
            "20250101",
        );
        assert_eq!(stem_swap[0].to_name, "holiday.PNG");
        assert_eq!(stem_swap[0].kind, ProposalKind::Ok);

        // Full-name case-only swap: still ok — apply does the two-step.
        let full_swap = plan(
            &[RenameOperation::Case {
                mode: CaseMode::Lower,
            }],
            &names(&["HOLIDAY.png"]),
            "20250101",
        );
        assert_eq!(full_swap[0].to_name, "holiday.png");
        assert_eq!(full_swap[0].kind, ProposalKind::Ok);
    }

    #[test]
    fn taking_case_variant_of_a_unchanged_neighbors_name_conflicts() {
        // Row 0 (a.txt unchanged) occupies "a.txt"; row 1 (b.txt) targets
        // "A.txt" — same slot on a case-insensitive filesystem: conflict.
        let generated = vec![
            ("a.txt".to_string(), String::new()),
            ("A.txt".to_string(), String::new()),
        ];
        let file_names = names(&["a.txt", "b.txt"]);
        let resolutions = resolve_targets(&generated, &file_names);
        assert_eq!(resolutions[0], (ProposalKind::Unchanged, None));
        assert_eq!(
            resolutions[1].0,
            ProposalKind::Conflict,
            "case-variant of an occupied name must conflict"
        );
    }

    #[test]
    fn conflict_reasons_override_sanitization_notices() {
        // A later duplicate row that also carries a sanitization notice must
        // surface the conflict reason, not the cosmetic notice.
        let generated = vec![
            ("dup name".to_string(), String::new()),
            ("Dup name".to_string(), "illegal characters; ".to_string()),
        ];
        let file_names = names(&["a.txt", "b.txt"]);
        let resolutions = resolve_targets(&generated, &file_names);
        assert_eq!(resolutions[1].0, ProposalKind::Conflict);
        assert!(resolutions[1].1.as_deref().unwrap().contains("duplicate"));
    }

    #[test]
    fn operation_order_changes_result() {
        // Prefix after a lower-case transform keeps the uppercase "X";
        // prefix before it gets lowercased with the stem.
        let lower_first = plan_names(
            &[
                RenameOperation::Prefix {
                    value: "X".to_string(),
                },
                RenameOperation::Case {
                    mode: CaseMode::Lower,
                },
            ],
            &["Name.txt"],
        );
        let prefix_first = plan_names(
            &[
                RenameOperation::Case {
                    mode: CaseMode::Lower,
                },
                RenameOperation::Prefix {
                    value: "X".to_string(),
                },
            ],
            &["Name.txt"],
        );
        assert_eq!(lower_first[0].to_name, "xname.txt");
        assert_eq!(prefix_first[0].to_name, "Xname.txt");
    }

    // ---------- helpers ----------

    #[test]
    fn split_stem_extension_handles_hidden_files() {
        assert_eq!(
            split_stem_extension("report.v2.txt"),
            ("report.v2".into(), "txt".into())
        );
        assert_eq!(
            split_stem_extension(".gitignore"),
            (".gitignore".into(), "".into())
        );
        assert_eq!(
            split_stem_extension("archive.tar.gz"),
            ("archive.tar".into(), "gz".into())
        );
        assert_eq!(split_stem_extension("noext"), ("noext".into(), "".into()));
    }

    #[test]
    fn replace_in_handles_unicode_case_folding() {
        assert_eq!(
            replace_in("Café Éclair", "café", "bistro", false),
            "bistro Éclair"
        );
        assert_eq!(
            replace_in("ÉCLAIR", "é", "e", true),
            "ÉCLAIR",
            "case-sensitive miss keeps the subject whole"
        );
    }

    #[test]
    fn title_case_is_unicode_aware() {
        assert_eq!(to_title_case("éclair au chocolat"), "Éclair Au Chocolat");
        assert_eq!(to_title_case("über-cool"), "Über-Cool");
    }

    #[test]
    fn format_number_pads_and_overflows() {
        assert_eq!(format_number(7, 3), "007");
        assert_eq!(format_number(1234, 3), "1234");
        assert_eq!(format_number(0, 0), "0");
    }

    #[test]
    fn case_insensitive_eq_matches_lowercase_forms() {
        assert!(case_insensitive_eq("Report.TXT", "report.txt"));
        assert!(!case_insensitive_eq("report.txt", "report.tx"));
    }

    #[test]
    fn date_token_is_today_for_known_days() {
        assert_eq!(date_token_from_days(0), "19700101");
        assert_eq!(date_token_from_days(31), "19700201");
        assert_eq!(date_token_from_days(365), "19710101");
        assert_eq!(date_token_from_days(789), "19720229", "leap day");
        assert_eq!(date_token_from_days(11_017), "20000301");
        // Monotonicity across a few recent-ish day counts.
        let today = date_token_from_days(days_since_epoch());
        assert!(
            today.as_str() >= "20260101",
            "today's date token must be recent"
        );
    }
}
