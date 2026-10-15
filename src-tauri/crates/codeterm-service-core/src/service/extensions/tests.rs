use std::any::TypeId;
use std::collections::HashSet;

use super::{required, Extensions, ExtensionsBuilder};

#[derive(Debug, PartialEq)]
struct Probe(u32);

struct Absent;

#[test]
fn try_get_distinguishes_present_and_absent_types() {
    let mut builder = ExtensionsBuilder::default();
    builder.insert(Probe(7));
    let extensions = Extensions { map: builder.map };
    assert_eq!(extensions.try_get::<Probe>(), Some(&Probe(7)));
    assert!(extensions.try_get::<Absent>().is_none());
}

#[test]
fn get_returns_the_inserted_value() {
    let mut builder = ExtensionsBuilder::default();
    builder.insert(Probe(7));
    let extensions = Extensions { map: builder.map };
    assert_eq!(extensions.get::<Probe>(), &Probe(7));
}

#[test]
#[should_panic(expected = "is registered twice")]
fn duplicate_insert_panics() {
    let mut builder = ExtensionsBuilder::default();
    builder.insert(Probe(1));
    builder.insert(Probe(2));
}

#[test]
#[should_panic(expected = "Absent is not registered")]
fn get_of_a_missing_type_panics_with_its_name() {
    let extensions = Extensions {
        map: Default::default(),
    };
    extensions.get::<Absent>();
}

#[test]
#[should_panic(expected = "is missing")]
fn finish_panics_on_a_missing_required_type() {
    ExtensionsBuilder::default().finish();
}

#[test]
fn required_lists_every_key_once() {
    let keys = required();
    let unique: HashSet<TypeId> = keys.iter().map(|(id, _)| *id).collect();
    assert_eq!(unique.len(), keys.len());
}
