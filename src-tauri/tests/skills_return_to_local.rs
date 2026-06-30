mod support;

use rusqlite::params;
use support::SkillTestFixture;

#[cfg(unix)]
fn symlink_file(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(windows)]
fn symlink_file(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(src, dst)
}

#[test]
fn return_to_local_moves_skill_out_of_managed_registry_and_keeps_local_dir() {
    let app = support::TestApp::new();
    let handle = app.handle();

    aio_coding_hub_lib::test_support::init_db(&handle).expect("init db");

    let fix = SkillTestFixture::new(&app, &handle, "codex", "Codex W");

    let managed_local_dir = fix.cli_skills_root.join(&fix.skill_key);
    std::fs::create_dir_all(&managed_local_dir).expect("create managed local dir");
    std::fs::write(
        managed_local_dir.join(".aio-coding-hub.managed"),
        "aio-coding-hub\n",
    )
    .expect("write managed marker");
    std::fs::write(
        managed_local_dir.join("SKILL.md"),
        "name: Context7 managed\n",
    )
    .expect("write managed skill");

    let ok = aio_coding_hub_lib::test_support::skill_return_to_local(
        &handle,
        fix.workspace_id,
        fix.skill_id,
    )
    .expect("return to local");
    assert!(ok, "skill return_to_local should succeed");

    assert!(
        managed_local_dir.exists(),
        "local skill dir should remain after returning"
    );
    assert!(
        managed_local_dir.join("SKILL.md").exists(),
        "local skill dir should contain SKILL.md"
    );
    assert!(
        !managed_local_dir.join(".aio-coding-hub.managed").exists(),
        "returned local skill should be unmanaged"
    );

    assert!(
        !fix.ssot_skill_dir.exists(),
        "ssot skill dir should be deleted after return_to_local"
    );

    let remaining: i64 = fix
        .conn
        .query_row(
            "SELECT COUNT(1) FROM skills WHERE id = ?1",
            params![fix.skill_id],
            |row| row.get(0),
        )
        .expect("count skills");
    assert_eq!(remaining, 0, "skill row should be deleted");
}

#[test]
fn return_to_local_resolves_symlink_entries_inside_ssot_dir() {
    let app = support::TestApp::new();
    let handle = app.handle();

    aio_coding_hub_lib::test_support::init_db(&handle).expect("init db");
    let fix = SkillTestFixture::new(&app, &handle, "codex", "Codex Return Symlink");

    let external_file = app.home_dir().join("external.txt");
    std::fs::write(&external_file, "external\n").expect("write external file");
    if let Err(err) = symlink_file(&external_file, &fix.ssot_skill_dir.join("linked.txt")) {
        eprintln!("skipping symlink return_to_local test: symlink creation unavailable: {err}");
        return;
    }

    let ok = aio_coding_hub_lib::test_support::skill_return_to_local(
        &handle,
        fix.workspace_id,
        fix.skill_id,
    )
    .expect("return to local should succeed with symlink");

    assert!(ok, "skill return_to_local should succeed");

    let local_dir = fix.cli_skills_root.join(&fix.skill_key);
    assert!(
        local_dir.exists(),
        "local skill dir should exist after returning"
    );

    let copied_file = local_dir.join("linked.txt");
    assert!(
        copied_file.exists(),
        "symlink target content should be copied"
    );
    let content = std::fs::read_to_string(&copied_file).expect("read copied file");
    assert_eq!(
        content, "external\n",
        "copied file should contain the symlink target content"
    );

    // The copied file should be a regular file, not a symlink
    let meta = std::fs::symlink_metadata(&copied_file).expect("read metadata");
    assert!(
        !meta.file_type().is_symlink(),
        "copied file should be a regular file, not a symlink"
    );
}
