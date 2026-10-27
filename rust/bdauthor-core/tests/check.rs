use bdauthor_core::check::*;

#[test]
fn empty_dir_is_error() {
    let d = std::env::temp_dir().join("bdauthor_check_empty");
    std::fs::create_dir_all(&d).unwrap();
    let r = check_bdmv(&d);
    assert!(!r.ok());
    assert!(r.render().contains("absent"));
}
