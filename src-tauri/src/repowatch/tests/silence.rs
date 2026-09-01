use super::support::*;

/// R5 — precise silence. Every case waits on the channel with a bounded
/// window whose EXPIRY IS THE PASS CONDITION.
#[test]
fn repository_watcher_stays_silent_on_irrelevant_churn() {
    if !git_available() {
        eprintln!("skipping: git is not on PATH");
        return;
    }
    let repo = primary_repo();
    let root = repo.path();
    let gitdir = root.join(".git");

    let (registry, rx) = live_registry();
    let _sub = registry.subscribe(root).expect("subscribe");

    // 1. object writes — a loose object and a pack file.
    settle(&rx);
    write(&gitdir.join("objects").join("ab").join("cdef0123456789"), "loose\n");
    write(&gitdir.join("objects").join("pack").join("pack-test.pack"), "packdata\n");
    expect_silence(&rx, "writing under .git/objects");

    // 2. reflog append.
    settle(&rx);
    let reflog = gitdir.join("logs").join("HEAD");
    write(&reflog, "one reflog line\n");
    write(&reflog, "one reflog line\nanother\n");
    expect_silence(&rx, "appending to .git/logs/HEAD");

    // 3. index.lock churn.
    settle(&rx);
    let lock = gitdir.join("index.lock");
    write(&lock, "");
    std::fs::remove_file(&lock).expect("remove index.lock");
    expect_silence(&rx, "creating and removing .git/index.lock");

    // 4. an ignored path — status_should_ignore gates the working-tree half.
    settle(&rx);
    write(&root.join("ignored").join("build-output.txt"), "noise\n");
    expect_silence(&rx, "writing under an ignored path");

    // 5. the anti-catastrophe oracle: a build-sized burst under an ignored
    // path emits ZERO events and the test finishes inside its bounded window.
    settle(&rx);
    let burst = root.join("ignored").join("burst");
    std::fs::create_dir_all(&burst).expect("burst dir");
    for i in 0..500 {
        write(&burst.join(format!("file-{i}.o")), "object noise\n");
    }
    expect_silence(&rx, "a 500-file burst under an ignored path");
}
