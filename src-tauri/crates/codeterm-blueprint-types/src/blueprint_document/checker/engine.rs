use super::{Proof, ProofAtom};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Atom {
    pub predicate: &'static str,
    pub arguments: Vec<String>,
}
impl Atom {
    pub fn new(predicate: &'static str, arguments: Vec<String>) -> Self {
        Self {
            predicate,
            arguments,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) enum Term {
    Var(String),
    Constant(String),
    Wildcard,
}
impl Term {
    fn parse(text: &str) -> Self {
        if text == "_" {
            Self::Wildcard
        } else if text.starts_with(|c: char| c.is_ascii_uppercase()) {
            Self::Var(text.into())
        } else {
            Self::Constant(text.into())
        }
    }
    fn value(&self, env: &BTreeMap<String, String>) -> Option<String> {
        match self {
            Self::Var(name) => env.get(name).cloned(),
            Self::Constant(value) => Some(value.clone()),
            Self::Wildcard => None,
        }
    }
}
#[derive(Clone, Debug)]
pub(super) struct Pattern {
    pub predicate: &'static str,
    pub terms: Vec<Term>,
}
pub(super) fn atom(predicate: &'static str, terms: &[&str]) -> Pattern {
    Pattern {
        predicate,
        terms: terms.iter().map(|s| Term::parse(s)).collect(),
    }
}
#[derive(Clone, Debug)]
pub(super) enum Literal {
    Positive(Pattern),
    Negative(Pattern),
    Compare(&'static str, Term, Term),
}
pub(super) fn yes(predicate: &'static str, terms: &[&str]) -> Literal {
    Literal::Positive(atom(predicate, terms))
}
pub(super) fn no(predicate: &'static str, terms: &[&str]) -> Literal {
    Literal::Negative(atom(predicate, terms))
}
pub(super) fn cmp(op: &'static str, left: &str, right: &str) -> Literal {
    Literal::Compare(op, Term::parse(left), Term::parse(right))
}
#[derive(Clone, Debug)]
pub(super) struct Rule {
    pub id: &'static str,
    pub head: Pattern,
    pub body: Vec<Literal>,
}
pub(super) fn rule(id: &'static str, head: Pattern, body: Vec<Literal>) -> Rule {
    Rule { id, head, body }
}

#[derive(Clone, Debug)]
pub(super) struct Witness {
    pub atom: ProofAtom,
    pub rule: &'static str,
    pub premises: Vec<Premise>,
}
#[derive(Clone, Debug)]
pub(super) enum Premise {
    Fact(Atom),
    Test(&'static str, Vec<String>, bool),
}
#[derive(Default)]
pub(super) struct Database {
    pub facts: BTreeMap<Atom, Witness>,
    relations: BTreeMap<&'static str, BTreeSet<Vec<String>>>,
}
impl Database {
    pub fn insert(&mut self, atom: Atom, witness: Witness) -> bool {
        if self.facts.contains_key(&atom) {
            return false;
        }
        self.relations
            .entry(atom.predicate)
            .or_default()
            .insert(atom.arguments.clone());
        self.facts.insert(atom, witness);
        true
    }
    pub fn edb(
        &mut self,
        predicate: &'static str,
        arguments: Vec<String>,
        field: &str,
        location: super::super::Location,
    ) {
        let atom = Atom::new(predicate, arguments.clone());
        self.insert(
            atom,
            Witness {
                atom: ProofAtom::new(predicate, arguments, true, field, location),
                rule: "fact",
                premises: Vec::new(),
            },
        );
    }
    pub fn rows(&self, predicate: &'static str) -> impl Iterator<Item = &Vec<String>> {
        self.relations.get(predicate).into_iter().flatten()
    }
    pub fn proof(&self, fact: &Atom) -> Option<Proof> {
        let w = self.facts.get(fact)?;
        let children = w
            .premises
            .iter()
            .filter_map(|p| match p {
                Premise::Fact(f) => self.proof(f),
                Premise::Test(predicate, args, present) => Some(Proof {
                    atom: ProofAtom::new(
                        predicate,
                        args.clone(),
                        *present,
                        &w.atom.field,
                        w.atom.location.clone(),
                    ),
                    rule: if *present { "comparison" } else { "absence" }.into(),
                    children: Vec::new(),
                }),
            })
            .collect();
        Some(Proof {
            atom: w.atom.clone(),
            rule: w.rule.into(),
            children,
        })
    }
    pub fn support(&self, fact: &Atom) -> BTreeSet<Atom> {
        let mut support = BTreeSet::new();
        let mut pending = vec![fact.clone()];
        while let Some(fact) = pending.pop() {
            if let Some(w) = self.facts.get(&fact) {
                for premise in &w.premises {
                    if let Premise::Fact(f) = premise {
                        if support.insert(f.clone()) {
                            pending.push(f.clone());
                        }
                    }
                }
            }
        }
        support
    }
    pub fn evaluate(&mut self, rules: &[Rule]) -> Result<(), String> {
        let strata = stratify(rules)?;
        for level in 0..=strata.values().copied().max().unwrap_or(0) {
            let selected = rules
                .iter()
                .filter(|r| strata[r.head.predicate] == level)
                .collect::<Vec<_>>();
            let mut delta = BTreeSet::new();
            for r in &selected {
                self.fire(r, None, &BTreeSet::new(), &mut delta)?;
            }
            while !delta.is_empty() {
                let mut next = BTreeSet::new();
                for r in &selected {
                    for (index, literal) in r.body.iter().enumerate() {
                        if let Literal::Positive(p) = literal {
                            if strata[p.predicate] == level
                                && delta.iter().any(|f| f.predicate == p.predicate)
                            {
                                self.fire(r, Some(index), &delta, &mut next)?;
                            }
                        }
                    }
                }
                delta = next;
            }
        }
        Ok(())
    }
    fn fire(
        &mut self,
        rule: &Rule,
        delta_at: Option<usize>,
        delta: &BTreeSet<Atom>,
        out: &mut BTreeSet<Atom>,
    ) -> Result<(), String> {
        let mut solutions = vec![(BTreeMap::new(), Vec::new())];
        for (index, literal) in rule.body.iter().enumerate() {
            let Literal::Positive(pattern) = literal else {
                continue;
            };
            let mut next = Vec::new();
            for (env, premises) in solutions {
                for row in self.rows(pattern.predicate) {
                    let fact = Atom::new(pattern.predicate, row.clone());
                    if delta_at == Some(index) && !delta.contains(&fact) {
                        continue;
                    }
                    if let Some(bound) = unify(pattern, row, &env) {
                        let mut premises = premises.clone();
                        premises.push(Premise::Fact(fact));
                        next.push((bound, premises));
                        if next.len() > 100_000 {
                            return Err("Checker join bound exceeded".into());
                        }
                    }
                }
            }
            solutions = next;
        }
        for (env, mut premises) in solutions {
            let mut satisfied = true;
            for literal in &rule.body {
                match literal {
                    Literal::Positive(_) => {}
                    Literal::Negative(p) => {
                        if self
                            .rows(p.predicate)
                            .any(|row| unify(p, row, &env).is_some())
                        {
                            satisfied = false;
                            break;
                        }
                        let args = p
                            .terms
                            .iter()
                            .map(|t| t.value(&env).unwrap_or("_".into()))
                            .collect();
                        premises.push(Premise::Test(p.predicate, args, false));
                    }
                    Literal::Compare(op, a, b) => {
                        let a = a.value(&env).ok_or("Unsafe comparison")?;
                        let b = b.value(&env).ok_or("Unsafe comparison")?;
                        let order = match (a.parse::<i128>(), b.parse::<i128>()) {
                            (Ok(a), Ok(b)) => a.cmp(&b),
                            _ => a.cmp(&b),
                        };
                        let hit = match *op {
                            "=" => order.is_eq(),
                            "!=" => !order.is_eq(),
                            "<" => order.is_lt(),
                            "<=" => !order.is_gt(),
                            ">" => order.is_gt(),
                            ">=" => !order.is_lt(),
                            _ => return Err("Unknown comparison".into()),
                        };
                        if !hit {
                            satisfied = false;
                            break;
                        }
                        premises.push(Premise::Test("compare", vec![(*op).into(), a, b], true));
                    }
                }
            }
            if !satisfied {
                continue;
            }
            let args = rule
                .head
                .terms
                .iter()
                .map(|t| t.value(&env).ok_or("Unsafe head"))
                .collect::<Result<Vec<_>, _>>()?;
            let fact = Atom::new(rule.head.predicate, args.clone());
            if self.facts.contains_key(&fact) {
                continue;
            }
            let source = premises
                .iter()
                .find_map(|p| match p {
                    Premise::Fact(f) => self.facts.get(f),
                    _ => None,
                })
                .ok_or("Rule has no positioned premise")?;
            let witness = Witness {
                atom: ProofAtom::new(
                    fact.predicate,
                    args,
                    true,
                    &source.atom.field,
                    source.atom.location.clone(),
                ),
                rule: rule.id,
                premises,
            };
            if self.insert(fact.clone(), witness) {
                out.insert(fact);
            }
            if self.facts.len() > 100_000 {
                return Err("Checker fact bound exceeded".into());
            }
        }
        Ok(())
    }
}
fn unify(
    pattern: &Pattern,
    row: &[String],
    env: &BTreeMap<String, String>,
) -> Option<BTreeMap<String, String>> {
    if row.len() != pattern.terms.len() {
        return None;
    }
    let mut bound = env.clone();
    for (term, value) in pattern.terms.iter().zip(row) {
        match term {
            Term::Wildcard => {}
            Term::Constant(want) if want != value => return None,
            Term::Var(name) => {
                if bound.get(name).is_some_and(|want| want != value) {
                    return None;
                }
                bound.insert(name.clone(), value.clone());
            }
            _ => {}
        }
    }
    Some(bound)
}
fn stratify(rules: &[Rule]) -> Result<BTreeMap<&'static str, usize>, String> {
    let mut strata = BTreeMap::new();
    let mut arities = BTreeMap::new();
    let mut labels = BTreeSet::new();
    for rule in rules {
        if !labels.insert(rule.id) {
            return Err(format!("Duplicate rule label {}", rule.id));
        }
        let mut bound = BTreeSet::new();
        for literal in &rule.body {
            if let Literal::Positive(p) = literal {
                for term in &p.terms {
                    if let Term::Var(v) = term {
                        bound.insert(v);
                    }
                }
            }
        }
        let mut required = rule.head.terms.iter().collect::<Vec<_>>();
        for literal in &rule.body {
            match literal {
                Literal::Negative(p) => {
                    required.extend(p.terms.iter().filter(|t| !matches!(t, Term::Wildcard)))
                }
                Literal::Compare(_, a, b) => required.extend([a, b]),
                _ => {}
            }
        }
        if required
            .iter()
            .any(|t| matches!(t, Term::Wildcard) || matches!(t, Term::Var(v) if !bound.contains(v)))
        {
            return Err(format!("Unsafe rule {}", rule.id));
        }
        for p in std::iter::once(&rule.head).chain(rule.body.iter().filter_map(|l| match l {
            Literal::Positive(p) | Literal::Negative(p) => Some(p),
            _ => None,
        })) {
            strata.entry(p.predicate).or_insert(0);
            if arities
                .insert(p.predicate, p.terms.len())
                .is_some_and(|n| n != p.terms.len())
            {
                return Err(format!("Inconsistent arity {}", p.predicate));
            }
        }
    }
    for iteration in 0..=strata.len() {
        let mut changed = false;
        for rule in rules {
            for literal in &rule.body {
                let (p, negative) = match literal {
                    Literal::Positive(p) => (p, false),
                    Literal::Negative(p) => (p, true),
                    _ => continue,
                };
                let needed = strata[p.predicate] + usize::from(negative);
                if needed > strata[rule.head.predicate] {
                    strata.insert(rule.head.predicate, needed);
                    changed = true;
                }
            }
        }
        if !changed {
            return Ok(strata);
        }
        if iteration == strata.len() {
            return Err("Unstratified negative cycle".into());
        }
    }
    Err("Cannot stratify rules".into())
}
