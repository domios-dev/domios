mod diagnostics;
mod dominance;
mod engine;
mod facts;
pub mod fix;
pub mod ports;
mod rules;
mod slots;
#[cfg(test)]
mod tests;

use super::{projection::PositionedIr, Document, Parsed};
pub use diagnostics::{Diagnostic, Finding, Proof, ProofAtom, CALL_RULE_IDS};
use engine::{Atom, Database};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default)]
pub struct VerifiedCall {
    pub fixed_arguments: BTreeMap<String, String>,
    pub result_tainted: bool,
    pub expanded_steps: usize,
}
#[derive(Clone, Debug)]
pub struct DuplicateFinding {
    pub verdict: String,
    pub other: String,
    pub why: String,
}
#[derive(Clone, Debug)]
pub enum DuplicateStatus {
    Available(Vec<DuplicateFinding>),
    Unavailable(String),
    Partial {
        findings: Vec<DuplicateFinding>,
        reason: String,
    },
}
impl Default for DuplicateStatus {
    fn default() -> Self {
        Self::Unavailable("Scoped vector/WL comparison is unavailable in this offline check".into())
    }
}
pub trait DuplicateOracle {
    fn compare(&self, document: &Document) -> DuplicateStatus;
}
#[derive(Clone, Debug, Default)]
pub struct CheckContext {
    pub grants: BTreeSet<String>,
    pub calls: BTreeMap<String, VerifiedCall>,
    pub call_output_media: BTreeMap<String, String>,
    pub call_diagnostics: Vec<Diagnostic>,
    pub duplicates: DuplicateStatus,
}

pub fn check_ir(ir: &PositionedIr, context: &CheckContext) -> Vec<Diagnostic> {
    check_ir_on_platform(ir, context, std::env::consts::OS)
}

fn check_ir_on_platform(
    ir: &PositionedIr,
    context: &CheckContext,
    platform: &str,
) -> Vec<Diagnostic> {
    let parsed = Parsed {
        document: ir.canonical.document.clone(),
        locations: ir.locations.clone(),
    };
    if serde_json::to_vec(&parsed.document).map_or(true, |bytes| {
        bytes.len() > crate::blueprint_recipe::MAX_RECIPE_BYTES
    }) {
        return vec![Diagnostic::error(
            parsed.at(""),
            "",
            "Blueprint document exceeds 256 KiB",
            "Split the graph",
        )];
    }
    if let Err((field, message)) = ports::preflight(&parsed.document) {
        return vec![Diagnostic::error(
            parsed.at(&field),
            &field,
            &message,
            "Declare an acyclic port schema within the total expansion budget",
        )];
    }
    let mut db = facts::database(ir, context);
    if let Err(message) = db.evaluate(&rules::program()) {
        return vec![Diagnostic::error(
            parsed.at("steps"),
            "steps",
            &message,
            "Reduce the graph or correct the developer-owned rule program",
        )];
    }
    let mut found = Vec::new();
    for spec in rules::findings() {
        for args in db.rows(spec.predicate) {
            let fact = Atom::new(spec.predicate, args.clone());
            let Some(mut proof) = db.proof(&fact) else {
                continue;
            };
            let mut leaves = Vec::new();
            collect_leaves(&proof, &mut leaves);
            let (failing, passing): (Vec<_>, Vec<_>) = leaves
                .into_iter()
                .cloned()
                .partition(|leaf| leaf.present && spec.failing.contains(&leaf.predicate.as_str()));
            if let Some(source) = failing.first() {
                proof.atom.field = source.field.clone();
                proof.atom.location = source.location.clone();
            }
            found.push((
                fact,
                Diagnostic::from_proof(
                    Finding {
                        rule: spec.rule,
                        name: spec.predicate,
                        severity: spec.severity,
                        message: &substitute(spec.message, args),
                        hint: spec.hint,
                    },
                    proof,
                    failing,
                    passing,
                ),
            ));
        }
    }
    let mut diagnostics = cascade(&db, found);
    diagnostics.extend(dominance::check(&parsed, &db));
    for (field, message) in super::validate::document(&parsed.document) {
        diagnostics.push(Diagnostic::error(
            parsed.at(&field),
            &field,
            &message,
            "Use blueprint schema and correct this field",
        ));
    }
    fold_into_document_errors(&mut diagnostics);
    diagnostics.extend(memory_warnings(&parsed, platform));
    schema_errors(&parsed, &mut diagnostics);
    diagnostics.extend(context.call_diagnostics.clone());
    if let DuplicateStatus::Unavailable(reason) | DuplicateStatus::Partial { reason, .. } =
        &context.duplicates
    {
        let atom = ProofAtom::new(
            "duplicate_comparison_unavailable",
            vec![reason.clone()],
            true,
            "",
            parsed.at(""),
        );
        diagnostics.push(Diagnostic::from_proof(Finding { rule:"R13",name:"duplicate_comparison_unavailable",severity:"info",message:reason,hint:"Cross-Blueprint duplicate checking requires the scoped vector/WL service; no lexical fallback is used" },Proof { atom:atom.clone(),rule:"capability".into(),children:Vec::new() },Vec::new(),vec![atom]));
    }
    diagnostics.sort_by(|a, b| {
        (
            severity(&a.severity),
            &a.file,
            a.line,
            a.column,
            &a.rule,
            &a.id,
        )
            .cmp(&(
                severity(&b.severity),
                &b.file,
                b.line,
                b.column,
                &b.rule,
                &b.id,
            ))
    });
    diagnostics.dedup_by(|a, b| a.id == b.id);
    diagnostics
}
fn fold_into_document_errors(diagnostics: &mut Vec<Diagnostic>) {
    let (mut roots, rest): (Vec<_>, Vec<_>) = std::mem::take(diagnostics)
        .into_iter()
        .partition(|d| d.rule == "IR");
    for diagnostic in rest {
        match roots
            .iter_mut()
            .find(|root| !root.field.is_empty() && root.field == diagnostic.field)
        {
            Some(root) => root.folded.push(diagnostic),
            None => diagnostics.push(diagnostic),
        }
    }
    diagnostics.append(&mut roots);
}
fn memory_warnings(parsed: &Parsed, platform: &str) -> Vec<Diagnostic> {
    if matches!(platform, "linux" | "windows") {
        return Vec::new();
    }
    let mut warnings = Vec::new();
    for (index, node) in parsed.document.nodes().enumerate() {
        let Some(code) = &node.code else { continue };
        let path = slots::node_path(&parsed.document, index);
        let fields = [
            (code.limits.mem_mb, format!("{path}.code.limits.mem_mb")),
            (
                node.limits.as_ref().and_then(|limits| limits.mem_mb),
                format!("{path}.limits.mem_mb"),
            ),
        ];
        for (limit, field) in fields {
            if limit.is_none() {
                continue;
            }
            let mut warning = Diagnostic::error(
                parsed.at(&field),
                &field,
                "mem_mb not enforced on this OS",
                "The node runs with its deadline and output budgets; no memory cap is applied",
            );
            warning.severity = "warning".into();
            warnings.push(warning);
        }
    }
    warnings
}

pub fn check_document(document: &Document, context: &CheckContext) -> Vec<Diagnostic> {
    check_ir(
        &PositionedIr {
            canonical: super::projection::canonical(document),
            locations: BTreeMap::new(),
        },
        context,
    )
}
pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|d| d.severity == "error" || has_errors(&d.folded))
}
fn severity(s: &str) -> u8 {
    match s {
        "error" => 0,
        "warning" => 1,
        _ => 2,
    }
}
fn substitute(template: &str, args: &[String]) -> String {
    args.iter()
        .enumerate()
        .fold(template.into(), |text, (i, arg)| {
            text.replace(&format!("{{{i}}}"), arg)
        })
}
fn collect_leaves<'a>(proof: &'a Proof, leaves: &mut Vec<&'a ProofAtom>) {
    if proof.children.is_empty() {
        leaves.push(&proof.atom);
    } else {
        for child in &proof.children {
            collect_leaves(child, leaves);
        }
    }
}
fn cascade(db: &Database, found: Vec<(Atom, Diagnostic)>) -> Vec<Diagnostic> {
    let dependencies = found.iter().map(|(f, _)| db.support(f)).collect::<Vec<_>>();
    let roots = found
        .iter()
        .enumerate()
        .map(|(i, _)| {
            found
                .iter()
                .enumerate()
                .filter(|(j, (f, d))| {
                    i != *j && d.severity == "error" && dependencies[i].contains(f)
                })
                .map(|(j, _)| j)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut folded: BTreeMap<usize, Vec<Diagnostic>> = BTreeMap::new();
    for (i, (_, diagnostic)) in found.iter().enumerate() {
        if roots[i].is_empty() {
            continue;
        }
        if let Some(root) = roots[i].iter().copied().find(|j| roots[*j].is_empty()) {
            folded.entry(root).or_default().push(diagnostic.clone());
        }
    }
    found
        .into_iter()
        .enumerate()
        .filter_map(|(i, (_, mut d))| {
            if !roots[i].is_empty() {
                return None;
            }
            d.folded = folded.remove(&i).unwrap_or_default();
            Some(d)
        })
        .collect()
}
fn schema_errors(parsed: &Parsed, diagnostics: &mut Vec<Diagnostic>) {
    let doc = &parsed.document;
    for (i, node) in doc.nodes().enumerate() {
        let path = slots::node_path(doc, i);
        for (direction, port) in [("in", &node.input), ("out", &node.output)] {
            if let Some(port) = port {
                if let Err(message) = ports::resolve(port, &doc.types) {
                    let field = format!("{path}.{direction}");
                    if !diagnostics
                        .iter()
                        .any(|d| d.field == field && d.message == message)
                    {
                        diagnostics.push(Diagnostic::error(
                            parsed.at(&field),
                            &field,
                            &message,
                            "Declare an acyclic bounded port schema",
                        ));
                    }
                }
            }
        }
        for (slot, form) in slots::forms(node) {
            if matches!(slot.as_str(), "every" | "timeout")
                && form.value().is_some()
                && slots::duration_ms(form).is_none()
            {
                let field = format!("{path}.{slot}");
                diagnostics.push(Diagnostic::error(
                    parsed.at(&field),
                    &field,
                    "Malformed duration",
                    "Use seconds, minutes, hours or days",
                ));
            }
        }
    }
    for (name, port) in doc.inputs.iter().chain(&doc.types) {
        if let Err(message) = ports::resolve(port, &doc.types) {
            let field = if doc.inputs.contains_key(name) {
                format!("inputs.{name}")
            } else {
                format!("types.{name}")
            };
            if !diagnostics
                .iter()
                .any(|d| d.field == field && d.message == message)
            {
                diagnostics.push(Diagnostic::error(
                    parsed.at(&field),
                    &field,
                    &message,
                    "Declare an acyclic bounded port schema",
                ));
            }
        }
    }
}

pub fn render(diagnostics: &[Diagnostic], explain: bool) -> String {
    fn tree(proof: &Proof, indent: &str, out: &mut String) {
        let location = &proof.atom.location;
        let at = if location.file.is_empty() {
            format!(" @ {}", proof.atom.field)
        } else {
            format!(" @ {}:{}:{}", location.file, location.line, location.column)
        };
        out.push_str(&format!(
            "{indent}{}{}({}) [{}]{at}\n",
            if proof.atom.present { "" } else { "!" },
            proof.atom.predicate,
            proof.atom.arguments.join(", "),
            proof.rule,
        ));
        for child in &proof.children {
            tree(child, &format!("{indent}  "), out);
        }
    }
    fn finding(d: &Diagnostic, indent: &str, explain: bool, out: &mut String) {
        let at = if d.file.is_empty() {
            String::new()
        } else {
            format!("{}:{}:{}: ", d.file, d.line, d.column)
        };
        out.push_str(&format!(
            "{indent}{at}{}: {} {}: {}\n{indent}  hint: {}\n",
            d.severity, d.rule, d.field, d.message, d.hint
        ));
        if explain {
            tree(&d.proof, &format!("{indent}  "), out);
            for (label, atoms) in [("CAUSED BY", &d.failing), ("GIVEN", &d.passing)] {
                out.push_str(&format!(
                    "{indent}  {label}: {}\n",
                    atoms
                        .iter()
                        .map(|a| format!(
                            "{}{}({})",
                            if a.present { "" } else { "!" },
                            a.predicate,
                            a.arguments.join(", ")
                        ))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
        }
        for folded in &d.folded {
            finding(folded, &format!("{indent}  folded: "), explain, out);
        }
    }
    let mut output = String::new();
    for d in diagnostics {
        finding(d, "", explain, &mut output);
    }
    output
}
