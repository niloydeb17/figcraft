use crate::navigation::Navigation;

#[test]
fn returning_home_removes_the_file_that_reload_would_reopen() {
    let mut navigation = Navigation::default();
    let mut fragment = "#file=K7M-2QX-9TD".to_string();
    assert_eq!(navigation.follow(false, &fragment), None);
    if let Some(next) = navigation.follow(true, &fragment) {
        fragment = next.to_string();
    }
    assert_eq!(fragment, "", "Home must not reload the previously open cloud file");
    assert_eq!(navigation.follow(true, &fragment), None, "no repeated history entries");
}

#[test]
fn startup_keeps_deep_links_while_the_file_is_joining() {
    for fragment in ["#file=K7M-2QX-9TD", "#join=K7M-2QX-9TD", "#x=1&join=K7M-2QX-9TD"] {
        let mut navigation = Navigation::default();
        assert_eq!(navigation.follow(true, fragment), None);
        assert_eq!(navigation.follow(true, fragment), None, "joining may take multiple frames");
        assert_eq!(navigation.follow(false, fragment), None, "the editor keeps its deep link");
        assert_eq!(navigation.follow(true, fragment), Some(""), "returning Home clears the route");
    }
}

#[test]
fn home_does_not_change_unrelated_fragments() {
    for fragment in ["", "#", "#help", "#profile=me", "#rejoin=x"] {
        let mut navigation = Navigation::default();
        assert_eq!(navigation.follow(false, fragment), None);
        assert_eq!(navigation.follow(true, fragment), None);
    }
}

#[test]
fn each_editor_to_home_visit_clears_its_own_file() {
    let mut navigation = Navigation::default();
    for fragment in ["#file=first", "#file=second", "#file=first"] {
        assert_eq!(navigation.follow(false, fragment), None);
        assert_eq!(navigation.follow(true, fragment), Some(""));
        assert_eq!(navigation.follow(true, ""), None);
    }
}
