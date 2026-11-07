use crate::boot::{Cover, MAX_COVER_MS, Progress, settled};

fn ready_home() -> Progress {
    Progress { account: true, pictures: true, file_shown: false, joining: false }
}

#[test]
fn home_waits_for_the_account_and_its_pictures() {
    assert!(!settled(false, Progress::default()), "nobody known yet: the signed-out Home must not show");
    assert!(!settled(false, Progress { account: true, ..Progress::default() }), "the cards' pictures are still coming");
    assert!(!settled(false, Progress { pictures: true, ..Progress::default() }));
    assert!(settled(false, ready_home()));
}

#[test]
fn a_file_address_waits_for_the_file_not_for_home() {
    // Home's pictures don't matter: the file opens over it.
    let joining = Progress { account: true, pictures: false, file_shown: false, joining: true };
    assert!(!settled(true, joining), "the file is still joining: Home must not show");
    assert!(settled(true, Progress { file_shown: true, ..joining }));
    // It couldn't be joined: Home (with your account) shows, and says so.
    assert!(settled(true, Progress { joining: false, ..joining }));
    // The file is in before the account answered: still wait for it (the avatar, the team).
    assert!(!settled(true, Progress { account: false, file_shown: true, ..joining }));
}

#[test]
fn the_cover_comes_off_once_two_frames_drew_the_settled_page() {
    let mut cover = Cover::new(false, 0.0);
    assert!(!cover.frame(Progress::default(), 10.0));
    assert!(!cover.frame(ready_home(), 20.0), "the first settled frame only lays the page out");
    assert!(cover.up());
    assert!(cover.frame(ready_home(), 30.0));
    assert!(!cover.up());
    assert!(!cover.frame(ready_home(), 40.0), "it comes off once");
}

#[test]
fn a_page_that_unsettles_starts_counting_again() {
    let mut cover = Cover::new(true, 0.0);
    let open = Progress { account: true, pictures: false, file_shown: true, joining: true };
    assert!(!cover.frame(open, 10.0));
    assert!(!cover.frame(Progress { file_shown: false, ..open }, 20.0));
    assert!(!cover.frame(open, 30.0));
    assert!(cover.frame(open, 40.0));
}

#[test]
fn a_silent_server_cannot_keep_the_page_covered() {
    let mut cover = Cover::new(false, 1_000.0);
    assert!(!cover.frame(Progress::default(), 1_000.0 + MAX_COVER_MS - 1.0));
    assert!(cover.frame(Progress::default(), 1_000.0 + MAX_COVER_MS));
    // A clock that isn't a number (it can't be compared) uncovers too.
    let mut cover = Cover::new(false, f64::NAN);
    assert!(cover.frame(Progress::default(), 5.0));
}
