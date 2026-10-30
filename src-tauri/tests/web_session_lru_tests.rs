//! Sites made up at run time are bounded: past the limit, the least recently used one gives
//! way. In a file of its own because the sites are process-wide and this test fills them.

use anix_lib::web_session::{least_recently_used, site_for_origin};

#[test]
fn the_least_recently_used_site_not_in_use_gives_way() {
    let entries = [("a", 5, false), ("b", 2, false), ("c", 1, true), ("d", 9, false)];
    assert_eq!(least_recently_used(entries), Some("b"));
    assert_eq!(least_recently_used([("a", 1, true), ("b", 2, true)]), None);
    assert_eq!(least_recently_used(Vec::<(&str, u64, bool)>::new()), None);
}

#[test]
fn new_sites_are_still_given_after_many_origins() {
    let first = site_for_origin("https://first.example").unwrap();
    for n in 0..300 {
        let origin = format!("https://mirror{}.example", n);
        let site = site_for_origin(&origin).unwrap_or_else(|| panic!("no site for {}", origin));
        assert_eq!(site.origins, &[origin.as_str()][..]);
        // Asking again gives the same site.
        assert!(std::ptr::eq(site, site_for_origin(&origin).unwrap()));
    }
    // The oldest one was evicted and comes back as a fresh site for the same origin.
    let again = site_for_origin("https://first.example").unwrap();
    assert_eq!(again.key, first.key);
    assert!(!std::ptr::eq(again, first));
}
