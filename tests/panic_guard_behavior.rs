use autoshade::panic_guard::{Recoverable, panic_is_recoverable};

#[test]
fn a_recoverable_scope_marks_only_its_own_lifetime() {
    assert!(!panic_is_recoverable());
    {
        let _scope = Recoverable::enter();
        assert!(panic_is_recoverable());
    }
    assert!(!panic_is_recoverable());
}

#[test]
fn a_nested_recoverable_scope_does_not_clear_its_outer_scope() {
    let _outer = Recoverable::enter();
    {
        let _inner = Recoverable::enter();
        assert!(panic_is_recoverable());
    }

    assert!(panic_is_recoverable());
}

#[test]
fn a_caught_panic_restores_the_thread_to_fatal_ground() {
    let marked = std::cell::Cell::new(false);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _scope = Recoverable::enter();
        marked.set(panic_is_recoverable());
        panic!("contained test panic");
    }));

    assert!(marked.get());
    assert!(caught.is_err());
    assert!(!panic_is_recoverable());
}
