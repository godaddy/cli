//! Tests for the store selection: resolution order, validation, the saved file
//! and its locking. Kept beside `context.rs` rather than in it to stay under the
//! repository's per-file size limit.

use super::*;

const STORE_A: &str = "6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b";
const STORE_B: &str = "a1e14cc2-29bd-4e99-a1ad-c426cb0266c1";
const STORE_C: &str = "0c3b1a52-77de-4c0e-8f6b-1d2a9e4f5b60";

fn entry(store_id: &str) -> StoreEntry {
    StoreEntry {
        store_id: store_id.to_owned(),
        business_id: None,
        display_name: None,
    }
}

fn registry_with(env: &str, store_id: &str) -> StoreRegistry {
    let mut registry = StoreRegistry::default();
    registry.stores.insert(env.to_owned(), entry(store_id));
    registry
}

fn no_env(_: &str) -> Option<String> {
    None
}

fn resolve(
    env: &str,
    explicit: Option<&str>,
    vars: &[(&str, &str)],
    registry: StoreRegistry,
) -> Result<Option<ResolvedStore>> {
    let vars: Vec<(String, String)> = vars
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect();
    resolve_with(
        env,
        explicit,
        move |name| {
            vars.iter()
                .find(|(var, _)| var == name)
                .map(|(_, value)| value.clone())
        },
        move || Ok(registry),
    )
}

fn resolved(
    env: &str,
    explicit: Option<&str>,
    vars: &[(&str, &str)],
    registry: StoreRegistry,
) -> ResolvedStore {
    resolve(env, explicit, vars, registry)
        .expect("resolves")
        .expect("a store is selected")
}

fn message(error: &CliCoreError) -> String {
    cli_engine::build_error_envelope(error, "commerce")
        .error
        .map(|e| e.message)
        .unwrap_or_default()
}

fn fix(error: &CliCoreError) -> String {
    cli_engine::build_error_envelope(error, "commerce")
        .fix
        .unwrap_or_default()
}

#[test]
fn the_env_var_name_is_the_uppercased_environment() {
    assert_eq!(env_var_for("prod"), "GDDY_STORE_ID_PROD");
    assert_eq!(env_var_for("ote"), "GDDY_STORE_ID_OTE");
    assert_eq!(env_var_for("my-env"), "GDDY_STORE_ID_MY_ENV");
}

#[test]
fn the_flag_beats_every_other_source() {
    let store = resolved(
        "ote",
        Some(STORE_A),
        &[("GDDY_STORE_ID_OTE", STORE_B), ("GDDY_STORE_ID", STORE_B)],
        registry_with("ote", STORE_C),
    );
    assert_eq!(store.store_id, STORE_A);
    assert_eq!(store.source, StoreIdSource::Flag);
    assert!(store.entry.is_none());
}

#[test]
fn the_per_environment_variable_beats_the_bare_variable_and_the_file() {
    let store = resolved(
        "ote",
        None,
        &[("GDDY_STORE_ID_OTE", STORE_A), ("GDDY_STORE_ID", STORE_B)],
        registry_with("ote", STORE_C),
    );
    assert_eq!(store.store_id, STORE_A);
    assert_eq!(store.source, StoreIdSource::EnvVarForEnv);
}

#[test]
fn a_per_environment_variable_for_another_environment_is_ignored() {
    let store = resolved(
        "ote",
        None,
        &[("GDDY_STORE_ID_PROD", STORE_A)],
        registry_with("ote", STORE_C),
    );
    assert_eq!(store.store_id, STORE_C);
    assert_eq!(store.source, StoreIdSource::File);
}

#[test]
fn the_bare_variable_beats_the_file() {
    let store = resolved(
        "ote",
        None,
        &[("GDDY_STORE_ID", STORE_B)],
        registry_with("ote", STORE_C),
    );
    assert_eq!(store.store_id, STORE_B);
    assert_eq!(store.source, StoreIdSource::EnvVar);
}

#[test]
fn the_file_is_used_when_nothing_else_is_set_and_carries_its_entry() {
    let mut registry = StoreRegistry::default();
    registry.stores.insert(
        "ote".to_owned(),
        StoreEntry {
            store_id: STORE_A.to_owned(),
            business_id: Some("biz-1".to_owned()),
            display_name: Some("My Shop".to_owned()),
        },
    );
    let store = resolved("ote", None, &[], registry);
    assert_eq!(store.source, StoreIdSource::File);
    let entry = store.entry.expect("the file entry is carried");
    assert_eq!(entry.business_id.as_deref(), Some("biz-1"));
    assert_eq!(entry.display_name.as_deref(), Some("My Shop"));
}

#[test]
fn nothing_selected_resolves_to_none() {
    let store = resolve("ote", None, &[], StoreRegistry::default()).expect("resolves");
    assert!(store.is_none());
}

#[test]
fn a_selection_for_another_environment_is_not_used() {
    let store = resolve("ote", None, &[], registry_with("prod", STORE_A)).expect("resolves");
    assert!(store.is_none(), "{store:?}");
}

#[test]
fn a_blank_environment_variable_counts_as_unset() {
    let store = resolved(
        "ote",
        None,
        &[("GDDY_STORE_ID_OTE", "  "), ("GDDY_STORE_ID", "")],
        registry_with("ote", STORE_C),
    );
    assert_eq!(store.store_id, STORE_C);
    assert_eq!(store.source, StoreIdSource::File);
}

#[test]
fn a_blank_flag_is_rejected_not_skipped() {
    for blank in ["", "   "] {
        let error = resolve(
            "ote",
            Some(blank),
            &[("GDDY_STORE_ID", STORE_B)],
            registry_with("ote", STORE_C),
        )
        .expect_err("a blank --store-id is an error");
        assert!(
            message(&error).contains("--store-id"),
            "{}",
            message(&error)
        );
    }
}

#[test]
fn an_invalid_id_is_rejected_and_names_where_it_came_from() {
    let from_flag =
        resolve("ote", Some("nope"), &[], StoreRegistry::default()).expect_err("flag is invalid");
    assert!(message(&from_flag).contains("--store-id"));
    assert!(
        fix(&from_flag).contains("Your stores"),
        "{}",
        fix(&from_flag)
    );

    let from_env = resolve(
        "ote",
        None,
        &[("GDDY_STORE_ID_OTE", "nope")],
        StoreRegistry::default(),
    )
    .expect_err("env var is invalid");
    assert!(message(&from_env).contains("GDDY_STORE_ID_OTE"));

    let from_bare = resolve(
        "ote",
        None,
        &[("GDDY_STORE_ID", "nope")],
        StoreRegistry::default(),
    )
    .expect_err("bare env var is invalid");
    assert!(message(&from_bare).contains("GDDY_STORE_ID"));

    let from_file = resolve("ote", None, &[], registry_with("ote", "nope"))
        .expect_err("saved selection is invalid");
    assert!(message(&from_file).contains("saved store"));
}

/// The first source that supplies a value decides, so an invalid value is
/// reported even though a later source would have been valid.
#[test]
fn an_invalid_higher_priority_value_is_not_skipped_for_a_valid_lower_one() {
    let error = resolve(
        "ote",
        None,
        &[("GDDY_STORE_ID_OTE", "nope"), ("GDDY_STORE_ID", STORE_B)],
        registry_with("ote", STORE_C),
    )
    .expect_err("the per-env value is invalid");
    assert!(message(&error).contains("GDDY_STORE_ID_OTE"));
}

#[test]
fn ids_are_canonicalized_to_lowercase_hyphenated_form() {
    let upper = STORE_A.to_uppercase();
    let braced = format!("{{{STORE_A}}}");
    let simple = STORE_A.replace('-', "");
    for spelling in [
        upper.as_str(),
        braced.as_str(),
        simple.as_str(),
        " 6d1f0f4c-8a2e-4a52-9a5b-0f6a3c1d2e4b ",
    ] {
        let store = resolved("ote", Some(spelling), &[], StoreRegistry::default());
        assert_eq!(store.store_id, STORE_A, "{spelling:?}");
    }
}

/// A flag or environment variable that supplies a store must not read the
/// saved selection, so a damaged file is not reported when it would not be
/// used.
#[test]
fn the_saved_selection_is_not_read_when_a_flag_or_variable_supplies_a_store() {
    let reads = std::cell::Cell::new(0_u32);
    let counting_registry = || {
        reads.set(reads.get() + 1);
        Ok(StoreRegistry::default())
    };

    let from_flag =
        resolve_with("ote", Some(STORE_A), no_env, counting_registry).expect("resolves");
    assert_eq!(from_flag.expect("selected").store_id, STORE_A);

    let from_env = resolve_with(
        "ote",
        None,
        |name| (name == STORE_ID_ENV_VAR).then(|| STORE_B.to_owned()),
        counting_registry,
    )
    .expect("resolves");
    assert_eq!(from_env.expect("selected").store_id, STORE_B);

    assert_eq!(
        reads.get(),
        0,
        "the registry was read though it was not needed"
    );

    // The counter is live: it does count a read when nothing else supplies a store.
    let none = resolve_with("ote", None, no_env, counting_registry).expect("resolves");
    assert!(none.is_none());
    assert_eq!(reads.get(), 1);
}

#[test]
fn a_registry_error_is_returned_when_it_is_needed() {
    let error = resolve_with("ote", None, no_env, || {
        Err(CliCoreError::message("registry unreadable"))
    })
    .expect_err("the registry error propagates");
    assert!(message(&error).contains("registry unreadable"));
}

#[test]
fn saving_and_loading_round_trips_an_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("gddy").join(STORE_FILE_NAME);
    save_store_at(
        &path,
        "ote",
        StoreEntry {
            store_id: STORE_A.to_owned(),
            business_id: Some("biz-1".to_owned()),
            display_name: Some("My Shop".to_owned()),
        },
    )
    .expect("save creates the directory and the file");

    let loaded = StoreRegistry::load(&path).expect("load");
    let entry = loaded.stores.get("ote").expect("entry present");
    assert_eq!(entry.store_id, STORE_A);
    assert_eq!(entry.business_id.as_deref(), Some("biz-1"));
    assert_eq!(entry.display_name.as_deref(), Some("My Shop"));
}

#[test]
fn saving_replaces_one_environments_entry_and_keeps_the_others() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STORE_FILE_NAME);
    save_store_at(&path, "ote", entry(STORE_A)).expect("save ote");
    save_store_at(&path, "prod", entry(STORE_B)).expect("save prod");
    save_store_at(&path, "ote", entry(STORE_C)).expect("replace ote");

    let loaded = StoreRegistry::load(&path).expect("load");
    assert_eq!(loaded.stores["ote"].store_id, STORE_C);
    assert_eq!(loaded.stores["prod"].store_id, STORE_B);
}

#[test]
fn two_environments_resolve_to_their_own_stores_through_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STORE_FILE_NAME);
    save_store_at(&path, "ote", entry(STORE_A)).expect("save ote");
    save_store_at(&path, "prod", entry(STORE_B)).expect("save prod");

    let for_env = |env: &str| {
        resolve_with(env, None, no_env, || StoreRegistry::load(&path))
            .expect("resolves")
            .expect("selected")
            .store_id
    };
    assert_eq!(for_env("ote"), STORE_A);
    assert_eq!(for_env("prod"), STORE_B);
}

#[test]
fn clearing_removes_only_the_named_environment() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STORE_FILE_NAME);
    save_store_at(&path, "ote", entry(STORE_A)).expect("save ote");
    save_store_at(&path, "prod", entry(STORE_B)).expect("save prod");

    assert!(clear_store_at(&path, "ote").expect("clear"));

    let loaded = StoreRegistry::load(&path).expect("load");
    assert!(!loaded.stores.contains_key("ote"));
    assert!(loaded.stores.contains_key("prod"));
}

#[test]
fn clearing_something_that_is_not_there_reports_false_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STORE_FILE_NAME);
    assert!(!clear_store_at(&path, "ote").expect("clear on a missing file"));
    assert!(!path.exists(), "clearing must not create the file");

    save_store_at(&path, "prod", entry(STORE_B)).expect("save prod");
    assert!(!clear_store_at(&path, "ote").expect("clear a missing entry"));
}

#[test]
fn a_missing_file_is_an_empty_registry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let registry = StoreRegistry::load(&dir.path().join(STORE_FILE_NAME)).expect("load");
    assert!(registry.stores.is_empty());
}

#[test]
fn a_corrupt_file_is_an_error_that_names_the_file_and_is_not_overwritten() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STORE_FILE_NAME);
    std::fs::write(&path, "stores = [not toml").expect("seed");

    let error = StoreRegistry::load(&path).expect_err("corrupt file is an error");
    assert!(message(&error).contains(&path.display().to_string()));
    assert!(fix(&error).contains("store use"), "{}", fix(&error));

    save_store_at(&path, "ote", entry(STORE_A)).expect_err("saving over a corrupt file fails");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "stores = [not toml",
        "a corrupt file must be left for the user to fix"
    );
}

/// A save used to write whatever it was given, so an uppercase or braced id
/// stayed in that spelling and an invalid one was saved and then failed on
/// the next lookup. The save now enforces what resolution enforces.
#[test]
fn saving_canonicalizes_the_id_so_the_file_holds_one_spelling() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STORE_FILE_NAME);
    let braced = format!("{{{}}}", STORE_A.to_uppercase());
    save_store_at(&path, "ote", entry(&braced)).expect("a valid id in any spelling saves");

    let loaded = StoreRegistry::load(&path).expect("load");
    assert_eq!(loaded.stores["ote"].store_id, STORE_A);
    assert!(
        std::fs::read_to_string(&path)
            .expect("read")
            .contains(STORE_A),
        "the file should contain the canonical spelling"
    );
}

#[test]
fn saving_an_invalid_id_fails_and_changes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("gddy").join(STORE_FILE_NAME);

    let error = save_store_at(&path, "ote", entry("nope")).expect_err("not a store id");
    assert!(
        message(&error).contains("not a store id"),
        "{}",
        message(&error)
    );
    assert!(!path.exists(), "a rejected save must not create the file");
    assert!(
        !path
            .with_file_name(format!("{STORE_FILE_NAME}.lock"))
            .exists(),
        "a rejected save must not create a lock file or directory"
    );

    save_store_at(&path, "prod", entry(STORE_B)).expect("save prod");
    let before = std::fs::read_to_string(&path).expect("read");
    save_store_at(&path, "ote", entry("")).expect_err("a blank id is rejected too");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), before);
}

#[test]
fn saving_keeps_business_id_and_display_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STORE_FILE_NAME);
    let upper = STORE_A.to_uppercase();
    save_store_at(
        &path,
        "ote",
        StoreEntry {
            store_id: upper,
            business_id: Some("biz-1".to_owned()),
            display_name: Some("My Shop".to_owned()),
        },
    )
    .expect("save");
    let loaded = StoreRegistry::load(&path).expect("load");
    assert_eq!(loaded.stores["ote"].business_id.as_deref(), Some("biz-1"));
    assert_eq!(
        loaded.stores["ote"].display_name.as_deref(),
        Some("My Shop")
    );
}

/// The race the lock exists for: with no lock, two writers that read the same
/// snapshot each write back a registry missing the other's entry. Each thread
/// saves its own environment 25 times; if any update were lost, an
/// environment's final entry would be missing or stale.
#[test]
fn concurrent_saves_for_different_environments_lose_no_updates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = std::sync::Arc::new(dir.path().join("gddy").join(STORE_FILE_NAME));
    let stores = [STORE_A, STORE_B, STORE_C];
    let handles: Vec<_> = stores
        .iter()
        .enumerate()
        .map(|(index, store)| {
            let path = std::sync::Arc::clone(&path);
            let store = (*store).to_owned();
            std::thread::spawn(move || {
                for _ in 0..25 {
                    save_store_at(&path, &format!("env{index}"), entry(&store))
                        .expect("save under contention");
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("thread finished");
    }

    let loaded = StoreRegistry::load(&path).expect("load");
    for (index, store) in stores.iter().enumerate() {
        assert_eq!(
            loaded
                .stores
                .get(&format!("env{index}"))
                .map(|e| e.store_id.as_str()),
            Some(*store),
            "env{index}'s entry was lost to a concurrent write: {loaded:?}"
        );
    }
}

#[test]
fn concurrent_saves_and_clears_lose_no_updates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = std::sync::Arc::new(dir.path().join(STORE_FILE_NAME));
    save_store_at(&path, "keep", entry(STORE_A)).expect("seed");

    let writers: Vec<_> = (0..4)
        .map(|index| {
            let path = std::sync::Arc::clone(&path);
            std::thread::spawn(move || {
                for round in 0..20 {
                    let env = format!("tmp{index}");
                    save_store_at(&path, &env, entry(STORE_B)).expect("save");
                    if round % 2 == 0 {
                        clear_store_at(&path, &env).expect("clear");
                    }
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().expect("thread finished");
    }

    let loaded = StoreRegistry::load(&path).expect("load");
    assert_eq!(
        loaded.stores.get("keep").map(|e| e.store_id.as_str()),
        Some(STORE_A),
        "an entry nobody touched was lost: {loaded:?}"
    );
    for index in 0..4 {
        // Round 19 is odd, so each thread's last action was a save.
        assert!(
            loaded.stores.contains_key(&format!("tmp{index}")),
            "tmp{index} was lost: {loaded:?}"
        );
    }
}

/// The lock is a real exclusion, not a no-op: while one handle holds it,
/// another cannot take it, and it is released when the holder is dropped.
#[test]
fn the_registry_lock_excludes_a_second_holder_until_released() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STORE_FILE_NAME);
    let lock_path = path.with_file_name(format!("{STORE_FILE_NAME}.lock"));

    let held = RegistryLock::acquire(&path).expect("first holder");
    let other = std::fs::OpenOptions::new()
        .write(true)
        .open(&lock_path)
        .expect("open the lock file");
    assert!(other.try_lock().is_err(), "a second holder got the lock");

    drop(held);
    other.try_lock().expect("the lock is free once released");
}

#[test]
fn clearing_with_no_file_creates_neither_the_file_nor_a_lock() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("gddy").join(STORE_FILE_NAME);
    assert!(!clear_store_at(&path, "ote").expect("clear"));
    assert!(
        !dir.path().join("gddy").exists(),
        "no directory should be created"
    );
}

/// A path whose parent is a file cannot be looked up, which is an error that
/// is not "missing". Clearing must report it, not say there was nothing to
/// clear (what `Path::exists` would have done). Unix only: other platforms
/// report this case as a missing path.
#[cfg(unix)]
#[test]
fn clearing_reports_a_failed_lookup_instead_of_saying_nothing_was_saved() {
    let dir = tempfile::tempdir().expect("tempdir");
    let parent_is_a_file = dir.path().join("gddy");
    std::fs::write(&parent_is_a_file, "not a directory").expect("seed");
    let path = parent_is_a_file.join(STORE_FILE_NAME);

    let error = clear_store_at(&path, "ote").expect_err("a failed lookup is an error");
    assert!(
        message(&error).contains(&path.display().to_string()),
        "{}",
        message(&error)
    );
    assert!(fix(&error).contains("store use"), "{}", fix(&error));
}

/// The silent-overwrite case: a table with the wrong name is valid TOML and
/// used to parse as an empty registry, so the next save replaced the file and
/// the selection it held was gone.
#[test]
fn a_misspelled_table_is_an_error_and_the_file_is_left_untouched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STORE_FILE_NAME);
    let original = format!("[store.prod]\nstore_id = \"{STORE_B}\"\n");
    std::fs::write(&path, &original).expect("seed");

    let error = StoreRegistry::load(&path).expect_err("an unknown table is an error");
    assert!(message(&error).contains("store"), "{}", message(&error));
    assert!(message(&error).contains(&path.display().to_string()));
    assert!(fix(&error).contains("store use"), "{}", fix(&error));

    save_store_at(&path, "ote", entry(STORE_A)).expect_err("a save must not overwrite it");
    clear_store_at(&path, "prod").expect_err("a clear must not overwrite it");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        original,
        "the file must be left for the user to fix"
    );
}

#[test]
fn a_misspelled_or_unknown_key_inside_an_entry_is_an_error_not_dropped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STORE_FILE_NAME);
    let original = format!("[stores.prod]\nstore_id = \"{STORE_B}\"\nbusinessid = \"typo\"\n");
    std::fs::write(&path, &original).expect("seed");

    let error = StoreRegistry::load(&path).expect_err("an unknown key is an error");
    assert!(
        message(&error).contains("businessid"),
        "{}",
        message(&error)
    );
    save_store_at(&path, "ote", entry(STORE_A)).expect_err("a save must not drop the key");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), original);
}

/// Rejecting unknown fields must not reject what this version writes.
#[test]
fn everything_this_version_writes_still_loads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STORE_FILE_NAME);
    save_store_at(
        &path,
        "ote",
        StoreEntry {
            store_id: STORE_A.to_owned(),
            business_id: Some("biz-1".to_owned()),
            display_name: Some("My Shop".to_owned()),
        },
    )
    .expect("save a full entry");
    save_store_at(&path, "prod", entry(STORE_B)).expect("save a minimal entry");

    let loaded = StoreRegistry::load(&path).expect("what we wrote loads");
    assert_eq!(loaded.stores.len(), 2);
    assert_eq!(
        loaded.stores["ote"].display_name.as_deref(),
        Some("My Shop")
    );
}

#[test]
fn an_empty_file_is_an_empty_registry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(STORE_FILE_NAME);
    std::fs::write(&path, "").expect("seed");
    assert!(
        StoreRegistry::load(&path)
            .expect("empty is valid")
            .stores
            .is_empty()
    );
}

#[cfg(unix)]
#[test]
fn the_lock_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("gddy").join(STORE_FILE_NAME);
    save_store_at(&path, "ote", entry(STORE_A)).expect("save");
    let lock = path.with_file_name(format!("{STORE_FILE_NAME}.lock"));
    let mode = std::fs::metadata(&lock)
        .expect("lock file exists")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn the_saved_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("gddy").join(STORE_FILE_NAME);
    save_store_at(&path, "ote", entry(STORE_A)).expect("save");
    let mode = std::fs::metadata(&path)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}
