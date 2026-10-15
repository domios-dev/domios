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

#[test]
fn late_slot_is_optional_until_initialized_and_preserves_first_value() {
    let mut builder = ExtensionsBuilder::default();
    builder.insert_late::<Probe>();
    let extensions = Extensions { map: builder.map };
    assert_eq!(extensions.try_get::<Probe>(), None);
    assert_eq!(extensions.set(Probe(7)), Ok(()));
    assert_eq!(extensions.try_get::<Probe>(), Some(&Probe(7)));
    assert_eq!(extensions.set(Probe(9)), Err(Probe(9)));
    assert_eq!(extensions.get::<Probe>(), &Probe(7));
}

#[test]
fn setting_an_unregistered_or_eager_slot_returns_the_value() {
    let mut builder = ExtensionsBuilder::default();
    builder.insert(Probe(7));
    let extensions = Extensions { map: builder.map };
    assert_eq!(extensions.set(Probe(9)), Err(Probe(9)));
    assert!(extensions.set(Absent).is_err());
    assert_eq!(extensions.get::<Probe>(), &Probe(7));
}

#[test]
#[should_panic(expected = "is registered twice")]
fn late_and_eager_slots_share_one_type_key() {
    let mut builder = ExtensionsBuilder::default();
    builder.insert_late::<Probe>();
    builder.insert(Probe(7));
}

#[test]
fn concurrent_initialization_shares_one_value() {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Barrier};

    let mut builder = ExtensionsBuilder::default();
    builder.insert_late::<Probe>();
    let extensions = Arc::new(Extensions { map: builder.map });
    let barrier = Arc::new(Barrier::new(3));
    let initializations = Arc::new(AtomicU32::new(0));
    std::thread::scope(|scope| {
        for _ in 0..2 {
            scope.spawn(|| {
                barrier.wait();
                assert_eq!(
                    extensions.get_or_init(|| {
                        initializations.fetch_add(1, Ordering::SeqCst);
                        Probe(7)
                    }),
                    &Probe(7)
                );
            });
        }
        barrier.wait();
    });
    assert_eq!(initializations.load(Ordering::SeqCst), 1);
}
