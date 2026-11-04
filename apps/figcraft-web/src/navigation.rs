//! Keep a return to Home from leaving an editor route in the page's address.

#[derive(Default)]
pub struct Navigation {
    was_editing: bool,
}

impl Navigation {
    /// The replacement fragment when leaving the editor for Home. Initial Home frames may be
    /// waiting for an address-based join, so they must not erase that deep link before it opens.
    pub fn follow(&mut self, home: bool, fragment: &str) -> Option<&'static str> {
        let was_editing = std::mem::replace(&mut self.was_editing, !home);
        let file_address = fragment.starts_with("#file=") || crate::query::join_link(fragment).is_some();
        (was_editing && home && file_address).then_some("")
    }
}
