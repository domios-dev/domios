#[test]
fn plain_test_can_spawn_async_and_blocking_tasks() {
    let task = super::spawn(async { 21 });
    let blocking = super::spawn_blocking(|| 2);
    assert_eq!(super::block_on(task).unwrap(), 21);
    assert_eq!(super::block_on(blocking).unwrap(), 2);
}
